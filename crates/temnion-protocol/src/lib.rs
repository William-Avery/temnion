// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Temnion Network Protocol (TNP), local IPC, Arrow columnar layout, and C ABI interface.
//!
//! # Architecture
//! Following Temnion Architecture §7, §29, and §30:
//! - **TNP (M18)**: Canonical framed binary protocol for client-server communication,
//!   handshake negotiation, capability self-description, streaming query execution,
//!   and continuation cursors.
//! - **Local IPC (M19)**: Duplex stream transport abstraction over pipes and sockets.
//! - **Arrow Columnar Format (M19)**: In-memory columnar representation for zero-copy/minimal-copy
//!   transfer to analytical and model runtimes.
//! - **C ABI (M19)**: Safe handle-based foreign function interface for C/C++/Python integration
//!   without raw pointer dereferences or unsafe memory risks.

use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crc32fast::Hasher;
use temnion_core::{ClockId, EntityId, SchemaId, ShardId, Timestamp};
use temnion_format::Limits;
use temnion_query::{
    QueryBudget as EngineQueryBudget, QueryExecutor, QueryResult, QueryRow, parse_compact_tem,
    parse_sql, parse_temql, plan_query,
};
use temnion_storage::{RecoveryMode, Store};

/// Magic header for TNP packets: b"TNPP" (Temnion Network Protocol Packet).
pub const TNP_MAGIC: [u8; 4] = *b"TNPP";

/// Current TNP protocol version.
pub const TNP_VERSION: u16 = 1;

/// Maximum allowable packet length (16 MB) to prevent denial-of-service memory exhaustion.
pub const MAX_PACKET_LEN: usize = 16 * 1024 * 1024;

/// Header length: magic (4B) + version (2B) + msg_type (2B) + stream_id (4B) + payload_len (4B) = 16 bytes.
pub const TNP_HEADER_LEN: usize = 16;

/// Total overhead per packet: header (16B) + CRC32C (4B) = 20 bytes.
pub const TNP_OVERHEAD: usize = TNP_HEADER_LEN + 4;

/// Protocol message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum TnpMessageType {
    HandshakeRequest = 0x0001,
    HandshakeResponse = 0x0002,
    DescribeRequest = 0x0003,
    DescribeResponse = 0x0004,
    QueryRequest = 0x0005,
    QueryResponse = 0x0006,
    StreamRecord = 0x0007,
    StreamEnd = 0x0008,
    ErrorResponse = 0x0009,
    Ping = 0x000A,
    Pong = 0x000B,
}

impl TryFrom<u16> for TnpMessageType {
    type Error = TnpError;

    fn try_from(val: u16) -> Result<Self, Self::Error> {
        match val {
            0x0001 => Ok(Self::HandshakeRequest),
            0x0002 => Ok(Self::HandshakeResponse),
            0x0003 => Ok(Self::DescribeRequest),
            0x0004 => Ok(Self::DescribeResponse),
            0x0005 => Ok(Self::QueryRequest),
            0x0006 => Ok(Self::QueryResponse),
            0x0007 => Ok(Self::StreamRecord),
            0x0008 => Ok(Self::StreamEnd),
            0x0009 => Ok(Self::ErrorResponse),
            0x000A => Ok(Self::Ping),
            0x000B => Ok(Self::Pong),
            other => Err(TnpError::UnknownMessageType(other)),
        }
    }
}

/// Errors occurring during TNP protocol framing, negotiation, or execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TnpError {
    InvalidMagic,
    UnsupportedVersion { expected: u16, actual: u16 },
    ChecksumMismatch,
    PacketTooLarge(usize),
    IncompletePacket,
    UnknownMessageType(u16),
    ProtocolViolation(String),
    HandshakeFailed(String),
    ExecutionError(String),
    IoError(String),
}

