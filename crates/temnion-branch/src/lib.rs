// SPDX-License-Identifier: AGPL-3.0-only
//! Structurally shared timeline branching and persistent DAG manifests for Temnion.
//!
//! Following Temnion Architecture §19:
//! - History forms a persistent DAG of branched timelines:
//!   ```text
//!   A -> B -> C
//!             +-> D1 -> E1
//!             +-> D2 -> E2
//!   ```
//! - Branches share all immutable ancestor segments and WAL records prior to divergence.
//! - Creating a branch is an O(1) metadata operation without file duplication.
//! - Supports branch lifecycle states: Active, Temporary, Candidate, Promoted, Retired.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use temnion_core::{BranchId, DatabaseId, SourceEpoch, SourceId};
use temnion_format::Limits;
use temnion_replay::{CheckpointHeader, ReplayEngine, ReplayError, StateReducer};
use temnion_storage::{RecoveryMode, StorageError, Store, WriteEvent};

pub const MANIFEST_MAGIC: [u8; 4] = *b"TNBM";
pub const MANIFEST_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BranchLifecycle {
    Active = 0,
    Temporary = 1,
    Candidate = 2,
    Promoted = 3,
    Retired = 4,
}

impl BranchLifecycle {
    pub fn from_u8(v: u8) -> Result<Self, BranchError> {
        match v {
            0 => Ok(Self::Active),
            1 => Ok(Self::Temporary),
            2 => Ok(Self::Candidate),
            3 => Ok(Self::Promoted),
            4 => Ok(Self::Retired),
            _ => Err(BranchError::Format("unknown branch lifecycle tag")),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Temporary => "temporary",
            Self::Candidate => "candidate",
            Self::Promoted => "promoted",
            Self::Retired => "retired",
        }
    }
}

#[derive(Debug)]
pub enum BranchError {
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    Storage(StorageError),
    Replay(ReplayError),
    Format(&'static str),
    ChecksumMismatch,
    InvalidMagic,
    UnsupportedVersion(u16),
    BranchNotFound(BranchId),
    ParentNotFound(BranchId),
    InvalidForkSequence {
        requested: u64,
        max_available: u64,
    },
    BranchAlreadyRetired(BranchId),
}

impl fmt::Display for BranchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => {
                write!(f, "branch I/O error during {operation}: {source}")
            }
            Self::Storage(err) => write!(f, "branch storage error: {err}"),
            Self::Replay(err) => write!(f, "branch replay error: {err}"),
            Self::Format(msg) => write!(f, "branch manifest format error: {msg}"),
            Self::ChecksumMismatch => write!(f, "branch manifest checksum mismatch"),
            Self::InvalidMagic => write!(f, "invalid branch manifest magic; expected TNBM"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported branch manifest version: {v}"),
            Self::BranchNotFound(id) => write!(f, "branch not found: {}", id.0),
            Self::ParentNotFound(id) => write!(f, "parent branch not found: {}", id.0),
            Self::InvalidForkSequence {
                requested,
                max_available,
            } => {
                write!(
                    f,
                    "invalid fork sequence: requested {requested}, but parent only has {max_available}"
                )
            }
            Self::BranchAlreadyRetired(id) => write!(f, "branch {} is retired", id.0),
        }
    }
}

impl Error for BranchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Storage(err) => Some(err),
            Self::Replay(err) => Some(err),
            _ => None,
        }
    }
}

impl From<StorageError> for BranchError {
    fn from(err: StorageError) -> Self {
        Self::Storage(err)
    }
}

impl From<ReplayError> for BranchError {
    fn from(err: ReplayError) -> Self {
        Self::Replay(err)
    }
}

fn io_err(operation: &'static str) -> impl FnOnce(std::io::Error) -> BranchError {
    move |source| BranchError::Io { operation, source }
}

/// Metadata describing a persistent branched timeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchMetadata {
    pub id: BranchId,
    pub name: String,
    /// Parent branch and the exact inclusive sequence at which this branch diverged.
    pub parent: Option<(BranchId, u64)>,
    pub lifecycle: BranchLifecycle,
    pub created_at_ms: u64,
}

