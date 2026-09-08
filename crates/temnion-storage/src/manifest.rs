// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Segment manifest tracking published immutable TSF segments and WAL retirement.
//!
//! # Architecture
//! Following Temnion Architecture §4, §10, and §21:
//! - **Segment Manifest (`manifest.bin`)**: Durably and atomically records all published
//!   immutable `.tsf` segments, their byte lengths, sequence ranges, and CRC32 checksums.
//! - **WAL Retirement**: Allows retiring/compacting WAL frames that have been verified and
//!   published into immutable segments, as long as they are not protected by active reference holds.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crc32fast::Hasher;
use temnion_core::{DatabaseId, SourceEpoch, SourceId};

use crate::StorageError;

const MANIFEST_MAGIC: &[u8; 4] = b"TNMN";
const MANIFEST_VERSION: u16 = 1;

/// Metadata for a single published immutable segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedSegmentMeta {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub filename: String,
    pub byte_length: u64,
    pub crc32: u32,
    pub sealed_at_ticks: u64,
}

/// Durable manifest of all active sealed segments and WAL retirement progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentManifest {
    pub database: DatabaseId,
    pub source: SourceId,
    pub epoch: SourceEpoch,
    pub sealed_segments: Vec<SealedSegmentMeta>,
    pub retired_up_to_sequence: Option<u64>,
}

impl SegmentManifest {
    pub fn new(database: DatabaseId, source: SourceId, epoch: SourceEpoch) -> Self {
        Self {
            database,
            source,
            epoch,
            sealed_segments: Vec::new(),
            retired_up_to_sequence: None,
        }
    }

    /// Registers a sealed segment in the manifest (replaces if identical sequence range exists).
    pub fn register_segment(&mut self, meta: SealedSegmentMeta) {
        if let Some(pos) = self.sealed_segments.iter().position(|s| {
            s.first_sequence == meta.first_sequence && s.last_sequence == meta.last_sequence
        }) {
            self.sealed_segments[pos] = meta;
        } else {
            self.sealed_segments.push(meta);
            self.sealed_segments
                .sort_by_key(|s| (s.first_sequence, s.last_sequence));
        }
    }

    /// Checks if a sequence is covered by any sealed segment.
    pub fn is_sequence_sealed(&self, seq: u64) -> bool {
        self.sealed_segments
            .iter()
            .any(|s| seq >= s.first_sequence && seq <= s.last_sequence)
    }

    /// Finds the sealed segment metadata covering a given sequence.
    pub fn find_segment_for_sequence(&self, seq: u64) -> Option<&SealedSegmentMeta> {
        self.sealed_segments
            .iter()
            .find(|s| seq >= s.first_sequence && seq <= s.last_sequence)
    }

    /// Encodes manifest into a binary representation with CRC32 checksum.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(128 + self.sealed_segments.len() * 64);
        buf.extend_from_slice(MANIFEST_MAGIC);
        buf.extend_from_slice(&MANIFEST_VERSION.to_le_bytes());
        buf.extend_from_slice(&self.database.0);
        buf.extend_from_slice(&self.source.0.to_le_bytes());
        buf.extend_from_slice(&self.epoch.0.to_le_bytes());

        match self.retired_up_to_sequence {
            Some(seq) => {
                buf.push(1);
                buf.extend_from_slice(&seq.to_le_bytes());
            }
            None => {
                buf.push(0);
                buf.extend_from_slice(&0u64.to_le_bytes());
            }
        }

        buf.extend_from_slice(&(self.sealed_segments.len() as u32).to_le_bytes());
        for seg in &self.sealed_segments {
            buf.extend_from_slice(&seg.first_sequence.to_le_bytes());
            buf.extend_from_slice(&seg.last_sequence.to_le_bytes());
            buf.extend_from_slice(&seg.byte_length.to_le_bytes());
            buf.extend_from_slice(&seg.crc32.to_le_bytes());
            buf.extend_from_slice(&seg.sealed_at_ticks.to_le_bytes());

            let name_bytes = seg.filename.as_bytes();
            buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(name_bytes);
        }

