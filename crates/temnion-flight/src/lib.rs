// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Arrow Flight remote analytical transport service for Temnion.
//!
//! # Architecture
//! Following Temnion Architecture §7, §29, and Milestone M20:
//! - **Flight Descriptors & Tickets**: Clients submit queries via `FlightDescriptor`
//!   and receive `FlightInfo` with schema and partitioned `FlightEndpoint` tickets.
//! - **Streaming FlightData**: Tickets are redeemed via `do_get`, streaming
//!   `FlightData` packets containing Arrow-compatible `ColumnarBatch` analytical vectors.
//! - **Authentication & Actions**: Handshake exchange verifies bearer tokens and
//!   `do_action` enables remote health checks and metadata introspection.
//! - **Zero Unsafe Code**: All buffer manipulations, serialization, and stream
//!   framing strictly enforce `#![forbid(unsafe_code)]`.

use std::fmt;

use crc32fast::Hasher;
use temnion_protocol::ColumnarBatch;
use temnion_query::{
    QueryBudget as EngineQueryBudget, QueryExecutor, parse_compact_tem, parse_temql, plan_query,
};
use temnion_storage::Store;

/// Magic header for Temnion Flight packets: b"FLGT".
pub const FLIGHT_MAGIC: [u8; 4] = *b"FLGT";

/// Protocol version for Temnion Flight.
pub const FLIGHT_VERSION: u16 = 1;

/// Maximum payload size (32 MB) to prevent DoS memory exhaustion.
pub const MAX_FLIGHT_PAYLOAD: usize = 32 * 1024 * 1024;

/// Errors occurring during Arrow Flight operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlightError {
    Unauthenticated,
    InvalidToken,
    InvalidDescriptor(String),
    InvalidTicket(String),
    CorruptedData,
    ExecutionError(String),
    PayloadTooLarge(usize),
    ActionNotImplemented(String),
    IncompleteMessage,
}

impl fmt::Display for FlightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthenticated => {
                write!(f, "Flight request unauthenticated; handshake required")
            }
            Self::InvalidToken => write!(f, "Invalid or expired Flight session token"),
            Self::InvalidDescriptor(msg) => write!(f, "Invalid Flight descriptor: {msg}"),
            Self::InvalidTicket(msg) => write!(f, "Invalid Flight ticket: {msg}"),
            Self::CorruptedData => write!(f, "Flight payload checksum verification failed"),
            Self::ExecutionError(msg) => write!(f, "Flight query execution failed: {msg}"),
            Self::PayloadTooLarge(size) => {
                write!(
                    f,
                    "Flight payload {size} exceeds maximum {MAX_FLIGHT_PAYLOAD}"
                )
            }
            Self::ActionNotImplemented(act) => write!(f, "Flight action '{act}' not implemented"),
            Self::IncompleteMessage => write!(f, "Incomplete or truncated Flight message"),
        }
    }
}

impl std::error::Error for FlightError {}

// ---------------------------------------------------------------------------
// Flight Core Data Structures
// ---------------------------------------------------------------------------

/// Flight descriptor specifying a dataset or query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlightDescriptor {
    /// Command-based descriptor (e.g. TemQL or compact tn: query string).
    Cmd(String),
    /// Path-based descriptor (e.g. ["temnion", "entities"]).
    Path(Vec<String>),
    /// Unspecified descriptor.
    None,
}