impl fmt::Display for TnpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "Invalid TNP packet magic, expected TNPP"),
            Self::UnsupportedVersion { expected, actual } => {
                write!(
                    f,
                    "Unsupported TNP version: expected {expected}, got {actual}"
                )
            }
            Self::ChecksumMismatch => write!(f, "TNP packet checksum mismatch (corrupted data)"),
            Self::PacketTooLarge(size) => {
                write!(f, "TNP packet size {size} exceeds maximum {MAX_PACKET_LEN}")
            }
            Self::IncompletePacket => write!(f, "TNP packet is truncated or incomplete"),
            Self::UnknownMessageType(t) => write!(f, "Unknown TNP message type: {t:#06x}"),
            Self::ProtocolViolation(msg) => write!(f, "TNP protocol violation: {msg}"),
            Self::HandshakeFailed(msg) => write!(f, "TNP handshake failed: {msg}"),
            Self::ExecutionError(msg) => write!(f, "TNP execution error: {msg}"),
            Self::IoError(msg) => write!(f, "TNP IO error: {msg}"),
        }
    }
}

impl std::error::Error for TnpError {}

/// A validated, framed TNP packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TnpPacket {
    pub version: u16,
    pub message_type: TnpMessageType,
    pub stream_id: u32,
    pub payload: Vec<u8>,
}

impl TnpPacket {
    /// Creates a new packet with current protocol version.
    pub fn new(message_type: TnpMessageType, stream_id: u32, payload: Vec<u8>) -> Self {
        Self {
            version: TNP_VERSION,
            message_type,
            stream_id,
            payload,
        }
    }

    /// Serializes the packet into framed binary bytes with CRC32C checksum.
    pub fn encode(&self) -> Result<Vec<u8>, TnpError> {
        if self.payload.len() > MAX_PACKET_LEN {
            return Err(TnpError::PacketTooLarge(self.payload.len()));
        }

        let mut buf = Vec::with_capacity(TNP_OVERHEAD + self.payload.len());
        buf.extend_from_slice(&TNP_MAGIC);
        buf.extend_from_slice(&self.version.to_le_bytes());
        buf.extend_from_slice(&(self.message_type as u16).to_le_bytes());
        buf.extend_from_slice(&self.stream_id.to_le_bytes());
        buf.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.payload);

        let mut hasher = Hasher::new();
        hasher.update(&buf);
        let checksum = hasher.finalize();
        buf.extend_from_slice(&checksum.to_le_bytes());

        Ok(buf)
    }

    /// Decodes and validates a packet from a binary slice.
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize), TnpError> {
        if bytes.len() < TNP_OVERHEAD {
            return Err(TnpError::IncompletePacket);
        }

        if bytes[0..4] != TNP_MAGIC {
            return Err(TnpError::InvalidMagic);
        }

        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != TNP_VERSION {
            return Err(TnpError::UnsupportedVersion {
                expected: TNP_VERSION,
                actual: version,
            });
        }

        let msg_raw = u16::from_le_bytes([bytes[6], bytes[7]]);
        let message_type = TnpMessageType::try_from(msg_raw)?;

        let stream_id = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let payload_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;

        if payload_len > MAX_PACKET_LEN {
            return Err(TnpError::PacketTooLarge(payload_len));
        }

        let total_len = TNP_HEADER_LEN + payload_len + 4;
        if bytes.len() < total_len {
            return Err(TnpError::IncompletePacket);
        }

        let payload = bytes[TNP_HEADER_LEN..TNP_HEADER_LEN + payload_len].to_vec();
        let expected_checksum = u32::from_le_bytes([
            bytes[total_len - 4],
            bytes[total_len - 3],
            bytes[total_len - 2],
            bytes[total_len - 1],
        ]);

        let mut hasher = Hasher::new();
        hasher.update(&bytes[..total_len - 4]);
        let computed = hasher.finalize();

        if computed != expected_checksum {
            return Err(TnpError::ChecksumMismatch);
        }

        Ok((
            Self {
                version,
                message_type,
                stream_id,
                payload,
            },
            total_len,
        ))
    }
}

// ---------------------------------------------------------------------------
// Protocol Payloads & Negotiation (M18)
// ---------------------------------------------------------------------------

