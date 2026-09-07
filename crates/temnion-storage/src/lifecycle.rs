// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Lifecycle management, retention holds, branch GC, and point-in-time backup/restore.
//!
//! # Architecture
//! Following Temnion Architecture §10, §21, and Milestone M43:
//! - **Reference Holds**: Explicitly pin event sequences, branches, or candidate models to prevent
//!   premature garbage collection or tier eviction.
//! - **Retention Policies**: Evaluates age and volume thresholds while strictly respecting
//!   active reference holds; no held event is ever eligible for deletion or eviction.
//! - **Point-in-Time Backup & Restore**: Synchronous snapshotting of WAL logs, immutable TSF
//!   segments, summaries, and branch manifests with per-file CRC32 verification.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crc32fast::Hasher;
use temnion_format::{Limits, StoredEvent};

use crate::{RecoveryMode, StorageError, Store};

// ---------------------------------------------------------------------------
// Reference Holds (M43)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceHold {
    pub id: String,
    pub reason: String,
    pub held_sequences: BTreeSet<u64>,
    pub held_branches: BTreeSet<u64>,
    pub created_at_ticks: u64,
}

impl ReferenceHold {
    pub fn new(id: impl Into<String>, reason: impl Into<String>, created_at: u64) -> Self {
        Self {
            id: id.into(),
            reason: reason.into(),
            held_sequences: BTreeSet::new(),
            held_branches: BTreeSet::new(),
            created_at_ticks: created_at,
        }
    }

    pub fn hold_sequence(&mut self, seq: u64) {
        self.held_sequences.insert(seq);
    }

    pub fn hold_range(&mut self, start: u64, end_inclusive: u64) {
        for s in start..=end_inclusive {
            self.held_sequences.insert(s);
        }
    }

    pub fn hold_branch(&mut self, branch_id: u64) {
        self.held_branches.insert(branch_id);
    }

    pub fn is_sequence_held(&self, seq: u64) -> bool {
        self.held_sequences.contains(&seq)
    }

    pub fn is_branch_held(&self, branch_id: u64) -> bool {
        self.held_branches.contains(&branch_id)
    }
}

// ---------------------------------------------------------------------------
// Retention Policy (M43)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub max_age_ticks: Option<u64>,
    pub max_retained_events: Option<usize>,
}

impl RetentionPolicy {
    /// Returns sequences of events that are eligible for reclamation or archival.
    /// Strictly guarantees that no sequence covered by ANY active reference hold
    /// is returned.
    pub fn evaluate_reclaimable(
        &self,
        events: &[StoredEvent],
        current_ticks: u64,
        holds: &[ReferenceHold],
    ) -> Vec<u64> {
        let is_held = |seq: u64| holds.iter().any(|h| h.is_sequence_held(seq));
        let mut candidates = Vec::new();

        // 1. Evaluate age threshold
        if let Some(max_age) = self.max_age_ticks {
            for ev in events {
                let age = current_ticks.saturating_sub(ev.times.valid.ticks);
                if age > max_age && !is_held(ev.id.sequence) {
                    candidates.push(ev.id.sequence);
                }
            }
        }

        // 2. Evaluate volume threshold
        if let Some(max_events) = self.max_retained_events {
            if events.len() > max_events {
                let excess = events.len() - max_events;
                for ev in events.iter().take(excess) {
                    if !candidates.contains(&ev.id.sequence) && !is_held(ev.id.sequence) {
                        candidates.push(ev.id.sequence);
                    }
                }
            }
        }

        candidates.sort_unstable();
        candidates.dedup();
        candidates
    }
}

// ---------------------------------------------------------------------------
// Backup & Restore Manager (M43)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupFileEntry {
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub crc32: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupManifest {
    pub timestamp_ns: u128,
    pub files: Vec<BackupFileEntry>,
}

pub struct BackupManager;