impl FlightDescriptor {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::Cmd(cmd) => {
                buf.push(1);
                buf.extend_from_slice(&(cmd.len() as u32).to_le_bytes());
                buf.extend_from_slice(cmd.as_bytes());
            }
            Self::Path(paths) => {
                buf.push(2);
                buf.extend_from_slice(&(paths.len() as u32).to_le_bytes());
                for p in paths {
                    buf.extend_from_slice(&(p.len() as u32).to_le_bytes());
                    buf.extend_from_slice(p.as_bytes());
                }
            }
            Self::None => {
                buf.push(0);
            }
        }
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FlightError> {
        if bytes.is_empty() {
            return Ok(Self::None);
        }
        match bytes[0] {
            0 => Ok(Self::None),
            1 => {
                if bytes.len() < 5 {
                    return Err(FlightError::InvalidDescriptor("Truncated Cmd".to_string()));
                }
                let len = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
                if bytes.len() < 5 + len {
                    return Err(FlightError::InvalidDescriptor(
                        "Truncated Cmd body".to_string(),
                    ));
                }
                let cmd = String::from_utf8(bytes[5..5 + len].to_vec())
                    .map_err(|e| FlightError::InvalidDescriptor(e.to_string()))?;
                Ok(Self::Cmd(cmd))
            }
            2 => {
                if bytes.len() < 5 {
                    return Err(FlightError::InvalidDescriptor(
                        "Truncated Path count".to_string(),
                    ));
                }
                let count = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
                let mut paths = Vec::with_capacity(count);
                let mut offset = 5;
                for _ in 0..count {
                    if bytes.len() < offset + 4 {
                        return Err(FlightError::InvalidDescriptor(
                            "Truncated path element length".to_string(),
                        ));
                    }
                    let seg_len = u32::from_le_bytes([
                        bytes[offset],
                        bytes[offset + 1],
                        bytes[offset + 2],
                        bytes[offset + 3],
                    ]) as usize;
                    offset += 4;
                    if bytes.len() < offset + seg_len {
                        return Err(FlightError::InvalidDescriptor(
                            "Truncated path element body".to_string(),
                        ));
                    }
                    let s = String::from_utf8(bytes[offset..offset + seg_len].to_vec())
                        .map_err(|e| FlightError::InvalidDescriptor(e.to_string()))?;
                    offset += seg_len;
                    paths.push(s);
                }
                Ok(Self::Path(paths))
            }
            _ => Err(FlightError::InvalidDescriptor(
                "Unknown descriptor tag".to_string(),
            )),
        }
    }
}

/// Opaque authorization ticket for stream retrieval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub ticket: Vec<u8>,
}

impl Ticket {
    pub fn new(query_str: &str, max_rows: u32) -> Self {
        let mut buf = Vec::new();
        buf.extend_from_slice(&max_rows.to_le_bytes());
        buf.extend_from_slice(query_str.as_bytes());
        Self { ticket: buf }
    }

    pub fn parse(&self) -> Result<(String, u32), FlightError> {
        if self.ticket.len() < 4 {
            return Err(FlightError::InvalidTicket("Truncated ticket".to_string()));
        }
        let max_rows = u32::from_le_bytes([
            self.ticket[0],
            self.ticket[1],
            self.ticket[2],
            self.ticket[3],
        ]);
        let query_str = String::from_utf8(self.ticket[4..].to_vec())
            .map_err(|e| FlightError::InvalidTicket(e.to_string()))?;
        Ok((query_str, max_rows))
    }
}

/// Flight endpoint specifying where a ticket can be redeemed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlightEndpoint {
    pub ticket: Ticket,
    pub locations: Vec<String>,
}

/// Metadata describing a query execution and result endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlightInfo {
    pub schema: Vec<u8>,
    pub descriptor: FlightDescriptor,
    pub endpoints: Vec<FlightEndpoint>,
    pub total_records: i64,
    pub total_bytes: i64,
}

/// Stream packet containing Arrow columnar vectors.
#[derive(Debug, Clone, PartialEq)]
pub struct FlightData {
    pub descriptor: Option<FlightDescriptor>,
    pub data_header: Vec<u8>,
    pub data_body: Vec<u8>,
}

impl FlightData {
    pub fn new(
        descriptor: Option<FlightDescriptor>,
        data_header: Vec<u8>,
        data_body: Vec<u8>,
    ) -> Self {
        Self {
            descriptor,
            data_header,
            data_body,
        }
    }