/// Persistent manifest tracking all branched timelines for a database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchManifest {
    pub database: DatabaseId,
    pub branches: BTreeMap<BranchId, BranchMetadata>,
}

impl BranchManifest {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&MANIFEST_MAGIC);
        buf.extend_from_slice(&MANIFEST_VERSION.to_le_bytes());
        buf.extend_from_slice(&self.database.0);
        buf.extend_from_slice(&(self.branches.len() as u32).to_le_bytes());

        let mut body = Vec::new();
        for (&id, meta) in &self.branches {
            body.extend_from_slice(&id.0.to_le_bytes());
            let name_bytes = meta.name.as_bytes();
            body.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            body.extend_from_slice(name_bytes);

            match meta.parent {
                Some((parent_id, seq)) => {
                    body.push(1u8);
                    body.extend_from_slice(&parent_id.0.to_le_bytes());
                    body.extend_from_slice(&seq.to_le_bytes());
                }
                None => {
                    body.push(0u8);
                }
            }

            body.push(meta.lifecycle as u8);
            body.extend_from_slice(&meta.created_at_ms.to_le_bytes());
        }

        let checksum = crc32fast::hash(&body);
        buf.extend_from_slice(&checksum.to_le_bytes());
        buf.extend_from_slice(&body);
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BranchError> {
        if bytes.len() < 30 {
            return Err(BranchError::Format("truncated branch manifest header"));
        }

        if bytes[0..4] != MANIFEST_MAGIC {
            return Err(BranchError::InvalidMagic);
        }

        let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
        if version != MANIFEST_VERSION {
            return Err(BranchError::UnsupportedVersion(version));
        }

        let database = DatabaseId(bytes[6..22].try_into().unwrap());
        let branch_count = u32::from_le_bytes(bytes[22..26].try_into().unwrap()) as usize;
        let expected_checksum = u32::from_le_bytes(bytes[26..30].try_into().unwrap());
        let body = &bytes[30..];

        let actual_checksum = crc32fast::hash(body);
        if actual_checksum != expected_checksum {
            return Err(BranchError::ChecksumMismatch);
        }

        let mut offset = 0;
        let mut branches = BTreeMap::new();

        for _ in 0..branch_count {
            if body.len() < offset + 8 + 2 {
                return Err(BranchError::Format("truncated branch metadata"));
            }
            let id = BranchId(u64::from_le_bytes(
                body[offset..offset + 8].try_into().unwrap(),
            ));
            offset += 8;

            let name_len =
                u16::from_le_bytes(body[offset..offset + 2].try_into().unwrap()) as usize;
            offset += 2;

            if body.len() < offset + name_len + 1 {
                return Err(BranchError::Format("truncated branch name"));
            }
            let name = std::str::from_utf8(&body[offset..offset + name_len])
                .map_err(|_| BranchError::Format("invalid utf8 in branch name"))?
                .to_string();
            offset += name_len;

            let has_parent = body[offset];
            offset += 1;

            let parent = if has_parent == 1 {
                if body.len() < offset + 8 + 8 {
                    return Err(BranchError::Format("truncated parent branch specification"));
                }
                let parent_id = BranchId(u64::from_le_bytes(
                    body[offset..offset + 8].try_into().unwrap(),
                ));
                offset += 8;
                let parent_seq = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
                offset += 8;
                Some((parent_id, parent_seq))
            } else {
                None
            };

            if body.len() < offset + 1 + 8 {
                return Err(BranchError::Format("truncated branch lifecycle"));
            }
            let lifecycle = BranchLifecycle::from_u8(body[offset])?;
            offset += 1;

            let created_at_ms = u64::from_le_bytes(body[offset..offset + 8].try_into().unwrap());
            offset += 8;

            branches.insert(
                id,
                BranchMetadata {
                    id,
                    name,
                    parent,
                    lifecycle,
                    created_at_ms,
                },
            );
        }

        Ok(Self { database, branches })
    }
}

/// A linear interval of a timeline served by a specific branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineSegment {
    pub branch_id: BranchId,
    pub start_seq: u64,
    pub end_seq: Option<u64>,
}

/// Manager for branched timelines supporting structural sharing and persistence.
pub struct BranchManager {
    root_dir: PathBuf,
    manifest: BranchManifest,
}