        let mut hasher = Hasher::new();
        hasher.update(&buf);
        let checksum = hasher.finalize();
        buf.extend_from_slice(&checksum.to_le_bytes());
        buf
    }

    /// Decodes manifest from binary bytes and verifies CRC32 checksum.
    pub fn decode(bytes: &[u8]) -> Result<Self, StorageError> {
        if bytes.len() < 42 {
            return Err(StorageError::InvalidRecord("Segment manifest too short"));
        }

        let data_len = bytes.len() - 4;
        let expected_checksum = u32::from_le_bytes([
            bytes[data_len],
            bytes[data_len + 1],
            bytes[data_len + 2],
            bytes[data_len + 3],
        ]);

        let mut hasher = Hasher::new();
        hasher.update(&bytes[..data_len]);
        if hasher.finalize() != expected_checksum {
            return Err(StorageError::InvalidRecord(
                "Segment manifest CRC32 mismatch",
            ));
        }

        if &bytes[0..4] != MANIFEST_MAGIC {
            return Err(StorageError::InvalidRecord(
                "Invalid segment manifest magic",
            ));
        }

        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != MANIFEST_VERSION {
            return Err(StorageError::InvalidRecord(
                "Unsupported segment manifest version",
            ));
        }

        let mut db_id = [0u8; 16];
        db_id.copy_from_slice(&bytes[6..22]);
        let database = DatabaseId(db_id);

        let source = SourceId(u32::from_le_bytes([
            bytes[22], bytes[23], bytes[24], bytes[25],
        ]));
        let epoch = SourceEpoch(u64::from_le_bytes([
            bytes[26], bytes[27], bytes[28], bytes[29], bytes[30], bytes[31], bytes[32], bytes[33],
        ]));

        let retired_flag = bytes[34];
        let retired_seq = u64::from_le_bytes([
            bytes[35], bytes[36], bytes[37], bytes[38], bytes[39], bytes[40], bytes[41], bytes[42],
        ]);
        let retired_up_to_sequence = if retired_flag == 1 {
            Some(retired_seq)
        } else {
            None
        };

        let mut offset = 43;
        if bytes.len() < offset + 4 {
            return Err(StorageError::InvalidRecord("Truncated segment count"));
        }
        let count = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;

        let mut sealed_segments = Vec::with_capacity(count);
        for _ in 0..count {
            if bytes.len() < offset + 34 {
                return Err(StorageError::InvalidRecord("Truncated segment entry"));
            }
            let first_sequence = u64::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
                bytes[offset + 4],
                bytes[offset + 5],
                bytes[offset + 6],
                bytes[offset + 7],
            ]);
            let last_sequence = u64::from_le_bytes([
                bytes[offset + 8],
                bytes[offset + 9],
                bytes[offset + 10],
                bytes[offset + 11],
                bytes[offset + 12],
                bytes[offset + 13],
                bytes[offset + 14],
                bytes[offset + 15],
            ]);
            let byte_length = u64::from_le_bytes([
                bytes[offset + 16],
                bytes[offset + 17],
                bytes[offset + 18],
                bytes[offset + 19],
                bytes[offset + 20],
                bytes[offset + 21],
                bytes[offset + 22],
                bytes[offset + 23],
            ]);
            let crc32 = u32::from_le_bytes([
                bytes[offset + 24],
                bytes[offset + 25],
                bytes[offset + 26],
                bytes[offset + 27],
            ]);
            let sealed_at_ticks = u64::from_le_bytes([
                bytes[offset + 28],
                bytes[offset + 29],
                bytes[offset + 30],
                bytes[offset + 31],
                bytes[offset + 32],
                bytes[offset + 33],
                bytes[offset + 34],
                bytes[offset + 35],
            ]);
            let name_len = u16::from_le_bytes([bytes[offset + 36], bytes[offset + 37]]) as usize;
            offset += 38;

            if bytes.len() < offset + name_len {
                return Err(StorageError::InvalidRecord("Truncated segment filename"));
            }
            let filename = std::str::from_utf8(&bytes[offset..offset + name_len])
                .map_err(|_| StorageError::InvalidRecord("Invalid UTF-8 in segment filename"))?
                .to_string();
            offset += name_len;

            sealed_segments.push(SealedSegmentMeta {
                first_sequence,
                last_sequence,
                filename,
                byte_length,
                crc32,
                sealed_at_ticks,
            });
        }

        Ok(Self {
            database,
            source,
            epoch,
            sealed_segments,
            retired_up_to_sequence,
        })
    }

    /// Loads manifest from disk, or returns Ok(None) if manifest file does not exist yet.
    pub fn load_if_exists(path: &Path) -> Result<Option<Self>, StorageError> {
        if !path.try_exists().map_err(|e| StorageError::Io {
            operation: "inspect_manifest",
            source: e,
        })? {
            return Ok(None);
        }
        let bytes = fs::read(path).map_err(|e| StorageError::Io {
            operation: "read_manifest",
            source: e,
        })?;
        let manifest = Self::decode(&bytes)?;
        Ok(Some(manifest))
    }

    /// Atomically persists manifest via write-to-temp and atomic replace.
    pub fn save(&self, target_path: &Path) -> Result<(), StorageError> {
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|e| StorageError::Io {
                operation: "create_manifest_dir",
                source: e,
            })?;
        }

        let temp_name = format!(".manifest_tmp_{}", now_ns());
        let temp_path = target_path.with_file_name(temp_name);
        let encoded = self.encode();

        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|e| StorageError::Io {
                operation: "create_temp_manifest",
                source: e,
            })?;
        file.write_all(&encoded).map_err(|e| StorageError::Io {
            operation: "write_temp_manifest",
            source: e,
        })?;
        file.sync_all().map_err(|e| StorageError::Io {
            operation: "sync_temp_manifest",
            source: e,
        })?;
        drop(file);

        fs::rename(&temp_path, target_path).map_err(|e| StorageError::Io {
            operation: "replace_manifest",
            source: e,
        })?;

        Ok(())
    }
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