    /// Serializes FlightData into framed binary format with CRC32C checksum.
    pub fn encode(&self) -> Result<Vec<u8>, FlightError> {
        let desc_bytes = self
            .descriptor
            .as_ref()
            .map(|d| d.encode())
            .unwrap_or_default();
        let total_size = 4
            + 2
            + 4
            + desc_bytes.len()
            + 4
            + self.data_header.len()
            + 4
            + self.data_body.len()
            + 4;
        if total_size > MAX_FLIGHT_PAYLOAD {
            return Err(FlightError::PayloadTooLarge(total_size));
        }

        let mut buf = Vec::with_capacity(total_size);
        buf.extend_from_slice(&FLIGHT_MAGIC);
        buf.extend_from_slice(&FLIGHT_VERSION.to_le_bytes());

        buf.extend_from_slice(&(desc_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(&desc_bytes);

        buf.extend_from_slice(&(self.data_header.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.data_header);

        buf.extend_from_slice(&(self.data_body.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.data_body);

        let mut hasher = Hasher::new();
        hasher.update(&buf);
        let checksum = hasher.finalize();
        buf.extend_from_slice(&checksum.to_le_bytes());

        Ok(buf)
    }

    /// Decodes and validates FlightData from binary bytes.
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize), FlightError> {
        if bytes.len() < 18 {
            return Err(FlightError::IncompleteMessage);
        }

        if bytes[0..4] != FLIGHT_MAGIC {
            return Err(FlightError::CorruptedData);
        }

        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != FLIGHT_VERSION {
            return Err(FlightError::CorruptedData);
        }

        let desc_len = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]) as usize;
        let mut offset = 10;
        if bytes.len() < offset + desc_len + 4 {
            return Err(FlightError::IncompleteMessage);
        }

        let descriptor = if desc_len > 0 {
            Some(FlightDescriptor::decode(&bytes[offset..offset + desc_len])?)
        } else {
            None
        };
        offset += desc_len;

        let header_len = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;
        if bytes.len() < offset + header_len + 4 {
            return Err(FlightError::IncompleteMessage);
        }
        let data_header = bytes[offset..offset + header_len].to_vec();
        offset += header_len;

        let body_len = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;
        if bytes.len() < offset + body_len + 4 {
            return Err(FlightError::IncompleteMessage);
        }
        let data_body = bytes[offset..offset + body_len].to_vec();
        offset += body_len;

        let expected_checksum = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]);
        offset += 4;

        let mut hasher = Hasher::new();
        hasher.update(&bytes[..offset - 4]);
        let computed = hasher.finalize();

        if computed != expected_checksum {
            return Err(FlightError::CorruptedData);
        }

        Ok((
            Self {
                descriptor,
                data_header,
                data_body,
            },
            offset,
        ))
    }
}

/// Authentication handshake request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlightHandshakeRequest {
    pub auth_token: String,
}

/// Authentication handshake response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlightHandshakeResponse {
    pub session_token: String,
    pub authenticated: bool,
}

// ---------------------------------------------------------------------------
// Flight Service Server
// ---------------------------------------------------------------------------

/// Server providing Arrow Flight RPC services against local Temnion storage.
pub struct FlightService {
    store: Store,
    server_location: String,
    shared_secret: String,
    active_sessions: std::collections::HashSet<String>,
}

impl FlightService {
    pub fn new(
        store: Store,
        server_location: impl Into<String>,
        shared_secret: impl Into<String>,
    ) -> Self {
        Self {
            store,
            server_location: server_location.into(),
            shared_secret: shared_secret.into(),
            active_sessions: std::collections::HashSet::new(),
        }
    }

    /// Authenticates client credentials and returns a session token.
    pub fn handshake(
        &mut self,
        req: &FlightHandshakeRequest,
    ) -> Result<FlightHandshakeResponse, FlightError> {
        if req.auth_token == self.shared_secret {
            let session_token = format!("flight-sess-{:x}", req.auth_token.len() * 31 + 42);
            self.active_sessions.insert(session_token.clone());
            Ok(FlightHandshakeResponse {
                session_token,
                authenticated: true,
            })
        } else {
            Err(FlightError::Unauthenticated)
        }
    }

    /// Verifies that a request carries a valid session token.
    pub fn verify_session(&self, token: &str) -> Result<(), FlightError> {
        if self.active_sessions.contains(token) {
            Ok(())
        } else {
            Err(FlightError::InvalidToken)
        }
    }