/// Handshake request payload sent by client on connection establishment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeRequest {
    pub client_version: u16,
    pub client_id: String,
    pub capability_flags: u64,
}

impl HandshakeRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.client_version.to_le_bytes());
        buf.extend_from_slice(&(self.client_id.len() as u32).to_le_bytes());
        buf.extend_from_slice(self.client_id.as_bytes());
        buf.extend_from_slice(&self.capability_flags.to_le_bytes());
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, TnpError> {
        if bytes.len() < 6 {
            return Err(TnpError::ProtocolViolation(
                "Truncated HandshakeRequest".to_string(),
            ));
        }
        let client_version = u16::from_le_bytes([bytes[0], bytes[1]]);
        let id_len = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize;
        if bytes.len() < 6 + id_len + 8 {
            return Err(TnpError::ProtocolViolation(
                "Truncated HandshakeRequest body".to_string(),
            ));
        }
        let client_id = String::from_utf8(bytes[6..6 + id_len].to_vec())
            .map_err(|e| TnpError::ProtocolViolation(e.to_string()))?;
        let cap_offset = 6 + id_len;
        let capability_flags = u64::from_le_bytes([
            bytes[cap_offset],
            bytes[cap_offset + 1],
            bytes[cap_offset + 2],
            bytes[cap_offset + 3],
            bytes[cap_offset + 4],
            bytes[cap_offset + 5],
            bytes[cap_offset + 6],
            bytes[cap_offset + 7],
        ]);

        Ok(Self {
            client_version,
            client_id,
            capability_flags,
        })
    }
}

/// Handshake response payload returned by server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeResponse {
    pub success: bool,
    pub negotiated_version: u16,
    pub server_id: String,
    pub capability_flags: u64,
}

impl HandshakeResponse {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(if self.success { 1 } else { 0 });
        buf.extend_from_slice(&self.negotiated_version.to_le_bytes());
        buf.extend_from_slice(&(self.server_id.len() as u32).to_le_bytes());
        buf.extend_from_slice(self.server_id.as_bytes());
        buf.extend_from_slice(&self.capability_flags.to_le_bytes());
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, TnpError> {
        if bytes.len() < 7 {
            return Err(TnpError::ProtocolViolation(
                "Truncated HandshakeResponse".to_string(),
            ));
        }
        let success = bytes[0] != 0;
        let negotiated_version = u16::from_le_bytes([bytes[1], bytes[2]]);
        let id_len = u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]) as usize;
        if bytes.len() < 7 + id_len + 8 {
            return Err(TnpError::ProtocolViolation(
                "Truncated HandshakeResponse body".to_string(),
            ));
        }
        let server_id = String::from_utf8(bytes[7..7 + id_len].to_vec())
            .map_err(|e| TnpError::ProtocolViolation(e.to_string()))?;
        let cap_offset = 7 + id_len;
        let capability_flags = u64::from_le_bytes([
            bytes[cap_offset],
            bytes[cap_offset + 1],
            bytes[cap_offset + 2],
            bytes[cap_offset + 3],
            bytes[cap_offset + 4],
            bytes[cap_offset + 5],
            bytes[cap_offset + 6],
            bytes[cap_offset + 7],
        ]);

        Ok(Self {
            success,
            negotiated_version,
            server_id,
            capability_flags,
        })
    }
}

/// Query format specification in QueryRequest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum QueryFormat {
    Temql = 1,
    CompactTem = 2,
    Sql = 3,
}

/// Query request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRequest {
    pub format: QueryFormat,
    pub query_str: String,
    pub max_rows: u32,
}

