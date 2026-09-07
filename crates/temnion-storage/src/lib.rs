// SPDX-License-Identifier: AGPL-3.0-only
//! Single-writer durable source logs. WAL data is retained after TSF export.
//!
//! A successful append acknowledges `File::sync_all`, not just queue admission.
//! On an I/O error the commit outcome is unknown and the writer is poisoned:
//! reopen, inspect, and reconcile before retrying.

use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use temnion_core::{
    ClockId, DatabaseId, EntityId, EventId, EventTimes, SchemaId, SourceEpoch, SourceId, Timestamp,
};
use temnion_events::HistoryFilter;
use temnion_format::{
    BATCH_HEADER_LEN, FormatError, Limits, StoredEvent, WAL_HEADER_LEN, WalHeader, decode_batch,
    decode_segment, decode_wal_header, encode_batch, encode_segment, encode_wal_header,
    frame_length,
};

#[derive(Debug)]
pub enum StorageError {
    Io {
        operation: &'static str,
        source: io::Error,
    },
    Format(FormatError),
    Entropy(String),
    AllocationFailed,
    AlreadyExists,
    WriterBusy,
    Poisoned,
    CommitOutcomeUnknown(Box<StorageError>),
    EmptyBatch,
    InvalidLimits,
    SequenceExhausted,
    KnownClockMismatch {
        expected: ClockId,
        actual: ClockId,
    },
    KnownTimeRegression {
        previous: u64,
        actual: u64,
    },
    InvalidRecord(&'static str),
    IncompleteTail {
        offset: u64,
        bytes: u64,
    },
    UnknownEvent(EventId),
    InvalidBudget,
    BudgetTooSmall {
        frame_bytes: usize,
        frame_records: usize,
    },
    InvalidCursor,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
            Self::Format(error) => write!(f, "invalid persistent data: {error}"),
            Self::Entropy(error) => write!(f, "OS entropy unavailable: {error}"),
            Self::AllocationFailed => write!(f, "could not reserve bounded storage metadata"),
            Self::AlreadyExists => write!(f, "database already exists"),
            Self::WriterBusy => write!(f, "database is locked by another writer"),
            Self::Poisoned => write!(f, "writer has an unknown I/O outcome; reopen and reconcile"),
            Self::CommitOutcomeUnknown(error) => write!(
                f,
                "append commit outcome unknown; reopen and reconcile: {error}"
            ),
            Self::EmptyBatch => write!(f, "a durable batch must not be empty"),
            Self::InvalidLimits => write!(
                f,
                "storage limits must be positive and permit format headers"
            ),
            Self::SequenceExhausted => write!(f, "source sequence or file offset exhausted"),
            Self::KnownClockMismatch { expected, actual } => {
                write!(f, "known clock {actual:?} differs from {expected:?}")
            }
            Self::KnownTimeRegression { previous, actual } => {
                write!(f, "known time regressed from {previous} to {actual}")
            }
            Self::InvalidRecord(reason) => write!(f, "invalid durable record: {reason}"),
            Self::IncompleteTail { offset, bytes } => write!(
                f,
                "incomplete WAL tail at byte {offset} ({bytes} bytes); explicit recovery is required"
            ),
            Self::UnknownEvent(id) => write!(f, "unknown durable event {id:?}"),
            Self::InvalidBudget => write!(f, "query budgets must be positive"),
            Self::BudgetTooSmall {
                frame_bytes,
                frame_records,
            } => write!(
                f,
                "budget must allow one complete frame: {frame_bytes} bytes, {frame_records} records"
            ),
            Self::InvalidCursor => {
                write!(f, "cursor does not match the database, filter, or snapshot")
            }
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Format(error) => Some(error),
            Self::CommitOutcomeUnknown(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<FormatError> for StorageError {
    fn from(error: FormatError) -> Self {
        Self::Format(error)
    }
}

fn io_error(operation: &'static str) -> impl FnOnce(io::Error) -> StorageError {
    move |source| StorageError::Io { operation, source }
}

fn random_id() -> Result<DatabaseId, StorageError> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|error| StorageError::Entropy(error.to_string()))?;
    Ok(DatabaseId(bytes))
}

fn validate_limits(limits: &Limits) -> Result<(), StorageError> {
    if limits.max_frame_bytes <= BATCH_HEADER_LEN
        || limits.max_records == 0
        || limits.max_payload_bytes == 0
        || limits.max_segment_bytes <= WAL_HEADER_LEN
    {
        return Err(StorageError::InvalidLimits);
    }
    Ok(())
}

fn lock_directory(root: &Path) -> Result<File, StorageError> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".writer.lock"))
        .map_err(io_error("open writer lock"))?;
    match fs2::FileExt::try_lock_exclusive(&lock) {
        Ok(()) => Ok(lock),
        Err(error)
            if error.kind() == io::ErrorKind::WouldBlock
                || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            Err(StorageError::WriterBusy)
        }
        Err(error) => Err(io_error("lock database")(error)),
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), StorageError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(io_error("synchronize directory"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryMode {
    RejectIncompleteTail,
    TruncateIncompleteTail,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub recovered_events: u64,
    pub discarded_tail_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct WriteEvent {
    pub entity: EntityId,
    pub times: EventTimes,
    pub schema: SchemaId,
    pub payload: Vec<u8>,
    pub causes: Vec<EventId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DurableReceipt {
    pub database: DatabaseId,
    pub first: EventId,
    pub last: EventId,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageQueryBudget {
    pub max_results: usize,
    /// Bounds decoded records, including prefix records in a resumed frame.
    pub max_scanned: usize,
    pub max_read_bytes: usize,
}

impl Default for StorageQueryBudget {
    fn default() -> Self {
        Self {
            max_results: 1024,
            max_scanned: 65_536,
            max_read_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageCursor {
    database: DatabaseId,
    source: SourceId,
    epoch: SourceEpoch,
    filter: HistoryFilter,
    next_sequence: u64,
    snapshot_end: u64,
}

#[derive(Debug)]
pub struct StoragePage {
    pub events: Vec<StoredEvent>,
    pub scanned: usize,
    pub bytes_read: usize,
    pub continuation: Option<StorageCursor>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SealReport {
    pub segments_created: usize,
    pub segments_existing: usize,
    pub events: u64,
}

#[derive(Clone, Copy, Debug)]
struct FrameLocation {
    offset: u64,
    length: usize,
    first: u64,
    count: usize,
}

#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    wal: File,
    header: WalHeader,
    limits: Limits,
    frames: Vec<FrameLocation>,
    next_sequence: u64,
    end_offset: u64,
    last_known: Option<Timestamp>,
    poisoned: bool,
    #[cfg(test)]
    fail_before_sync: bool,
    _lock: File,
}

impl Store {
    pub fn create(
        path: impl AsRef<Path>,
        source: SourceId,
        epoch: SourceEpoch,
        limits: Limits,
    ) -> Result<Self, StorageError> {
        validate_limits(&limits)?;
        let root = path.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(io_error("create database directory"))?;
        let lock = lock_directory(&root)?;
        let header = WalHeader {
            database: random_id()?,
            source,
            epoch,
        };
        let mut wal = match OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(root.join("events.wal"))
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(StorageError::AlreadyExists);
            }
            Err(error) => return Err(io_error("create WAL")(error)),
        };
        let encoded = encode_wal_header(header);
        wal.write_all(&encoded)
            .map_err(io_error("write WAL header"))?;
        wal.sync_all().map_err(io_error("synchronize WAL header"))?;
        #[cfg(unix)]
        for directory in fs::canonicalize(&root)
            .map_err(io_error("resolve database directory"))?
            .ancestors()
        {
            sync_directory(directory)?;
        }
        Ok(Self {
            root,
            wal,
            header,
            limits,
            frames: Vec::new(),
            next_sequence: 0,
            end_offset: encoded.len() as u64,
            last_known: None,
            poisoned: false,
            #[cfg(test)]
            fail_before_sync: false,
            _lock: lock,
        })
    }

    pub fn open(
        path: impl AsRef<Path>,
        limits: Limits,
        recovery: RecoveryMode,
    ) -> Result<(Self, RecoveryReport), StorageError> {
        validate_limits(&limits)?;
        let root = path.as_ref().to_path_buf();
        let lock = lock_directory(&root)?;
        let mut wal = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join("events.wal"))
            .map_err(io_error("open WAL"))?;
        let length = wal.metadata().map_err(io_error("inspect WAL"))?.len();
        let mut bytes = vec![0u8; length.min(WAL_HEADER_LEN as u64) as usize];
        wal.read_exact(&mut bytes)
            .map_err(io_error("read WAL header"))?;
        let header = decode_wal_header(&bytes)?;
        let mut store = Self {
            root,
            wal,
            header,
            limits,
            frames: Vec::new(),
            next_sequence: 0,
            end_offset: WAL_HEADER_LEN as u64,
            last_known: None,
            poisoned: false,
            #[cfg(test)]
            fail_before_sync: false,
            _lock: lock,
        };
        let mut discarded_tail_bytes = 0;
        while store.end_offset < length {
            let remaining = length - store.end_offset;
            let incomplete = if remaining < BATCH_HEADER_LEN as u64 {
                true
            } else {
                let mut prefix = vec![0; BATCH_HEADER_LEN];
                store
                    .wal
                    .seek(SeekFrom::Start(store.end_offset))
                    .map_err(io_error("seek WAL frame"))?;
                store
                    .wal
                    .read_exact(&mut prefix)
                    .map_err(io_error("read WAL frame header"))?;
                let frame_bytes = frame_length(&prefix, &store.limits)?;
                if frame_bytes as u64 > remaining {
                    true
                } else {
                    let records = store.read_frame_at(store.end_offset, frame_bytes)?;
                    if records.is_empty() {
                        return Err(StorageError::InvalidRecord("empty WAL batch"));
                    }
                    let first = store.next_sequence;
                    let mut known = store.last_known;
                    for record in &records {
                        validate_record(store.header, store.next_sequence, known, record)?;
                        known = Some(record.times.known);
                        store.next_sequence = store
                            .next_sequence
                            .checked_add(1)
                            .ok_or(StorageError::SequenceExhausted)?;
                    }
                    store
                        .frames
                        .try_reserve(1)
                        .map_err(|_| StorageError::AllocationFailed)?;
                    store.frames.push(FrameLocation {
                        offset: store.end_offset,
                        length: frame_bytes,
                        first,
                        count: records.len(),
                    });
                    store.last_known = known;
                    store.end_offset = store
                        .end_offset
                        .checked_add(frame_bytes as u64)
                        .ok_or(StorageError::SequenceExhausted)?;
                    false
                }
            };
            if incomplete {
                match recovery {
                    RecoveryMode::RejectIncompleteTail => {
                        return Err(StorageError::IncompleteTail {
                            offset: store.end_offset,
                            bytes: remaining,
                        });
                    }
                    RecoveryMode::TruncateIncompleteTail => {
                        store
                            .wal
                            .set_len(store.end_offset)
                            .map_err(io_error("truncate incomplete WAL tail"))?;
                        store
                            .wal
                            .sync_all()
                            .map_err(io_error("synchronize WAL recovery"))?;
                        discarded_tail_bytes = remaining;
                        break;
                    }
                }
            }
        }
        store
            .wal
            .seek(SeekFrom::Start(store.end_offset))
            .map_err(io_error("position WAL writer"))?;
        let report = RecoveryReport {
            recovered_events: store.next_sequence,
            discarded_tail_bytes,
        };
        Ok((store, report))
    }

    pub fn header(&self) -> WalHeader {
        self.header
    }

    pub fn len(&self) -> u64 {
        self.next_sequence
    }

    pub fn is_empty(&self) -> bool {
        self.next_sequence == 0
    }

    pub fn wal_bytes(&self) -> u64 {
        self.end_offset
    }

    /// Starts a bounded scan at a source sequence in the current durable prefix.
    /// This is an in-process cursor, not a trusted network continuation token.
    pub fn cursor_from_sequence(
        &self,
        sequence: u64,
        filter: HistoryFilter,
    ) -> Result<StorageCursor, StorageError> {
        if self.poisoned {
            return Err(StorageError::Poisoned);
        }
        if sequence > self.next_sequence {
            return Err(StorageError::InvalidCursor);
        }
        Ok(StorageCursor {
            database: self.header.database,
            source: self.header.source,
            epoch: self.header.epoch,
            filter,
            next_sequence: sequence,
            snapshot_end: self.next_sequence,
        })
    }

    pub fn append(&mut self, batch: Vec<WriteEvent>) -> Result<DurableReceipt, StorageError> {
        if self.poisoned {
            return Err(StorageError::Poisoned);
        }
        if batch.is_empty() {
            return Err(StorageError::EmptyBatch);
        }
        if batch.len() > self.limits.max_records {
            return Err(FormatError::LimitExceeded("records per batch").into());
        }
        let end = self
            .next_sequence
            .checked_add(batch.len() as u64)
            .ok_or(StorageError::SequenceExhausted)?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(batch.len())
            .map_err(|_| StorageError::AllocationFailed)?;
        let mut known = self.last_known;
        for (index, input) in batch.into_iter().enumerate() {
            let record = StoredEvent {
                id: EventId {
                    source: self.header.source,
                    epoch: self.header.epoch,
                    sequence: self.next_sequence + index as u64,
                },
                entity: input.entity,
                times: input.times,
                schema: input.schema,
                payload: input.payload,
                causes: input.causes,
            };
            validate_record(self.header, record.id.sequence, known, &record)?;
            known = Some(record.times.known);
            records.push(record);
        }
        let encoded = encode_batch(&records, &self.limits)?;
        let new_offset = self
            .end_offset
            .checked_add(encoded.len() as u64)
            .ok_or(StorageError::SequenceExhausted)?;
        self.frames
            .try_reserve(1)
            .map_err(|_| StorageError::AllocationFailed)?;
        let write_result = (|| {
            if self
                .wal
                .metadata()
                .map_err(io_error("inspect append position"))?
                .len()
                != self.end_offset
            {
                return Err(StorageError::InvalidRecord(
                    "WAL changed outside the locked writer",
                ));
            }
            self.wal
                .seek(SeekFrom::Start(self.end_offset))
                .map_err(io_error("seek append position"))?;
            self.wal
                .write_all(&encoded)
                .map_err(io_error("append WAL batch"))?;
            #[cfg(test)]
            if self.fail_before_sync {
                return Err(io_error("synchronize WAL batch")(io::Error::other(
                    "injected sync failure",
                )));
            }
            self.wal
                .sync_all()
                .map_err(io_error("synchronize WAL batch"))?;
            Ok(())
        })();
        if let Err(error) = write_result {
            self.poisoned = true;
            return Err(StorageError::CommitOutcomeUnknown(Box::new(error)));
        }
        let receipt = DurableReceipt {
            database: self.header.database,
            first: records[0].id,
            last: records[records.len() - 1].id,
            count: records.len(),
        };
        self.frames.push(FrameLocation {
            offset: self.end_offset,
            length: encoded.len(),
            first: self.next_sequence,
            count: records.len(),
        });
        self.next_sequence = end;
        self.end_offset = new_offset;
        self.last_known = known;
        Ok(receipt)
    }

    fn read_frame_at(
        &mut self,
        offset: u64,
        length: usize,
    ) -> Result<Vec<StoredEvent>, StorageError> {
        if length > self.limits.max_frame_bytes {
            return Err(FormatError::LimitExceeded("frame bytes").into());
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| StorageError::AllocationFailed)?;
        bytes.resize(length, 0);
        self.wal
            .seek(SeekFrom::Start(offset))
            .map_err(io_error("seek history frame"))?;
        self.wal
            .read_exact(&mut bytes)
            .map_err(io_error("read history frame"))?;
        Ok(decode_batch(&bytes, &self.limits)?)
    }

    fn frame_for_sequence(&self, sequence: u64) -> Option<FrameLocation> {
        let index = self.frames.partition_point(|frame| frame.first <= sequence);
        index.checked_sub(1).map(|index| self.frames[index])
    }

    fn read_indexed_frame(
        &mut self,
        frame: FrameLocation,
    ) -> Result<Vec<StoredEvent>, StorageError> {
        let records = self.read_frame_at(frame.offset, frame.length)?;
        if records.len() != frame.count {
            return Err(StorageError::InvalidRecord(
                "frame count changed after recovery",
            ));
        }
        let mut known = None;
        for (index, record) in records.iter().enumerate() {
            validate_record(self.header, frame.first + index as u64, known, record)?;
            if self
                .last_known
                .is_some_and(|last| last.clock != record.times.known.clock)
            {
                return Err(StorageError::InvalidRecord(
                    "frame clock changed after recovery",
                ));
            }
            known = Some(record.times.known);
        }
        Ok(records)
    }

    pub fn get(&mut self, id: EventId) -> Result<StoredEvent, StorageError> {
        if self.poisoned {
            return Err(StorageError::Poisoned);
        }
        if id.source != self.header.source
            || id.epoch != self.header.epoch
            || id.sequence >= self.next_sequence
        {
            return Err(StorageError::UnknownEvent(id));
        }
        let frame = self
            .frame_for_sequence(id.sequence)
            .ok_or(StorageError::UnknownEvent(id))?;
        self.read_indexed_frame(frame)?
            .into_iter()
            .find(|event| event.id == id)
            .ok_or(StorageError::UnknownEvent(id))
    }

    pub fn history(
        &mut self,
        filter: HistoryFilter,
        budget: StorageQueryBudget,
        cursor: Option<StorageCursor>,
    ) -> Result<StoragePage, StorageError> {
        if self.poisoned {
            return Err(StorageError::Poisoned);
        }
        if budget.max_results == 0 || budget.max_scanned == 0 || budget.max_read_bytes == 0 {
            return Err(StorageError::InvalidBudget);
        }
        if let (Some(cutoff), Some(known)) = (filter.known_as_of, self.last_known) {
            if cutoff.clock != known.clock {
                return Err(StorageError::KnownClockMismatch {
                    expected: known.clock,
                    actual: cutoff.clock,
                });
            }
        }
        let (mut next, end) = match cursor {
            Some(cursor)
                if cursor.database == self.header.database
                    && cursor.source == self.header.source
                    && cursor.epoch == self.header.epoch
                    && cursor.filter == filter
                    && cursor.next_sequence <= cursor.snapshot_end
                    && cursor.snapshot_end <= self.next_sequence =>
            {
                (cursor.next_sequence, cursor.snapshot_end)
            }
            Some(_) => return Err(StorageError::InvalidCursor),
            None => (0, self.next_sequence),
        };
        let mut page = StoragePage {
            events: Vec::new(),
            scanned: 0,
            bytes_read: 0,
            continuation: None,
        };
        while next < end {
            let frame = self
                .frame_for_sequence(next)
                .ok_or(StorageError::InvalidCursor)?;
            if frame.length > budget.max_read_bytes || frame.count > budget.max_scanned {
                if page.bytes_read != 0 {
                    break;
                }
                return Err(StorageError::BudgetTooSmall {
                    frame_bytes: frame.length,
                    frame_records: frame.count,
                });
            }
            if frame.length > budget.max_read_bytes - page.bytes_read
                || frame.count > budget.max_scanned - page.scanned
            {
                break;
            }
            let records = self.read_indexed_frame(frame)?;
            page.scanned += records.len();
            page.bytes_read += frame.length;
            for record in records {
                if record.id.sequence < next {
                    continue;
                }
                if record.id.sequence >= end {
                    break;
                }
                next = record.id.sequence + 1;
                if filter.entity.is_some_and(|entity| entity != record.entity)
                    || filter
                        .known_as_of
                        .is_some_and(|cutoff| record.times.known.ticks > cutoff.ticks)
                    || filter.time.is_some_and(|(axis, range)| {
                        !record
                            .times
                            .on(axis)
                            .is_some_and(|time| range.contains(time))
                    })
                {
                    continue;
                }
                page.events
                    .try_reserve(1)
                    .map_err(|_| StorageError::AllocationFailed)?;
                page.events.push(record);
                if page.events.len() == budget.max_results {
                    break;
                }
            }
            if page.events.len() == budget.max_results {
                break;
            }
        }
        if next < end {
            page.continuation = Some(StorageCursor {
                database: self.header.database,
                source: self.header.source,
                epoch: self.header.epoch,
                filter,
                next_sequence: next,
                snapshot_end: end,
            });
        }
        Ok(page)
    }

    /// Exports one independently readable TSF per WAL batch. The WAL is retained.
    pub fn seal(&mut self) -> Result<SealReport, StorageError> {
        if self.poisoned {
            return Err(StorageError::Poisoned);
        }
        let directory = self.root.join("segments");
        fs::create_dir_all(&directory).map_err(io_error("create segment directory"))?;
        #[cfg(unix)]
        sync_directory(&self.root)?;
        let mut report = SealReport::default();
        for index in 0..self.frames.len() {
            let frame = self.frames[index];
            let records = self.read_indexed_frame(frame)?;
            let encoded = encode_segment(self.header, &records, &self.limits)?;
            let last = frame.first + frame.count as u64 - 1;
            let destination = directory.join(format!("{:020}-{:020}.tsf", frame.first, last));
            if destination
                .try_exists()
                .map_err(io_error("inspect segment path"))?
            {
                let existing = read_bounded_file(&destination, self.limits.max_segment_bytes)?;
                if existing != encoded {
                    return Err(StorageError::InvalidRecord(
                        "existing immutable segment differs from WAL",
                    ));
                }
                decode_segment(&existing, &self.limits)?;
                report.segments_existing += 1;
            } else {
                let token = random_id()?;
                let name = token.to_string();
                let temporary = directory.join(format!(".tmp-{name}"));
                let mut file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&temporary)
                    .map_err(io_error("create temporary segment"))?;
                file.write_all(&encoded)
                    .map_err(io_error("write temporary segment"))?;
                file.sync_all()
                    .map_err(io_error("synchronize temporary segment"))?;
                drop(file);
                let persisted = read_bounded_file(&temporary, self.limits.max_segment_bytes)?;
                let decoded = decode_segment(&persisted, &self.limits)?;
                if decoded.header != self.header || decoded.records != records {
                    return Err(StorageError::InvalidRecord(
                        "segment verification differs from WAL",
                    ));
                }
                // Hard-link publication fails if the destination exists; unlike
                // rename on Unix it cannot overwrite immutable evidence.
                fs::hard_link(&temporary, &destination)
                    .map_err(io_error("publish immutable segment"))?;
                #[cfg(unix)]
                sync_directory(&directory)?;
                fs::remove_file(&temporary)
                    .map_err(io_error("remove published segment temporary"))?;
                #[cfg(unix)]
                sync_directory(&directory)?;
                report.segments_created += 1;
            }
            report.events += records.len() as u64;
        }
        Ok(report)
    }
}

fn validate_record(
    header: WalHeader,
    sequence: u64,
    previous: Option<Timestamp>,
    record: &StoredEvent,
) -> Result<(), StorageError> {
    if record.id.source != header.source
        || record.id.epoch != header.epoch
        || record.id.sequence != sequence
    {
        return Err(StorageError::InvalidRecord(
            "source, epoch, or contiguous sequence mismatch",
        ));
    }
    if let Some(previous) = previous {
        if previous.clock != record.times.known.clock {
            return Err(StorageError::KnownClockMismatch {
                expected: previous.clock,
                actual: record.times.known.clock,
            });
        }
        if record.times.known.ticks < previous.ticks {
            return Err(StorageError::KnownTimeRegression {
                previous: previous.ticks,
                actual: record.times.known.ticks,
            });
        }
    }
    if record.causes.iter().any(|cause| {
        cause.source == header.source && cause.epoch == header.epoch && cause.sequence >= sequence
    }) {
        return Err(StorageError::InvalidRecord(
            "same-source cause does not precede event",
        ));
    }
    Ok(())
}

pub fn read_bounded_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, StorageError> {
    let mut file = File::open(path).map_err(io_error("open bounded file"))?;
    let length = file
        .metadata()
        .map_err(io_error("inspect bounded file"))?
        .len();
    if length > max_bytes as u64 {
        return Err(FormatError::LimitExceeded("file bytes").into());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length as usize)
        .map_err(|_| StorageError::AllocationFailed)?;
    bytes.resize(length as usize, 0);
    file.read_exact(&mut bytes)
        .map_err(io_error("read bounded file"))?;
    let mut trailing = [0u8; 1];
    if file
        .read(&mut trailing)
        .map_err(io_error("check bounded file end"))?
        != 0
    {
        return Err(StorageError::InvalidRecord("file grew during bounded read"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use temnion_core::ShardId;

    #[test]
    fn ambiguous_sync_failure_poisoning_requires_recovery_before_id_reuse() {
        let token = random_id().unwrap();
        let name = token.to_string();
        let path = std::env::temp_dir().join(format!("temnion-sync-fault-{name}"));
        fs::create_dir(&path).unwrap();
        let input = WriteEvent {
            entity: EntityId {
                shard: ShardId(0),
                slot: 0,
                generation: 0,
            },
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 0),
                observed: None,
                known: Timestamp::new(ClockId(2), 0),
            },
            schema: SchemaId(1),
            payload: vec![42],
            causes: Vec::new(),
        };
        let mut store =
            Store::create(&path, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();
        store.fail_before_sync = true;
        assert!(matches!(
            store.append(vec![input.clone()]),
            Err(StorageError::CommitOutcomeUnknown(_))
        ));
        assert_eq!(
            store.len(),
            0,
            "unacknowledged writer metadata is not published"
        );
        assert!(matches!(
            store.append(vec![input.clone()]),
            Err(StorageError::Poisoned)
        ));
        drop(store);
        let (mut reopened, report) =
            Store::open(&path, Limits::default(), RecoveryMode::RejectIncompleteTail).unwrap();
        assert_eq!(
            report.recovered_events, 1,
            "a failed acknowledgment can still leave a complete valid record"
        );
        assert_eq!(reopened.append(vec![input]).unwrap().first.sequence, 1);
        drop(reopened);
        fs::remove_dir_all(&path).unwrap();
    }
}