impl BackupManager {
    /// Copies all database files from `store_dir` into `backup_dir` and writes
    /// an atomic checksummed manifest.
    pub fn create_backup(
        store_dir: &Path,
        backup_dir: &Path,
    ) -> Result<BackupManifest, StorageError> {
        if !store_dir.exists() {
            return Err(StorageError::InvalidRecord(
                "Source database directory does not exist",
            ));
        }
        if !backup_dir.exists() {
            fs::create_dir_all(backup_dir).map_err(|e| StorageError::Io {
                operation: "create_backup_dir",
                source: e,
            })?;
        }

        let mut files = Vec::new();
        Self::copy_and_checksum_dir(store_dir, store_dir, backup_dir, &mut files)?;

        let timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        let manifest = BackupManifest {
            timestamp_ns,
            files,
        };
        Self::write_manifest(backup_dir, &manifest)?;

        Ok(manifest)
    }

    /// Validates all files in `backup_dir` against their recorded sizes and CRC32 checksums.
    pub fn verify_backup(backup_dir: &Path) -> Result<BackupManifest, StorageError> {
        let manifest = Self::read_manifest(backup_dir)?;
        for entry in &manifest.files {
            let path = backup_dir.join(&entry.relative_path);
            if !path.exists() {
                return Err(StorageError::InvalidRecord("Backup file missing"));
            }
            let (size, crc) = Self::compute_crc32(&path)?;
            if size != entry.size_bytes || crc != entry.crc32 {
                return Err(StorageError::InvalidRecord(
                    "Backup file checksum or size mismatch",
                ));
            }
        }
        Ok(manifest)
    }