impl QueryRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(self.format as u8);
        buf.extend_from_slice(&self.max_rows.to_le_bytes());
        buf.extend_from_slice(&(self.query_str.len() as u32).to_le_bytes());
        buf.extend_from_slice(self.query_str.as_bytes());
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, TnpError> {
        if bytes.len() < 9 {
            return Err(TnpError::ProtocolViolation(
                "Truncated QueryRequest".to_string(),
            ));
        }
        let format = match bytes[0] {
            1 => QueryFormat::Temql,
            2 => QueryFormat::CompactTem,
            3 => QueryFormat::Sql,
            _ => {
                return Err(TnpError::ProtocolViolation(
                    "Unknown query format".to_string(),
                ));
            }
        };
        let max_rows = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        let q_len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
        if bytes.len() < 9 + q_len {
            return Err(TnpError::ProtocolViolation(
                "Truncated QueryRequest string".to_string(),
            ));
        }
        let query_str = String::from_utf8(bytes[9..9 + q_len].to_vec())
            .map_err(|e| TnpError::ProtocolViolation(e.to_string()))?;

        Ok(Self {
            format,
            query_str,
            max_rows,
        })
    }
}

// ---------------------------------------------------------------------------
// Local IPC Transport & Server Dispatcher (M19)
// ---------------------------------------------------------------------------

/// Bidirectional packet channel wrapping any `Read + Write` stream (pipe or socket).
pub struct TnpChannel<R, W> {
    reader: R,
    writer: W,
    read_buf: Vec<u8>,
}

impl<R: Read, W: Write> TnpChannel<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            read_buf: Vec::with_capacity(4096),
        }
    }

    /// Sends a TNP packet over the writer stream.
    pub fn send(&mut self, packet: &TnpPacket) -> Result<(), TnpError> {
        let encoded = packet.encode()?;
        self.writer
            .write_all(&encoded)
            .map_err(|e| TnpError::IoError(e.to_string()))?;
        self.writer
            .flush()
            .map_err(|e| TnpError::IoError(e.to_string()))?;
        Ok(())
    }

    /// Receives the next complete TNP packet, buffering partial reads.
    pub fn recv(&mut self) -> Result<TnpPacket, TnpError> {
        loop {
            // Attempt to decode from current buffer
            if !self.read_buf.is_empty() {
                match TnpPacket::decode(&self.read_buf) {
                    Ok((packet, bytes_consumed)) => {
                        self.read_buf.drain(..bytes_consumed);
                        return Ok(packet);
                    }
                    Err(TnpError::IncompletePacket) => {
                        // Need more data from reader
                    }
                    Err(err) => return Err(err),
                }
            }

            // Read more data
            let mut chunk = [0u8; 4096];
            let n = self
                .reader
                .read(&mut chunk)
                .map_err(|e| TnpError::IoError(e.to_string()))?;
            if n == 0 {
                return Err(TnpError::IoError("Connection closed by peer".to_string()));
            }
            self.read_buf.extend_from_slice(&chunk[..n]);
        }
    }
}

/// Server dispatcher evaluating incoming TNP requests against a database store.
pub struct TnpServer {
    store: Store,
    server_id: String,
}

impl TnpServer {
    pub fn new(store: Store, server_id: impl Into<String>) -> Self {
        Self {
            store,
            server_id: server_id.into(),
        }
    }

