// SPDX-License-Identifier: AGPL-3.0-only
//! Explicit little-endian WAL and TSF codecs with bounded allocation.

use std::error::Error;
use std::fmt;

use temnion_core::{
    ClockId, DatabaseId, EntityId, EventId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId,
    Timestamp,
};

const WAL_MAGIC: [u8; 4] = *b"TNWL";
const BATCH_MAGIC: [u8; 4] = *b"TNWB";
const SEGMENT_MAGIC: [u8; 4] = *b"TNSF";
const SEGMENT_FOOTER_MAGIC: [u8; 4] = *b"TNFT";

const FORMAT_VERSION: u16 = 1;
const NO_FLAGS: u16 = 0;
const OBSERVED_FLAG: u16 = 1;
const KNOWN_RECORD_FLAGS: u16 = OBSERVED_FLAG;

const EVENT_ID_LEN: usize = 4 + 8 + 8;
const TIMESTAMP_LEN: usize = 4 + 8;
const OBSERVED_TIMESTAMP_LEN: usize = TIMESTAMP_LEN;
const RECORD_FIXED_LEN: usize = EVENT_ID_LEN + 12 + TIMESTAMP_LEN + TIMESTAMP_LEN + 4 + 2 + 2 + 4;

const WAL_CHECKSUM_OFFSET: usize = 36;
const BATCH_BODY_CHECKSUM_OFFSET: usize = 16;
const BATCH_HEADER_CHECKSUM_OFFSET: usize = 20;
const SEGMENT_BODY_LEN_OFFSET: usize = 36;
const SEGMENT_RECORD_COUNT_OFFSET: usize = 40;
const SEGMENT_BODY_CHECKSUM_OFFSET: usize = 44;
const SEGMENT_HEADER_CHECKSUM_OFFSET: usize = 48;
const SEGMENT_HEADER_LEN: usize = 52;
const SEGMENT_FOOTER_DIR_LEN_OFFSET: usize = 8;
const SEGMENT_FOOTER_CHECKSUM_OFFSET: usize = 12;
const SEGMENT_FOOTER_LEN: usize = 16;