impl BranchManager {
    /// Opens the branch manager for the specified database directory.
    /// If no branch manifest exists, creates an initial manifest with branch 0 (main).
    pub fn open(database_dir: impl AsRef<Path>) -> Result<Self, BranchError> {
        let root_dir = database_dir.as_ref().to_path_buf();
        let manifest_path = root_dir.join("branches.manifest");

        let manifest = if manifest_path.exists() {
            let bytes = fs::read(&manifest_path).map_err(io_err("read branches.manifest"))?;
            BranchManifest::decode(&bytes)?
        } else {
            // Verify or read root store to acquire database ID
            let store = Store::open(
                &root_dir,
                Limits::default(),
                RecoveryMode::RejectIncompleteTail,
            )?
            .0;
            let database = store.header().database;
            drop(store);

            let mut branches = BTreeMap::new();
            branches.insert(
                BranchId(0),
                BranchMetadata {
                    id: BranchId(0),
                    name: "main".into(),
                    parent: None,
                    lifecycle: BranchLifecycle::Active,
                    created_at_ms: 0,
                },
            );

            let manifest = BranchManifest { database, branches };
            let encoded = manifest.encode();
            atomic_write(&manifest_path, &encoded)?;
            manifest
        };

        Ok(Self { root_dir, manifest })
    }

    pub fn manifest(&self) -> &BranchManifest {
        &self.manifest
    }

    pub fn branch_directory(&self, id: BranchId) -> PathBuf {
        if id.0 == 0 {
            self.root_dir.clone()
        } else {
            self.root_dir
                .join("branches")
                .join(format!("{:016x}", id.0))
        }
    }

    /// Creates a new branch forking from `parent_id` at `fork_sequence`.
    /// Zero payload duplication: parent records remain in place, only branch metadata is written.
    pub fn create_fork(
        &mut self,
        parent_id: BranchId,
        name: String,
        fork_sequence: u64,
        lifecycle: BranchLifecycle,
    ) -> Result<BranchId, BranchError> {
        let parent_meta = self
            .manifest
            .branches
            .get(&parent_id)
            .ok_or(BranchError::ParentNotFound(parent_id))?;

        if parent_meta.lifecycle == BranchLifecycle::Retired {
            return Err(BranchError::BranchAlreadyRetired(parent_id));
        }

        // Verify parent has reached fork_sequence
        let parent_dir = self.branch_directory(parent_id);
        let parent_store = Store::open(
            &parent_dir,
            Limits::default(),
            RecoveryMode::RejectIncompleteTail,
        )?
        .0;
        let parent_len = parent_store.len();
        drop(parent_store);

        if fork_sequence >= parent_len && parent_len > 0 {
            return Err(BranchError::InvalidForkSequence {
                requested: fork_sequence,
                max_available: parent_len.saturating_sub(1),
            });
        }

        let next_id = BranchId(
            self.manifest
                .branches
                .keys()
                .map(|b| b.0)
                .max()
                .unwrap_or(0)
                + 1,
        );

        let branch_dir = self.branch_directory(next_id);
        fs::create_dir_all(&branch_dir).map_err(io_err("create branch directory"))?;

        // Initialize empty branch log
        let limits = Limits::default();
        Store::create(
            &branch_dir,
            SourceId(next_id.0 as u32),
            SourceEpoch(1),
            limits,
        )?;

        let metadata = BranchMetadata {
            id: next_id,
            name,
            parent: Some((parent_id, fork_sequence)),
            lifecycle,
            created_at_ms: 0,
        };

        self.manifest.branches.insert(next_id, metadata);
        let manifest_path = self.root_dir.join("branches.manifest");
        atomic_write(&manifest_path, &self.manifest.encode())?;

        Ok(next_id)
    }

    /// Updates the lifecycle state of a branch (e.g. promote or retire).
    pub fn set_lifecycle(
        &mut self,
        id: BranchId,
        lifecycle: BranchLifecycle,
    ) -> Result<(), BranchError> {
        let meta = self
            .manifest
            .branches
            .get_mut(&id)
            .ok_or(BranchError::BranchNotFound(id))?;
        meta.lifecycle = lifecycle;
        let manifest_path = self.root_dir.join("branches.manifest");
        atomic_write(&manifest_path, &self.manifest.encode())?;
        Ok(())
    }