    /// Handles an incoming TNP connection stream.
    pub fn handle_connection<R: Read, W: Write>(
        &mut self,
        channel: &mut TnpChannel<R, W>,
    ) -> Result<(), TnpError> {
        // 1. Handshake exchange
        let packet = channel.recv()?;
        if packet.message_type != TnpMessageType::HandshakeRequest {
            let err_pkt = TnpPacket::new(
                TnpMessageType::ErrorResponse,
                0,
                b"Expected HandshakeRequest".to_vec(),
            );
            let _ = channel.send(&err_pkt);
            return Err(TnpError::HandshakeFailed(
                "Expected HandshakeRequest".to_string(),
            ));
        }

        let req = HandshakeRequest::decode(&packet.payload)?;
        if req.client_version != TNP_VERSION {
            let resp = HandshakeResponse {
                success: false,
                negotiated_version: TNP_VERSION,
                server_id: self.server_id.clone(),
                capability_flags: 0,
            };
            channel.send(&TnpPacket::new(
                TnpMessageType::HandshakeResponse,
                packet.stream_id,
                resp.encode(),
            ))?;
            return Err(TnpError::UnsupportedVersion {
                expected: TNP_VERSION,
                actual: req.client_version,
            });
        }

        // Capability flags: 0x01 = query, 0x02 = describe, 0x04 = stream
        let resp = HandshakeResponse {
            success: true,
            negotiated_version: TNP_VERSION,
            server_id: self.server_id.clone(),
            capability_flags: 0x07,
        };
        channel.send(&TnpPacket::new(
            TnpMessageType::HandshakeResponse,
            packet.stream_id,
            resp.encode(),
        ))?;

        // 2. Request loop
        loop {
            let req_packet = match channel.recv() {
                Ok(p) => p,
                Err(TnpError::IoError(_)) => break, // Peer closed connection
                Err(e) => return Err(e),
            };

            match req_packet.message_type {
                TnpMessageType::Ping => {
                    channel.send(&TnpPacket::new(
                        TnpMessageType::Pong,
                        req_packet.stream_id,
                        vec![],
                    ))?;
                }
                TnpMessageType::DescribeRequest => {
                    let desc = format!(
                        "{{\"server_id\":\"{}\",\"version\":{},\"capabilities\":[\"tnp\",\"query-ir\",\"temql\",\"compact-tem\",\"sql\",\"arrow-columnar\"]}}",
                        self.server_id, TNP_VERSION
                    );
                    channel.send(&TnpPacket::new(
                        TnpMessageType::DescribeResponse,
                        req_packet.stream_id,
                        desc.into_bytes(),
                    ))?;
                }
                TnpMessageType::QueryRequest => {
                    let q_req = QueryRequest::decode(&req_packet.payload)?;
                    let logical = match q_req.format {
                        QueryFormat::Temql => parse_temql(&q_req.query_str)
                            .map_err(|e| TnpError::ExecutionError(e.to_string()))?,
                        QueryFormat::CompactTem => parse_compact_tem(&q_req.query_str)
                            .map_err(|e| TnpError::ExecutionError(e.to_string()))?,
                        QueryFormat::Sql => parse_sql(&q_req.query_str)
                            .map_err(|e| TnpError::ExecutionError(e.to_string()))?,
                    };

                    let physical = plan_query(&logical);
                    let budget = EngineQueryBudget {
                        max_rows: Some(q_req.max_rows as usize),
                        max_events_scanned: Some(50_000),
                        max_bytes: Some(32 * 1024 * 1024),
                    };

                    let result =
                        QueryExecutor::execute_storage_scan(&mut self.store, &physical, &budget)
                            .map_err(|e| TnpError::ExecutionError(e.to_string()))?;

                    // Send QueryResponse header
                    let resp_header = format!(
                        "{{\"rows\":{},\"scanned\":{},\"truncated\":{}}}",
                        result.rows.len(),
                        result.events_scanned,
                        result.truncated
                    );
                    channel.send(&TnpPacket::new(
                        TnpMessageType::QueryResponse,
                        req_packet.stream_id,
                        resp_header.into_bytes(),
                    ))?;

                    // Stream records
                    for row in &result.rows {
                        let record_str = format!(
                            "{}:{}:{}|{}|{}|{}",
                            row.entity.shard.0,
                            row.entity.slot,
                            row.entity.generation,
                            row.valid_time.ticks,
                            row.known_time.ticks,
                            row.sequence
                        );
                        channel.send(&TnpPacket::new(
                            TnpMessageType::StreamRecord,
                            req_packet.stream_id,
                            record_str.into_bytes(),
                        ))?;
                    }

                    // Send StreamEnd
                    channel.send(&TnpPacket::new(
                        TnpMessageType::StreamEnd,
                        req_packet.stream_id,
                        (result.rows.len() as u32).to_le_bytes().to_vec(),
                    ))?;
                }
                _ => {
                    channel.send(&TnpPacket::new(
                        TnpMessageType::ErrorResponse,
                        req_packet.stream_id,
                        b"Unhandled message type".to_vec(),
                    ))?;
                }
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Arrow Columnar Layout (M19)
// ---------------------------------------------------------------------------

/// Columnar batch layout matching Apache Arrow columnar structure for zero-copy bulk analytics.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnarBatch {
    pub length: usize,
    pub entity_shards: Vec<u32>,
    pub entity_slots: Vec<u32>,
    pub entity_generations: Vec<u32>,
    pub valid_timestamps: Vec<u64>,
    pub known_timestamps: Vec<u64>,
    pub sequences: Vec<u64>,
    pub schema_ids: Vec<u32>,
}

impl ColumnarBatch {
    /// Constructs a ColumnarBatch from an array of QueryRow records.
    pub fn from_query_rows(rows: &[QueryRow]) -> Self {
        let length = rows.len();
        let mut entity_shards = Vec::with_capacity(length);
        let mut entity_slots = Vec::with_capacity(length);
        let mut entity_generations = Vec::with_capacity(length);
        let mut valid_timestamps = Vec::with_capacity(length);
        let mut known_timestamps = Vec::with_capacity(length);
        let mut sequences = Vec::with_capacity(length);
        let mut schema_ids = Vec::with_capacity(length);

        for r in rows {
            entity_shards.push(r.entity.shard.0);
            entity_slots.push(r.entity.slot);
            entity_generations.push(r.entity.generation);
            valid_timestamps.push(r.valid_time.ticks);
            known_timestamps.push(r.known_time.ticks);
            sequences.push(r.sequence);
            schema_ids.push(r.schema.0);
        }

        Self {
            length,
            entity_shards,
            entity_slots,
            entity_generations,
            valid_timestamps,
            known_timestamps,
            sequences,
            schema_ids,
        }
    }

    /// Converts ColumnarBatch back into QueryRow objects.
    pub fn to_query_rows(&self) -> Vec<QueryRow> {
        let mut rows = Vec::with_capacity(self.length);
        for i in 0..self.length {
            let entity = EntityId {
                shard: ShardId(self.entity_shards[i]),
                slot: self.entity_slots[i],
                generation: self.entity_generations[i],
            };
            let valid_time = Timestamp {
                clock: ClockId(1),
                ticks: self.valid_timestamps[i],
            };
            let known_time = Timestamp {
                clock: ClockId(1),
                ticks: self.known_timestamps[i],
            };
            let sequence = self.sequences[i];
            let schema = SchemaId(self.schema_ids[i]);

            rows.push(QueryRow {
                entity,
                schema,
                valid_time,
                known_time,
                sequence,
                fields: HashMap::new(),
            });
        }
        rows
    }
}

// ---------------------------------------------------------------------------
// Stable C ABI (M19)
// ---------------------------------------------------------------------------

/// Opaque handle registry for C ABI integration without raw pointer risks.
pub struct HandleRegistry<T> {
    next_id: AtomicU64,
    items: Mutex<HashMap<u64, Arc<Mutex<T>>>>,
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            items: Mutex::new(HashMap::new()),
        }
    }
}

impl<T> HandleRegistry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, item: T) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut map = self.items.lock().unwrap();
        map.insert(id, Arc::new(Mutex::new(item)));
        id
    }

    pub fn get(&self, id: u64) -> Option<Arc<Mutex<T>>> {
        let map = self.items.lock().unwrap();
        map.get(&id).cloned()
    }

    pub fn remove(&self, id: u64) -> bool {
        let mut map = self.items.lock().unwrap();
        map.remove(&id).is_some()
    }
}