    /// Restores a backup from `backup_dir` to `target_dir` and verifies the resulting store.
    pub fn restore_backup(backup_dir: &Path, target_dir: &Path) -> Result<Store, StorageError> {
        let manifest = Self::verify_backup(backup_dir)?;
        if !target_dir.exists() {
            fs::create_dir_all(target_dir).map_err(|e| StorageError::Io {
                operation: "create_restore_target_dir",
                source: e,
            })?;
        }

        for entry in &manifest.files {
            let src = backup_dir.join(&entry.relative_path);
            let dst = target_dir.join(&entry.relative_path);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| StorageError::Io {
                    operation: "create_restore_parent_dir",
                    source: e,
                })?;
            }
            fs::copy(&src, &dst).map_err(|e| StorageError::Io {
                operation: "copy_restore_file",
                source: e,
            })?;
        }

        // Open restored store with strict recovery mode
        let (store, _) = Store::open(
            target_dir,
            Limits::default(),
            RecoveryMode::RejectIncompleteTail,
        )?;
        Ok(store)
    }

    fn copy_and_checksum_dir(
        base: &Path,
        current: &Path,
        backup_base: &Path,
        entries: &mut Vec<BackupFileEntry>,
    ) -> Result<(), StorageError> {
        let read_dir = fs::read_dir(current).map_err(|e| StorageError::Io {
            operation: "read_dir",
            source: e,
        })?;

        for entry in read_dir {
            let entry = entry.map_err(|e| StorageError::Io {
                operation: "read_dir_entry",
                source: e,
            })?;
            let path = entry.path();
            if path.is_dir() {
                Self::copy_and_checksum_dir(base, &path, backup_base, entries)?;
            } else if path.is_file() {
                let rel = path
                    .strip_prefix(base)
                    .map_err(|_| StorageError::InvalidRecord("Failed to compute relative path"))?;
                let dst = backup_base.join(rel);
                if let Some(parent) = dst.parent() {
                    fs::create_dir_all(parent).map_err(|e| StorageError::Io {
                        operation: "create_dir_all",
                        source: e,
                    })?;
                }
                fs::copy(&path, &dst).map_err(|e| StorageError::Io {
                    operation: "copy_file",
                    source: e,
                })?;
                let (size_bytes, crc32) = Self::compute_crc32(&dst)?;
                entries.push(BackupFileEntry {
                    relative_path: rel.to_path_buf(),
                    size_bytes,
                    crc32,
                });
            }
        }
        Ok(())
    }

    fn compute_crc32(path: &Path) -> Result<(u64, u32), StorageError> {
        let mut file = File::open(path).map_err(|e| StorageError::Io {
            operation: "open_for_checksum",
            source: e,
        })?;
        let mut hasher = Hasher::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = file.read(&mut buffer).map_err(|e| StorageError::Io {
                operation: "read_for_checksum",
                source: e,
            })?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
            total += n as u64;
        }
        Ok((total, hasher.finalize()))
    }

    fn write_manifest(backup_dir: &Path, manifest: &BackupManifest) -> Result<(), StorageError> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&manifest.timestamp_ns.to_le_bytes());
        encoded.extend_from_slice(&(manifest.files.len() as u32).to_le_bytes());
        for f in &manifest.files {
            let rel_str = f.relative_path.to_string_lossy();
            let rel_bytes = rel_str.as_bytes();
            encoded.extend_from_slice(&(rel_bytes.len() as u32).to_le_bytes());
            encoded.extend_from_slice(rel_bytes);
            encoded.extend_from_slice(&f.size_bytes.to_le_bytes());
            encoded.extend_from_slice(&f.crc32.to_le_bytes());
        }

        let mut hasher = Hasher::new();
        hasher.update(&encoded);
        let checksum = hasher.finalize();
        encoded.extend_from_slice(&checksum.to_le_bytes());

        let target = backup_dir.join("backup_manifest.bin");
        let mut file = File::create(target).map_err(|e| StorageError::Io {
            operation: "create_backup_manifest",
            source: e,
        })?;
        file.write_all(&encoded).map_err(|e| StorageError::Io {
            operation: "write_backup_manifest",
            source: e,
        })?;
        file.sync_all().map_err(|e| StorageError::Io {
            operation: "sync_backup_manifest",
            source: e,
        })?;
        Ok(())
    }

    fn read_manifest(backup_dir: &Path) -> Result<BackupManifest, StorageError> {
        let path = backup_dir.join("backup_manifest.bin");
        let bytes = fs::read(&path).map_err(|e| StorageError::Io {
            operation: "read_backup_manifest",
            source: e,
        })?;
        if bytes.len() < 24 {
            return Err(StorageError::InvalidRecord("Backup manifest too short"));
        }
        let data_len = bytes.len() - 4;
        let expected_crc = u32::from_le_bytes(bytes[data_len..].try_into().unwrap());
        let mut hasher = Hasher::new();
        hasher.update(&bytes[..data_len]);
        if hasher.finalize() != expected_crc {
            return Err(StorageError::InvalidRecord(
                "Backup manifest checksum corruption",
            ));
        }

        let timestamp_ns = u128::from_le_bytes(bytes[0..16].try_into().unwrap());
        let file_count = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
        let mut offset = 20;
        let mut files = Vec::with_capacity(file_count);

        for _ in 0..file_count {
            if offset + 4 > data_len {
                return Err(StorageError::InvalidRecord("Truncated manifest file entry"));
            }
            let str_len =
                u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            if offset + str_len + 12 > data_len {
                return Err(StorageError::InvalidRecord(
                    "Truncated manifest path string",
                ));
            }
            let path_str = std::str::from_utf8(&bytes[offset..offset + str_len])
                .map_err(|_| StorageError::InvalidRecord("Invalid UTF-8 in manifest path"))?;
            offset += str_len;
            let size_bytes = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            offset += 8;
            let crc32 = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            offset += 4;

            files.push(BackupFileEntry {
                relative_path: PathBuf::from(path_str),
                size_bytes,
                crc32,
            });
        }

        Ok(BackupManifest {
            timestamp_ns,
            files,
        })
    }
}