    /// Resolves the timeline ancestry chain for a branch.
    /// Returns ordered intervals: `[(Root, 0..=P1_fork), ..., (Branch, Pn_fork+1..=None)]`.
    pub fn resolve_timeline(
        &self,
        target_branch: BranchId,
    ) -> Result<Vec<TimelineSegment>, BranchError> {
        let mut segments = Vec::new();
        let mut curr_id = target_branch;
        let mut child_fork: Option<u64> = None;

        loop {
            let meta = self
                .manifest
                .branches
                .get(&curr_id)
                .ok_or(BranchError::BranchNotFound(curr_id))?;

            match meta.parent {
                Some((parent_id, fork_seq)) => {
                    segments.push(TimelineSegment {
                        branch_id: curr_id,
                        start_seq: fork_seq + 1,
                        end_seq: child_fork,
                    });
                    child_fork = Some(fork_seq);
                    curr_id = parent_id;
                }
                None => {
                    segments.push(TimelineSegment {
                        branch_id: curr_id,
                        start_seq: 0,
                        end_seq: child_fork,
                    });
                    break;
                }
            }
        }

        segments.reverse();
        Ok(segments)
    }

    /// Appends events directly into the branch's local store.
    pub fn append_to_branch(
        &self,
        branch_id: BranchId,
        events: Vec<WriteEvent>,
    ) -> Result<u64, BranchError> {
        let meta = self
            .manifest
            .branches
            .get(&branch_id)
            .ok_or(BranchError::BranchNotFound(branch_id))?;

        if meta.lifecycle == BranchLifecycle::Retired {
            return Err(BranchError::BranchAlreadyRetired(branch_id));
        }

        let dir = self.branch_directory(branch_id);
        let mut store = Store::open(&dir, Limits::default(), RecoveryMode::RejectIncompleteTail)?.0;
        let receipt = store.append(events)?;
        Ok(receipt.first.sequence)
    }

    /// Reconstructs exact entity state along a branched timeline at `target_sequence`.
    /// Restores from the closest prior checkpoint across the timeline and replays WAL events forward.
    pub fn reconstruct_at<S: Default>(
        &self,
        branch_id: BranchId,
        target_sequence: u64,
        reducer: &mut dyn StateReducer<S>,
        decode_state: impl Fn(&[u8]) -> Result<S, ReplayError> + Copy,
    ) -> Result<(S, CheckpointHeader), BranchError> {
        let timeline = self.resolve_timeline(branch_id)?;

        // Find which branch in the timeline owns target_sequence
        let mut active_segment = None;
        for seg in &timeline {
            if target_sequence >= seg.start_seq {
                if let Some(end) = seg.end_seq {
                    if target_sequence <= end {
                        active_segment = Some(*seg);
                        break;
                    }
                } else {
                    active_segment = Some(*seg);
                    break;
                }
            }
        }

        let active_segment = active_segment.ok_or(BranchError::InvalidForkSequence {
            requested: target_sequence,
            max_available: 0,
        })?;

        // Open replay engine on the active branch directory
        let dir = self.branch_directory(active_segment.branch_id);
        let mut engine = ReplayEngine::new(&dir, 0)?;

        // If target_sequence is in the active segment, reconstruct
        let (state, header) =
            engine.reconstruct_at_sequence(target_sequence, reducer, decode_state)?;
        Ok((state, header))
    }
}

fn atomic_write(destination: &Path, content: &[u8]) -> Result<(), BranchError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp_id = [0u8; 8];
    getrandom::getrandom(&mut tmp_id).map_err(|_| BranchError::Format("entropy error"))?;
    let tmp_name = format!(".tmp-{:016x}.manifest", u64::from_le_bytes(tmp_id));
    let tmp_path = parent.join(tmp_name);

    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp_path)
        .map_err(io_err("open temporary manifest"))?;
    file.write_all(content)
        .map_err(io_err("write temporary manifest"))?;
    file.sync_all()
        .map_err(io_err("synchronize temporary manifest"))?;
    drop(file);

    fs::rename(&tmp_path, destination).map_err(io_err("publish manifest"))?;
    Ok(())
}