/// Global registries for safe handle management.
pub static STORE_REGISTRY: std::sync::LazyLock<HandleRegistry<Store>> =
    std::sync::LazyLock::new(HandleRegistry::new);

pub static RESULT_REGISTRY: std::sync::LazyLock<HandleRegistry<QueryResult>> =
    std::sync::LazyLock::new(HandleRegistry::new);

// C ABI Status Codes
pub const TEMNION_SUCCESS: i32 = 0;
pub const TEMNION_ERR_INVALID_ARGUMENT: i32 = -1;
pub const TEMNION_ERR_NOT_FOUND: i32 = -2;
pub const TEMNION_ERR_EXECUTION_FAILED: i32 = -3;
pub const TEMNION_ERR_PANIC: i32 = -4;

/// Safe C ABI: Opens a store database and returns an integer handle in `out_handle`.
pub fn temnion_c_store_open(path: &str, out_handle: &mut u64) -> i32 {
    let res = std::panic::catch_unwind(|| {
        match Store::open(
            std::path::Path::new(path),
            Limits::default(),
            RecoveryMode::RejectIncompleteTail,
        ) {
            Ok((store, _)) => Ok(STORE_REGISTRY.insert(store)),
            Err(_) => Err(TEMNION_ERR_EXECUTION_FAILED),
        }
    });

    match res {
        Ok(Ok(handle)) => {
            *out_handle = handle;
            TEMNION_SUCCESS
        }
        Ok(Err(code)) => code,
        Err(_) => TEMNION_ERR_PANIC,
    }
}