pub const WAL_HEADER_LEN: usize = 40;
pub const BATCH_HEADER_LEN: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredEvent {
    pub id: EventId,
    pub entity: EntityId,
    pub times: EventTimes,
    pub schema: SchemaId,
    pub payload: Vec<u8>,
    pub causes: Vec<EventId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalHeader {
    pub database: DatabaseId,
    pub source: SourceId,
    pub epoch: SourceEpoch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_frame_bytes: usize,
    pub max_records: usize,
    pub max_payload_bytes: usize,
    pub max_causes: usize,
    pub max_segment_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_frame_bytes: 4 * 1024 * 1024,
            max_records: 65_536,
            max_payload_bytes: 1024 * 1024,
            max_causes: 64,
            max_segment_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub header: WalHeader,
    pub records: Vec<StoredEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatError {
    Incomplete { needed: usize, available: usize },
    InvalidMagic,
    UnsupportedVersion(u16),
    ChecksumMismatch,
    LimitExceeded(&'static str),
    InvalidData(&'static str),
    AllocationFailed,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete { needed, available } => {
                write!(f, "incomplete input: need {needed} bytes, have {available}")
            }
            Self::InvalidMagic => write!(f, "invalid magic"),
            Self::UnsupportedVersion(version) => write!(f, "unsupported format version {version}"),
            Self::ChecksumMismatch => write!(f, "checksum mismatch"),
            Self::LimitExceeded(limit) => write!(f, "limit exceeded: {limit}"),
            Self::InvalidData(reason) => write!(f, "invalid data: {reason}"),
            Self::AllocationFailed => write!(f, "bounded allocation failed"),
        }
    }
}

impl Error for FormatError {}

pub fn encode_wal_header(header: WalHeader) -> Vec<u8> {
    let mut bytes = vec![0u8; WAL_HEADER_LEN];
    bytes[..4].copy_from_slice(&WAL_MAGIC);
    put_u16(&mut bytes, 4, FORMAT_VERSION);
    put_u16(&mut bytes, 6, NO_FLAGS);
    bytes[8..24].copy_from_slice(&header.database.0);
    put_u32(&mut bytes, 24, header.source.0);
    put_u64(&mut bytes, 28, header.epoch.0);
    let checksum = crc32fast::hash(&bytes[..WAL_CHECKSUM_OFFSET]);
    put_u32(&mut bytes, WAL_CHECKSUM_OFFSET, checksum);
    bytes
}

pub fn decode_wal_header(bytes: &[u8]) -> Result<WalHeader, FormatError> {
    match bytes.len().cmp(&WAL_HEADER_LEN) {
        std::cmp::Ordering::Less => {
            return Err(FormatError::Incomplete {
                needed: WAL_HEADER_LEN,
                available: bytes.len(),
            });
        }
        std::cmp::Ordering::Greater => {
            return Err(FormatError::InvalidData("trailing WAL header bytes"));
        }
        std::cmp::Ordering::Equal => {}
    }
    if bytes[..4] != WAL_MAGIC {
        return Err(FormatError::InvalidMagic);
    }
    let version = get_u16(bytes, 4);
    if version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    if get_u16(bytes, 6) != NO_FLAGS {
        return Err(FormatError::InvalidData("unsupported WAL header flags"));
    }
    let expected = crc32fast::hash(&bytes[..WAL_CHECKSUM_OFFSET]);
    if get_u32(bytes, WAL_CHECKSUM_OFFSET) != expected {
        return Err(FormatError::ChecksumMismatch);
    }
    let mut database = [0u8; 16];
    database.copy_from_slice(&bytes[8..24]);
    Ok(WalHeader {
        database: DatabaseId(database),
        source: SourceId(get_u32(bytes, 24)),
        epoch: SourceEpoch(get_u64(bytes, 28)),
    })
}

pub fn encode_batch(records: &[StoredEvent], limits: &Limits) -> Result<Vec<u8>, FormatError> {
    if records.len() > limits.max_records {
        return Err(FormatError::LimitExceeded("records per batch"));
    }
    let mut body_len = 0usize;
    for record in records {
        body_len = body_len
            .checked_add(encoded_record_len(record, limits)?)
            .ok_or(FormatError::InvalidData("frame length overflow"))?;
    }
    let frame_len = BATCH_HEADER_LEN
        .checked_add(body_len)
        .ok_or(FormatError::InvalidData("frame length overflow"))?;
    if frame_len > limits.max_frame_bytes {
        return Err(FormatError::LimitExceeded("frame bytes"));
    }
    let frame_len_u32 =
        u32::try_from(frame_len).map_err(|_| FormatError::LimitExceeded("frame bytes"))?;
    let record_count = u32::try_from(records.len())
        .map_err(|_| FormatError::LimitExceeded("records per batch"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(frame_len)
        .map_err(|_| FormatError::AllocationFailed)?;
    bytes.resize(BATCH_HEADER_LEN, 0);
    for record in records {
        encode_record_body(&mut bytes, record)?;
    }
    let body_checksum = crc32fast::hash(&bytes[BATCH_HEADER_LEN..]);
    bytes[..4].copy_from_slice(&BATCH_MAGIC);
    put_u16(&mut bytes, 4, FORMAT_VERSION);
    put_u16(&mut bytes, 6, NO_FLAGS);
    put_u32(&mut bytes, 8, frame_len_u32);
    put_u32(&mut bytes, 12, record_count);
    put_u32(&mut bytes, BATCH_BODY_CHECKSUM_OFFSET, body_checksum);
    let header_checksum = crc32fast::hash(&bytes[..BATCH_HEADER_CHECKSUM_OFFSET]);
    put_u32(&mut bytes, BATCH_HEADER_CHECKSUM_OFFSET, header_checksum);
    Ok(bytes)
}

pub fn frame_length(prefix: &[u8], limits: &Limits) -> Result<usize, FormatError> {
    if prefix.len() < BATCH_HEADER_LEN {
        return Err(FormatError::Incomplete {
            needed: BATCH_HEADER_LEN,
            available: prefix.len(),
        });
    }
    if prefix[..4] != BATCH_MAGIC {
        return Err(FormatError::InvalidMagic);
    }
    let version = get_u16(prefix, 4);
    if version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    if get_u16(prefix, 6) != NO_FLAGS {
        return Err(FormatError::InvalidData("unsupported batch flags"));
    }
    let expected = crc32fast::hash(&prefix[..BATCH_HEADER_CHECKSUM_OFFSET]);
    if get_u32(prefix, BATCH_HEADER_CHECKSUM_OFFSET) != expected {
        return Err(FormatError::ChecksumMismatch);
    }
    let frame_len = usize::try_from(get_u32(prefix, 8))
        .map_err(|_| FormatError::LimitExceeded("frame bytes"))?;
    if frame_len < BATCH_HEADER_LEN {
        return Err(FormatError::InvalidData("frame length smaller than header"));
    }
    if frame_len > limits.max_frame_bytes {
        return Err(FormatError::LimitExceeded("frame bytes"));
    }
    let record_count = usize::try_from(get_u32(prefix, 12))
        .map_err(|_| FormatError::LimitExceeded("records per batch"))?;
    if record_count > limits.max_records {
        return Err(FormatError::LimitExceeded("records per batch"));
    }
    Ok(frame_len)
}

pub fn decode_batch(frame: &[u8], limits: &Limits) -> Result<Vec<StoredEvent>, FormatError> {
    let frame_len = frame_length(frame, limits)?;
    if frame.len() < frame_len {
        return Err(FormatError::Incomplete {
            needed: frame_len,
            available: frame.len(),
        });
    }
    if frame.len() > frame_len {
        return Err(FormatError::InvalidData("trailing batch bytes"));
    }
    let expected = get_u32(frame, BATCH_BODY_CHECKSUM_OFFSET);
    let actual = crc32fast::hash(&frame[BATCH_HEADER_LEN..]);
    if expected != actual {
        return Err(FormatError::ChecksumMismatch);
    }
    let record_count = usize::try_from(get_u32(frame, 12))
        .map_err(|_| FormatError::LimitExceeded("records per batch"))?;
    let body = &frame[BATCH_HEADER_LEN..frame_len];
    let mut cursor = 0usize;
    let mut records = Vec::new();
    records
        .try_reserve_exact(record_count)
        .map_err(|_| FormatError::AllocationFailed)?;
    for _ in 0..record_count {
        records.push(decode_record_body(body, &mut cursor, limits)?);
    }
    if cursor != body.len() {
        return Err(FormatError::InvalidData("trailing record bytes"));
    }
    Ok(records)
}

pub fn encode_segment(
    header: WalHeader,
    records: &[StoredEvent],
    limits: &Limits,
) -> Result<Vec<u8>, FormatError> {
    for record in records {
        if record.id.source != header.source || record.id.epoch != header.epoch {
            return Err(FormatError::InvalidData(
                "record source or epoch differs from segment header",
            ));
        }
    }
    let batch = encode_batch(records, limits)?;
    let batch_len_u32 =
        u32::try_from(batch.len()).map_err(|_| FormatError::LimitExceeded("frame bytes"))?;
    let record_count = u32::try_from(records.len())
        .map_err(|_| FormatError::LimitExceeded("records per batch"))?;
    let total_len = SEGMENT_HEADER_LEN
        .checked_add(batch.len())
        .and_then(|len| len.checked_add(SEGMENT_FOOTER_LEN))
        .ok_or(FormatError::InvalidData("segment length overflow"))?;
    if total_len > limits.max_segment_bytes {
        return Err(FormatError::LimitExceeded("segment bytes"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(total_len)
        .map_err(|_| FormatError::AllocationFailed)?;
    bytes.resize(SEGMENT_HEADER_LEN, 0);
    bytes.extend_from_slice(&batch);
    bytes.resize(total_len, 0);

    bytes[..4].copy_from_slice(&SEGMENT_MAGIC);
    put_u16(&mut bytes, 4, FORMAT_VERSION);
    put_u16(&mut bytes, 6, NO_FLAGS);
    bytes[8..24].copy_from_slice(&header.database.0);
    put_u32(&mut bytes, 24, header.source.0);
    put_u64(&mut bytes, 28, header.epoch.0);
    put_u32(&mut bytes, SEGMENT_BODY_LEN_OFFSET, batch_len_u32);
    put_u32(&mut bytes, SEGMENT_RECORD_COUNT_OFFSET, record_count);
    let body_checksum =
        crc32fast::hash(&bytes[SEGMENT_HEADER_LEN..SEGMENT_HEADER_LEN + batch.len()]);
    put_u32(&mut bytes, SEGMENT_BODY_CHECKSUM_OFFSET, body_checksum);
    let header_checksum = crc32fast::hash(&bytes[..SEGMENT_HEADER_CHECKSUM_OFFSET]);
    put_u32(&mut bytes, SEGMENT_HEADER_CHECKSUM_OFFSET, header_checksum);

    let footer = total_len - SEGMENT_FOOTER_LEN;
    bytes[footer..footer + 4].copy_from_slice(&SEGMENT_FOOTER_MAGIC);
    put_u16(&mut bytes, footer + 4, FORMAT_VERSION);
    put_u16(&mut bytes, footer + 6, NO_FLAGS);
    put_u32(&mut bytes, footer + SEGMENT_FOOTER_DIR_LEN_OFFSET, 0);
    let footer_checksum = crc32fast::hash(&bytes[footer..footer + SEGMENT_FOOTER_CHECKSUM_OFFSET]);
    put_u32(
        &mut bytes,
        footer + SEGMENT_FOOTER_CHECKSUM_OFFSET,
        footer_checksum,
    );
    Ok(bytes)
}

pub fn decode_segment(bytes: &[u8], limits: &Limits) -> Result<Segment, FormatError> {
    if bytes.len() < SEGMENT_HEADER_LEN {
        return Err(FormatError::Incomplete {
            needed: SEGMENT_HEADER_LEN,
            available: bytes.len(),
        });
    }
    if bytes[..4] != SEGMENT_MAGIC {
        return Err(FormatError::InvalidMagic);
    }
    let version = get_u16(bytes, 4);
    if version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    if get_u16(bytes, 6) != NO_FLAGS {
        return Err(FormatError::InvalidData("unsupported segment flags"));
    }
    let expected = crc32fast::hash(&bytes[..SEGMENT_HEADER_CHECKSUM_OFFSET]);
    if get_u32(bytes, SEGMENT_HEADER_CHECKSUM_OFFSET) != expected {
        return Err(FormatError::ChecksumMismatch);
    }
    let body_len = usize::try_from(get_u32(bytes, SEGMENT_BODY_LEN_OFFSET))
        .map_err(|_| FormatError::LimitExceeded("segment bytes"))?;
    let total_len = SEGMENT_HEADER_LEN
        .checked_add(body_len)
        .and_then(|len| len.checked_add(SEGMENT_FOOTER_LEN))
        .ok_or(FormatError::InvalidData("segment length overflow"))?;
    if total_len > limits.max_segment_bytes {
        return Err(FormatError::LimitExceeded("segment bytes"));
    }
    if bytes.len() < total_len {
        return Err(FormatError::Incomplete {
            needed: total_len,
            available: bytes.len(),
        });
    }
    if bytes.len() > total_len {
        return Err(FormatError::InvalidData("trailing segment bytes"));
    }
    let footer_start = SEGMENT_HEADER_LEN + body_len;
    if bytes[footer_start..footer_start + 4] != SEGMENT_FOOTER_MAGIC {
        return Err(FormatError::InvalidMagic);
    }
    let footer_version = get_u16(bytes, footer_start + 4);
    if footer_version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion(footer_version));
    }
    if get_u16(bytes, footer_start + 6) != NO_FLAGS {
        return Err(FormatError::InvalidData("unsupported segment footer flags"));
    }
    if get_u32(bytes, footer_start + SEGMENT_FOOTER_DIR_LEN_OFFSET) != 0 {
        return Err(FormatError::InvalidData("nonzero segment directory length"));
    }
    let footer_checksum =
        crc32fast::hash(&bytes[footer_start..footer_start + SEGMENT_FOOTER_CHECKSUM_OFFSET]);
    if get_u32(bytes, footer_start + SEGMENT_FOOTER_CHECKSUM_OFFSET) != footer_checksum {
        return Err(FormatError::ChecksumMismatch);
    }
    let body = &bytes[SEGMENT_HEADER_LEN..footer_start];
    let expected_body = get_u32(bytes, SEGMENT_BODY_CHECKSUM_OFFSET);
    if crc32fast::hash(body) != expected_body {
        return Err(FormatError::ChecksumMismatch);
    }
    if body_len < BATCH_HEADER_LEN {
        return Err(FormatError::InvalidData(
            "segment body shorter than embedded batch header",
        ));
    }
    let mut database = [0u8; 16];
    database.copy_from_slice(&bytes[8..24]);
    let header = WalHeader {
        database: DatabaseId(database),
        source: SourceId(get_u32(bytes, 24)),
        epoch: SourceEpoch(get_u64(bytes, 28)),
    };
    let expected_count = usize::try_from(get_u32(bytes, SEGMENT_RECORD_COUNT_OFFSET))
        .map_err(|_| FormatError::LimitExceeded("records per batch"))?;
    if expected_count > limits.max_records {
        return Err(FormatError::LimitExceeded("records per batch"));
    }
    let records = match decode_batch(body, limits) {
        Err(FormatError::Incomplete { .. }) => {
            return Err(FormatError::InvalidData(
                "complete segment contains a truncated embedded batch",
            ));
        }
        other => other,
    }?;
    if records.len() != expected_count {
        return Err(FormatError::InvalidData(
            "segment record count differs from embedded batch",
        ));
    }
    if records
        .iter()
        .any(|record| record.id.source != header.source || record.id.epoch != header.epoch)
    {
        return Err(FormatError::InvalidData(
            "record source or epoch differs from segment header",
        ));
    }
    Ok(Segment { header, records })
}

fn encoded_record_len(record: &StoredEvent, limits: &Limits) -> Result<usize, FormatError> {
    if record.payload.len() > limits.max_payload_bytes {
        return Err(FormatError::LimitExceeded("payload bytes"));
    }
    if record.payload.len() > u32::MAX as usize {
        return Err(FormatError::LimitExceeded("payload bytes"));
    }
    if record.causes.len() > limits.max_causes {
        return Err(FormatError::LimitExceeded("causes per event"));
    }
    if record.causes.len() > u16::MAX as usize {
        return Err(FormatError::LimitExceeded("causes per event"));
    }
    let observed_len = if record.times.observed.is_some() {
        OBSERVED_TIMESTAMP_LEN
    } else {
        0
    };
    let causes_len = record
        .causes
        .len()
        .checked_mul(EVENT_ID_LEN)
        .ok_or(FormatError::InvalidData("record length overflow"))?;
    RECORD_FIXED_LEN
        .checked_add(observed_len)
        .and_then(|len| len.checked_add(causes_len))
        .and_then(|len| len.checked_add(record.payload.len()))
        .ok_or(FormatError::InvalidData("record length overflow"))
}

fn encode_record_body(bytes: &mut Vec<u8>, record: &StoredEvent) -> Result<(), FormatError> {
    let payload_len = u32::try_from(record.payload.len())
        .map_err(|_| FormatError::LimitExceeded("payload bytes"))?;
    let cause_count = u16::try_from(record.causes.len())
        .map_err(|_| FormatError::LimitExceeded("causes per event"))?;

    put_u32_push(bytes, record.id.source.0);
    put_u64_push(bytes, record.id.epoch.0);
    put_u64_push(bytes, record.id.sequence);
    put_u32_push(bytes, record.entity.shard.0);
    put_u32_push(bytes, record.entity.slot);
    put_u32_push(bytes, record.entity.generation);
    put_u32_push(bytes, record.times.valid.clock.0);
    put_u64_push(bytes, record.times.valid.ticks);
    put_u32_push(bytes, record.times.known.clock.0);
    put_u64_push(bytes, record.times.known.ticks);
    put_u32_push(bytes, record.schema.0);

    let flags = if record.times.observed.is_some() {
        OBSERVED_FLAG
    } else {
        NO_FLAGS
    };
    put_u16_push(bytes, flags);
    put_u16_push(bytes, cause_count);
    put_u32_push(bytes, payload_len);

    if let Some(observed) = record.times.observed {
        put_u32_push(bytes, observed.clock.0);
        put_u64_push(bytes, observed.ticks);
    }
    for cause in &record.causes {
        put_u32_push(bytes, cause.source.0);
        put_u64_push(bytes, cause.epoch.0);
        put_u64_push(bytes, cause.sequence);
    }
    bytes.extend_from_slice(&record.payload);
    Ok(())
}

fn decode_record_body(
    body: &[u8],
    cursor: &mut usize,
    limits: &Limits,
) -> Result<StoredEvent, FormatError> {
    let id = EventId {
        source: SourceId(read_u32(body, cursor, "truncated event source")?),
        epoch: SourceEpoch(read_u64(body, cursor, "truncated event epoch")?),
        sequence: read_u64(body, cursor, "truncated event sequence")?,
    };
    let entity = EntityId {
        shard: ShardId(read_u32(body, cursor, "truncated entity shard")?),
        slot: read_u32(body, cursor, "truncated entity slot")?,
        generation: read_u32(body, cursor, "truncated entity generation")?,
    };
    let valid = Timestamp {
        clock: ClockId(read_u32(body, cursor, "truncated valid clock")?),
        ticks: read_u64(body, cursor, "truncated valid ticks")?,
    };
    let known = Timestamp {
        clock: ClockId(read_u32(body, cursor, "truncated known clock")?),
        ticks: read_u64(body, cursor, "truncated known ticks")?,
    };
    let schema = SchemaId(read_u32(body, cursor, "truncated schema id")?);
    let flags = read_u16(body, cursor, "truncated record flags")?;
    if flags & !KNOWN_RECORD_FLAGS != 0 {
        return Err(FormatError::InvalidData("unsupported record flags"));
    }
    let cause_count = usize::from(read_u16(body, cursor, "truncated cause count")?);
    if cause_count > limits.max_causes {
        return Err(FormatError::LimitExceeded("causes per event"));
    }
    let payload_len = usize::try_from(read_u32(body, cursor, "truncated payload length")?)
        .map_err(|_| FormatError::LimitExceeded("payload bytes"))?;
    if payload_len > limits.max_payload_bytes {
        return Err(FormatError::LimitExceeded("payload bytes"));
    }
    let observed = if flags & OBSERVED_FLAG != 0 {
        Some(Timestamp {
            clock: ClockId(read_u32(body, cursor, "truncated observed clock")?),
            ticks: read_u64(body, cursor, "truncated observed ticks")?,
        })
    } else {
        None
    };

    let mut causes = Vec::new();
    causes
        .try_reserve_exact(cause_count)
        .map_err(|_| FormatError::AllocationFailed)?;
    for _ in 0..cause_count {
        causes.push(EventId {
            source: SourceId(read_u32(body, cursor, "truncated cause source")?),
            epoch: SourceEpoch(read_u64(body, cursor, "truncated cause epoch")?),
            sequence: read_u64(body, cursor, "truncated cause sequence")?,
        });
    }

    let end = cursor
        .checked_add(payload_len)
        .ok_or(FormatError::InvalidData("payload length overflow"))?;
    let payload_slice = body
        .get(*cursor..end)
        .ok_or(FormatError::InvalidData("truncated payload bytes"))?;
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(payload_len)
        .map_err(|_| FormatError::AllocationFailed)?;
    payload.extend_from_slice(payload_slice);
    *cursor = end;

    Ok(StoredEvent {
        id,
        entity,
        times: EventTimes {
            valid,
            observed,
            known,
        },
        schema,
        payload,
        causes,
    })
}

fn read_u16(bytes: &[u8], cursor: &mut usize, error: &'static str) -> Result<u16, FormatError> {
    let value = read_fixed::<2>(bytes, cursor, error)?;
    Ok(u16::from_le_bytes(value))
}

fn read_u32(bytes: &[u8], cursor: &mut usize, error: &'static str) -> Result<u32, FormatError> {
    let value = read_fixed::<4>(bytes, cursor, error)?;
    Ok(u32::from_le_bytes(value))
}

fn read_u64(bytes: &[u8], cursor: &mut usize, error: &'static str) -> Result<u64, FormatError> {
    let value = read_fixed::<8>(bytes, cursor, error)?;
    Ok(u64::from_le_bytes(value))
}

fn read_fixed<const N: usize>(
    bytes: &[u8],
    cursor: &mut usize,
    error: &'static str,
) -> Result<[u8; N], FormatError> {
    let end = cursor
        .checked_add(N)
        .ok_or(FormatError::InvalidData("cursor overflow"))?;
    let slice = bytes
        .get(*cursor..end)
        .ok_or(FormatError::InvalidData(error))?;
    let mut value = [0u8; N];
    value.copy_from_slice(slice);
    *cursor = end;
    Ok(value)
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u16(bytes: &[u8], offset: usize) -> u16 {
    let mut raw = [0u8; 2];
    raw.copy_from_slice(&bytes[offset..offset + 2]);
    u16::from_le_bytes(raw)
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    let mut raw = [0u8; 4];
    raw.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(raw)
}

fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(raw)
}

fn put_u16_push(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn put_u32_push(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn put_u64_push(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_header() -> WalHeader {
        WalHeader {
            database: DatabaseId([
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ]),
            source: SourceId(0x1122_3344),
            epoch: SourceEpoch(0x0102_0304_0506_0708),
        }
    }

    fn sample_record(
        sequence: u64,
        observed: Option<Timestamp>,
        payload: &[u8],
        causes: Vec<EventId>,
    ) -> StoredEvent {
        StoredEvent {
            id: EventId {
                source: sample_header().source,
                epoch: sample_header().epoch,
                sequence,
            },
            entity: EntityId {
                shard: ShardId(7),
                slot: 11 + sequence as u32,
                generation: 13,
            },
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 100 + sequence),
                observed,
                known: Timestamp::new(ClockId(2), 200 + sequence),
            },
            schema: SchemaId(0x5566_7788),
            payload: payload.to_vec(),
            causes,
        }
    }

    fn observed_time() -> Timestamp {
        Timestamp::new(ClockId(9), 555)
    }

    fn multi_source_causes() -> Vec<EventId> {
        vec![
            EventId {
                source: SourceId(77),
                epoch: SourceEpoch(9),
                sequence: 3,
            },
            EventId {
                source: sample_header().source,
                epoch: sample_header().epoch,
                sequence: 0,
            },
        ]
    }

    fn recompute_batch_header(frame: &mut [u8]) {
        let body_checksum = crc32fast::hash(&frame[BATCH_HEADER_LEN..]);
        put_u32(frame, BATCH_BODY_CHECKSUM_OFFSET, body_checksum);
        let header_checksum = crc32fast::hash(&frame[..BATCH_HEADER_CHECKSUM_OFFSET]);
        put_u32(frame, BATCH_HEADER_CHECKSUM_OFFSET, header_checksum);
    }

    fn recompute_segment_checksums(segment: &mut [u8]) {
        let body_len = get_u32(segment, SEGMENT_BODY_LEN_OFFSET) as usize;
        let footer = SEGMENT_HEADER_LEN + body_len;
        let body_checksum = crc32fast::hash(&segment[SEGMENT_HEADER_LEN..footer]);
        put_u32(segment, SEGMENT_BODY_CHECKSUM_OFFSET, body_checksum);
        let header_checksum = crc32fast::hash(&segment[..SEGMENT_HEADER_CHECKSUM_OFFSET]);
        put_u32(segment, SEGMENT_HEADER_CHECKSUM_OFFSET, header_checksum);
        let footer_checksum =
            crc32fast::hash(&segment[footer..footer + SEGMENT_FOOTER_CHECKSUM_OFFSET]);
        put_u32(
            segment,
            footer + SEGMENT_FOOTER_CHECKSUM_OFFSET,
            footer_checksum,
        );
    }

    #[test]
    fn default_limits_match_the_contract() {
        assert_eq!(
            Limits::default(),
            Limits {
                max_frame_bytes: 4 * 1024 * 1024,
                max_records: 65_536,
                max_payload_bytes: 1024 * 1024,
                max_causes: 64,
                max_segment_bytes: 64 * 1024 * 1024,
            }
        );
    }

    #[test]
    fn wal_header_roundtrips_and_matches_the_golden_fixture() {
        let encoded = encode_wal_header(sample_header());
        let expected = [
            84, 78, 87, 76, 1, 0, 0, 0, 0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204,
            221, 238, 255, 68, 51, 34, 17, 8, 7, 6, 5, 4, 3, 2, 1, 177, 249, 120, 187,
        ];
        assert_eq!(encoded, expected);
        assert_eq!(decode_wal_header(&encoded).unwrap(), sample_header());
    }

    #[test]
    fn wal_header_rejects_exhaustive_truncation_bit_flips_and_trailing_bytes() {
        let encoded = encode_wal_header(sample_header());
        for cut in 0..WAL_HEADER_LEN {
            assert_eq!(
                decode_wal_header(&encoded[..cut]),
                Err(FormatError::Incomplete {
                    needed: WAL_HEADER_LEN,
                    available: cut,
                }),
                "cut={cut}"
            );
        }
        let mut wrong_magic = encoded.clone();
        wrong_magic[0] ^= 0x01;
        assert_eq!(
            decode_wal_header(&wrong_magic),
            Err(FormatError::InvalidMagic)
        );
        let mut flipped = encoded.clone();
        flipped[10] ^= 0x01;
        assert_eq!(
            decode_wal_header(&flipped),
            Err(FormatError::ChecksumMismatch)
        );
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_wal_header(&trailing),
            Err(FormatError::InvalidData("trailing WAL header bytes"))
        );
    }

    #[test]
    fn batch_roundtrips_with_observed_and_multi_source_causes() {
        let first = sample_record(0, Some(observed_time()), &[1, 2, 3], multi_source_causes());
        let second = sample_record(1, None, &[4, 5], Vec::new());
        let encoded = encode_batch(&[first.clone(), second.clone()], &Limits::default()).unwrap();
        assert_eq!(
            frame_length(&encoded[..BATCH_HEADER_LEN], &Limits::default()).unwrap(),
            encoded.len()
        );
        assert_eq!(
            decode_batch(&encoded, &Limits::default()).unwrap(),
            vec![first, second]
        );
    }

    #[test]
    fn batch_matches_the_golden_fixture_and_version_bytes() {
        let record = sample_record(
            0,
            Some(observed_time()),
            &[0xaa, 0xbb],
            multi_source_causes(),
        );
        let encoded = encode_batch(&[record], &Limits::default()).unwrap();
        let expected = [
            84, 78, 87, 66, 1, 0, 0, 0, 146, 0, 0, 0, 1, 0, 0, 0, 232, 2, 201, 205, 73, 176, 141,
            188, 68, 51, 34, 17, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 11, 0,
            0, 0, 13, 0, 0, 0, 1, 0, 0, 0, 100, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 200, 0, 0, 0, 0,
            0, 0, 0, 136, 119, 102, 85, 1, 0, 2, 0, 2, 0, 0, 0, 9, 0, 0, 0, 43, 2, 0, 0, 0, 0, 0,
            0, 77, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 68, 51, 34, 17, 8, 7,
            6, 5, 4, 3, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 170, 187,
        ];
        assert_eq!(encoded, expected);
        assert_eq!(get_u16(&encoded, 4), FORMAT_VERSION);
    }

    #[test]
    fn frame_length_validates_header_before_trusting_length_and_count() {
        let record = sample_record(0, None, &[1], Vec::new());
        let encoded = encode_batch(&[record], &Limits::default()).unwrap();

        let mut bad_version = encoded[..BATCH_HEADER_LEN].to_vec();
        put_u16(&mut bad_version, 4, 9);
        recompute_batch_header(&mut bad_version);
        assert_eq!(
            frame_length(&bad_version, &Limits::default()),
            Err(FormatError::UnsupportedVersion(9))
        );

        let mut bad_flags = encoded[..BATCH_HEADER_LEN].to_vec();
        put_u16(&mut bad_flags, 6, 0x80);
        recompute_batch_header(&mut bad_flags);
        assert_eq!(
            frame_length(&bad_flags, &Limits::default()),
            Err(FormatError::InvalidData("unsupported batch flags"))
        );

        let mut too_many = encoded[..BATCH_HEADER_LEN].to_vec();
        put_u32(&mut too_many, 12, 2);
        recompute_batch_header(&mut too_many);
        assert_eq!(
            frame_length(
                &too_many,
                &Limits {
                    max_records: 1,
                    ..Limits::default()
                }
            ),
            Err(FormatError::LimitExceeded("records per batch"))
        );
    }

    #[test]
    fn batch_rejects_exhaustive_truncation_trailing_bytes_and_bit_flips() {
        let record = sample_record(
            0,
            Some(observed_time()),
            &[1, 2, 3, 4],
            multi_source_causes(),
        );
        let encoded = encode_batch(&[record], &Limits::default()).unwrap();
        for cut in 0..encoded.len() {
            let expected = if cut < BATCH_HEADER_LEN {
                Err(FormatError::Incomplete {
                    needed: BATCH_HEADER_LEN,
                    available: cut,
                })
            } else {
                Err(FormatError::Incomplete {
                    needed: encoded.len(),
                    available: cut,
                })
            };
            assert_eq!(
                decode_batch(&encoded[..cut], &Limits::default()),
                expected,
                "cut={cut}"
            );
        }

        let mut wrong_magic = encoded.clone();
        wrong_magic[0] ^= 0x01;
        assert_eq!(
            decode_batch(&wrong_magic, &Limits::default()),
            Err(FormatError::InvalidMagic)
        );

        let mut header_flip = encoded.clone();
        header_flip[8] ^= 0x01;
        assert_eq!(
            decode_batch(&header_flip, &Limits::default()),
            Err(FormatError::ChecksumMismatch)
        );

        let mut payload_flip = encoded.clone();
        *payload_flip.last_mut().unwrap() ^= 0x80;
        assert_eq!(
            decode_batch(&payload_flip, &Limits::default()),
            Err(FormatError::ChecksumMismatch)
        );

        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_batch(&trailing, &Limits::default()),
            Err(FormatError::InvalidData("trailing batch bytes"))
        );
    }

    #[test]
    fn batch_rejects_noncanonical_lengths_and_complete_internal_truncation() {
        let record = sample_record(0, None, &[1, 2], Vec::new());
        let encoded = encode_batch(&[record], &Limits::default()).unwrap();

        let mut shorter = encoded.clone();
        put_u32(&mut shorter, 8, (encoded.len() as u32) - 1);
        recompute_batch_header(&mut shorter);
        assert_eq!(
            decode_batch(&shorter, &Limits::default()),
            Err(FormatError::InvalidData("trailing batch bytes"))
        );

        let mut wrong_count = encoded.clone();
        put_u32(&mut wrong_count, 12, 2);
        recompute_batch_header(&mut wrong_count);
        assert_eq!(
            decode_batch(&wrong_count, &Limits::default()),
            Err(FormatError::InvalidData("truncated event source"))
        );

        let mut truncated_payload = encoded.clone();
        let payload_len_offset = BATCH_HEADER_LEN + 64;
        put_u32(&mut truncated_payload, payload_len_offset, 9);
        recompute_batch_header(&mut truncated_payload);
        assert_eq!(
            decode_batch(&truncated_payload, &Limits::default()),
            Err(FormatError::InvalidData("truncated payload bytes"))
        );

        let with_cause = sample_record(
            0,
            None,
            &[7],
            vec![EventId {
                source: SourceId(1),
                epoch: SourceEpoch(2),
                sequence: 3,
            }],
        );
        let mut truncated_cause = encode_batch(&[with_cause], &Limits::default()).unwrap();
        put_u16(&mut truncated_cause, BATCH_HEADER_LEN + 62, 2);
        recompute_batch_header(&mut truncated_cause);
        assert_eq!(
            decode_batch(&truncated_cause, &Limits::default()),
            Err(FormatError::InvalidData("truncated cause source"))
        );

        let observed = sample_record(0, Some(observed_time()), &[1], Vec::new());
        let mut truncated_observed = encode_batch(&[observed], &Limits::default()).unwrap();
        let observed_offset = BATCH_HEADER_LEN + 68;
        put_u16(
            &mut truncated_observed,
            BATCH_HEADER_LEN + 60,
            OBSERVED_FLAG,
        );
        truncated_observed.truncate(observed_offset + 4);
        let truncated_len = truncated_observed.len() as u32;
        put_u32(&mut truncated_observed, 8, truncated_len);
        let body_checksum = crc32fast::hash(&truncated_observed[BATCH_HEADER_LEN..]);
        put_u32(
            &mut truncated_observed,
            BATCH_BODY_CHECKSUM_OFFSET,
            body_checksum,
        );
        let header_checksum = crc32fast::hash(&truncated_observed[..BATCH_HEADER_CHECKSUM_OFFSET]);
        put_u32(
            &mut truncated_observed,
            BATCH_HEADER_CHECKSUM_OFFSET,
            header_checksum,
        );
        assert_eq!(
            decode_batch(&truncated_observed, &Limits::default()),
            Err(FormatError::InvalidData("truncated observed ticks"))
        );
    }

    #[test]
    fn batch_enforces_payload_cause_frame_and_record_limits() {
        let record = sample_record(0, None, &[1, 2, 3], Vec::new());
        assert_eq!(
            encode_batch(
                std::slice::from_ref(&record),
                &Limits {
                    max_payload_bytes: 2,
                    ..Limits::default()
                }
            ),
            Err(FormatError::LimitExceeded("payload bytes"))
        );

        let many_causes = sample_record(
            0,
            None,
            &[1],
            vec![
                EventId {
                    source: SourceId(1),
                    epoch: SourceEpoch(1),
                    sequence: 1,
                };
                2
            ],
        );
        assert_eq!(
            encode_batch(
                &[many_causes],
                &Limits {
                    max_causes: 1,
                    ..Limits::default()
                }
            ),
            Err(FormatError::LimitExceeded("causes per event"))
        );

        let encoded = encode_batch(&[record], &Limits::default()).unwrap();
        assert_eq!(
            frame_length(
                &encoded[..BATCH_HEADER_LEN],
                &Limits {
                    max_frame_bytes: encoded.len() - 1,
                    ..Limits::default()
                }
            ),
            Err(FormatError::LimitExceeded("frame bytes"))
        );
    }

    #[test]
    fn segment_roundtrips_with_embedded_batch_and_matches_versioned_fixture() {
        let header = sample_header();
        let records = vec![
            sample_record(
                0,
                Some(observed_time()),
                &[0xaa, 0xbb],
                multi_source_causes(),
            ),
            sample_record(1, None, &[0xcc], Vec::new()),
        ];
        let encoded = encode_segment(header, &records, &Limits::default()).unwrap();
        assert_eq!(&encoded[..4], &SEGMENT_MAGIC);
        assert_eq!(get_u16(&encoded, 4), FORMAT_VERSION);
        let decoded = decode_segment(&encoded, &Limits::default()).unwrap();
        assert_eq!(decoded, Segment { header, records });
    }

    #[test]
    fn segment_rejects_exhaustive_truncation_trailing_bytes_and_bit_flips() {
        let header = sample_header();
        let records = vec![sample_record(
            0,
            Some(observed_time()),
            &[1, 2],
            multi_source_causes(),
        )];
        let encoded = encode_segment(header, &records, &Limits::default()).unwrap();
        for cut in 0..encoded.len() {
            let result = decode_segment(&encoded[..cut], &Limits::default());
            let expected = if cut < SEGMENT_HEADER_LEN {
                Err(FormatError::Incomplete {
                    needed: SEGMENT_HEADER_LEN,
                    available: cut,
                })
            } else {
                Err(FormatError::Incomplete {
                    needed: encoded.len(),
                    available: cut,
                })
            };
            assert_eq!(result, expected, "cut={cut}");
        }

        let mut wrong_magic = encoded.clone();
        wrong_magic[0] ^= 0x01;
        assert_eq!(
            decode_segment(&wrong_magic, &Limits::default()),
            Err(FormatError::InvalidMagic)
        );

        let mut header_flip = encoded.clone();
        header_flip[24] ^= 0x01;
        assert_eq!(
            decode_segment(&header_flip, &Limits::default()),
            Err(FormatError::ChecksumMismatch)
        );

        let mut payload_flip = encoded.clone();
        let footer_start = encoded.len() - SEGMENT_FOOTER_LEN;
        payload_flip[footer_start - 1] ^= 0x40;
        assert_eq!(
            decode_segment(&payload_flip, &Limits::default()),
            Err(FormatError::ChecksumMismatch)
        );

        let mut trailing = encoded.clone();
        trailing.push(1);
        assert_eq!(
            decode_segment(&trailing, &Limits::default()),
            Err(FormatError::InvalidData("trailing segment bytes"))
        );
    }

    #[test]
    fn segment_rejects_noncanonical_footer_identity_mismatch_and_embedded_truncation() {
        let header = sample_header();
        let record = sample_record(0, None, &[1, 2], Vec::new());
        let encoded =
            encode_segment(header, std::slice::from_ref(&record), &Limits::default()).unwrap();

        let mut bad_footer = encoded.clone();
        let footer = bad_footer.len() - SEGMENT_FOOTER_LEN;
        put_u32(&mut bad_footer, footer + SEGMENT_FOOTER_DIR_LEN_OFFSET, 1);
        recompute_segment_checksums(&mut bad_footer);
        assert_eq!(
            decode_segment(&bad_footer, &Limits::default()),
            Err(FormatError::InvalidData("nonzero segment directory length"))
        );

        let mut mismatched = encoded.clone();
        let body_start = SEGMENT_HEADER_LEN;
        let body_len = get_u32(&mismatched, SEGMENT_BODY_LEN_OFFSET) as usize;
        put_u32(&mut mismatched, body_start + BATCH_HEADER_LEN, 0xfeed_beef);
        recompute_batch_header(&mut mismatched[body_start..body_start + body_len]);
        recompute_segment_checksums(&mut mismatched);
        assert_eq!(
            decode_segment(&mismatched, &Limits::default()),
            Err(FormatError::InvalidData(
                "record source or epoch differs from segment header"
            ))
        );

        let mut embedded_short = encoded.clone();
        let body_len = get_u32(&embedded_short, SEGMENT_BODY_LEN_OFFSET) as usize;
        let body_end = SEGMENT_HEADER_LEN + body_len;
        put_u32(&mut embedded_short, body_start + 8, (body_len as u32) + 10);
        recompute_batch_header(&mut embedded_short[body_start..body_end]);
        recompute_segment_checksums(&mut embedded_short);
        assert_eq!(
            decode_segment(&embedded_short, &Limits::default()),
            Err(FormatError::InvalidData(
                "complete segment contains a truncated embedded batch"
            ))
        );
    }

    #[test]
    fn segment_enforces_byte_limit() {
        let header = sample_header();
        let record = sample_record(0, None, &[1, 2, 3], Vec::new());
        let encoded = encode_segment(header, &[record], &Limits::default()).unwrap();
        assert_eq!(
            decode_segment(
                &encoded,
                &Limits {
                    max_segment_bytes: encoded.len() - 1,
                    ..Limits::default()
                }
            ),
            Err(FormatError::LimitExceeded("segment bytes"))
        );

        let mut too_many = encoded.clone();
        put_u32(&mut too_many, SEGMENT_RECORD_COUNT_OFFSET, 2);
        recompute_segment_checksums(&mut too_many);
        assert_eq!(
            decode_segment(
                &too_many,
                &Limits {
                    max_records: 1,
                    ..Limits::default()
                }
            ),
            Err(FormatError::LimitExceeded("records per batch"))
        );
    }
}