    /// Evaluates a query descriptor and produces `FlightInfo` metadata.
    pub fn get_flight_info(
        &mut self,
        session_token: &str,
        descriptor: &FlightDescriptor,
    ) -> Result<FlightInfo, FlightError> {
        self.verify_session(session_token)?;

        let query_str = match descriptor {
            FlightDescriptor::Cmd(cmd) => cmd.clone(),
            FlightDescriptor::Path(_paths) => "FROM temnion\nENTITY 0:1:1".to_string(),
            FlightDescriptor::None => {
                return Err(FlightError::InvalidDescriptor(
                    "Missing query command".to_string(),
                ));
            }
        };

        let ticket = Ticket::new(&query_str, 10_000);
        let endpoint = FlightEndpoint {
            ticket,
            locations: vec![self.server_location.clone()],
        };

        let schema_desc = b"entity_shard:u32,entity_slot:u32,entity_gen:u32,valid_time:u64,known_time:u64,seq:u64,schema:u32".to_vec();

        Ok(FlightInfo {
            schema: schema_desc,
            descriptor: descriptor.clone(),
            endpoints: vec![endpoint],
            total_records: -1, // Unknown prior to stream completion
            total_bytes: -1,
        })
    }

    /// Redeems a ticket and streams `FlightData` batches.
    pub fn do_get(
        &mut self,
        session_token: &str,
        ticket: &Ticket,
    ) -> Result<Vec<FlightData>, FlightError> {
        self.verify_session(session_token)?;

        let (query_str, max_rows) = ticket.parse()?;
        let logical = if query_str.trim().starts_with("tn:")
            || query_str.trim().starts_with('#')
            || query_str.trim().starts_with('$')
        {
            parse_compact_tem(&query_str).map_err(|e| FlightError::ExecutionError(e.to_string()))?
        } else {
            parse_temql(&query_str).map_err(|e| FlightError::ExecutionError(e.to_string()))?
        };

        let physical = plan_query(&logical);
        let budget = EngineQueryBudget {
            max_rows: Some(max_rows as usize),
            max_events_scanned: Some(100_000),
            max_bytes: Some(64 * 1024 * 1024),
        };

        let query_result = QueryExecutor::execute_storage_scan(&mut self.store, &physical, &budget)
            .map_err(|e| FlightError::ExecutionError(e.to_string()))?;

        let batch = ColumnarBatch::from_query_rows(&query_result.rows);

        // Serialize columnar batch into FlightData
        let mut header = Vec::new();
        header.extend_from_slice(&(batch.length as u32).to_le_bytes());
        header.extend_from_slice(&(query_result.events_scanned as u32).to_le_bytes());

        // Body: serialize vectors
        let mut body = Vec::new();
        for &shard in &batch.entity_shards {
            body.extend_from_slice(&shard.to_le_bytes());
        }
        for &slot in &batch.entity_slots {
            body.extend_from_slice(&slot.to_le_bytes());
        }
        for &generation in &batch.entity_generations {
            body.extend_from_slice(&generation.to_le_bytes());
        }
        for &vt in &batch.valid_timestamps {
            body.extend_from_slice(&vt.to_le_bytes());
        }
        for &kt in &batch.known_timestamps {
            body.extend_from_slice(&kt.to_le_bytes());
        }
        for &seq in &batch.sequences {
            body.extend_from_slice(&seq.to_le_bytes());
        }
        for &sc in &batch.schema_ids {
            body.extend_from_slice(&sc.to_le_bytes());
        }

        let flight_data = FlightData::new(Some(FlightDescriptor::Cmd(query_str)), header, body);
        Ok(vec![flight_data])
    }

    /// Executes arbitrary control plane actions (e.g. ping, status).
    pub fn do_action(
        &mut self,
        session_token: &str,
        action_type: &str,
        _body: &[u8],
    ) -> Result<Vec<u8>, FlightError> {
        self.verify_session(session_token)?;
        match action_type {
            "ping" => Ok(b"PONG".to_vec()),
            "status" => {
                let status = format!(
                    "{{\"status\":\"online\",\"version\":{},\"location\":\"{}\"}}",
                    FLIGHT_VERSION, self.server_location
                );
                Ok(status.into_bytes())
            }
            other => Err(FlightError::ActionNotImplemented(other.to_string())),
        }
    }
}