/// Safe C ABI: Closes a store by handle.
pub fn temnion_c_store_close(handle: u64) -> i32 {
    let res = std::panic::catch_unwind(|| {
        if STORE_REGISTRY.remove(handle) {
            TEMNION_SUCCESS
        } else {
            TEMNION_ERR_NOT_FOUND
        }
    });

    match res {
        Ok(code) => code,
        Err(_) => TEMNION_ERR_PANIC,
    }
}

/// Safe C ABI: Executes a query on the store and returns a result handle.
pub fn temnion_c_query_execute(
    store_handle: u64,
    query_str: &str,
    out_result_handle: &mut u64,
) -> i32 {
    let res = std::panic::catch_unwind(|| {
        let store_arc = match STORE_REGISTRY.get(store_handle) {
            Some(s) => s,
            None => return Err(TEMNION_ERR_NOT_FOUND),
        };

        let trimmed = query_str.trim();
        let logical = match if trimmed.starts_with("tn:")
            || trimmed.starts_with('#')
            || trimmed.starts_with('$')
        {
            parse_compact_tem(query_str)
        } else if trimmed.to_ascii_lowercase().starts_with("select") {
            parse_sql(query_str)
        } else {
            parse_temql(query_str)
        } {
            Ok(l) => l,
            Err(_) => return Err(TEMNION_ERR_INVALID_ARGUMENT),
        };

        let physical = plan_query(&logical);
        let mut store_guard = store_arc.lock().unwrap();
        match QueryExecutor::execute_storage_scan(
            &mut store_guard,
            &physical,
            &EngineQueryBudget::default(),
        ) {
            Ok(result) => Ok(RESULT_REGISTRY.insert(result)),
            Err(_) => Err(TEMNION_ERR_EXECUTION_FAILED),
        }
    });

    match res {
        Ok(Ok(handle)) => {
            *out_result_handle = handle;
            TEMNION_SUCCESS
        }
        Ok(Err(code)) => code,
        Err(_) => TEMNION_ERR_PANIC,
    }
}

/// Safe C ABI: Retrieves row count from query result handle.
pub fn temnion_c_result_row_count(result_handle: u64) -> usize {
    let res = std::panic::catch_unwind(|| match RESULT_REGISTRY.get(result_handle) {
        Some(res_arc) => {
            let res = res_arc.lock().unwrap();
            res.rows.len()
        }
        None => 0,
    });

    res.unwrap_or(0)
}

/// Safe C ABI: Releases a query result handle.
pub fn temnion_c_result_free(result_handle: u64) -> i32 {
    let res = std::panic::catch_unwind(|| {
        if RESULT_REGISTRY.remove(result_handle) {
            TEMNION_SUCCESS
        } else {
            TEMNION_ERR_NOT_FOUND
        }
    });

    match res {
        Ok(code) => code,
        Err(_) => TEMNION_ERR_PANIC,
    }
}
