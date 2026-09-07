// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use temnion_core::{
    ClockId, EntityId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_format::Limits;
use temnion_protocol::{
    ColumnarBatch, HandshakeRequest, HandshakeResponse, QueryFormat, QueryRequest,
    TEMNION_ERR_INVALID_ARGUMENT, TEMNION_ERR_NOT_FOUND, TEMNION_SUCCESS, TNP_VERSION, TnpChannel,
    TnpError, TnpMessageType, TnpPacket, TnpServer, temnion_c_query_execute, temnion_c_result_free,
    temnion_c_result_row_count, temnion_c_store_close, temnion_c_store_open,
};
use temnion_query::QueryRow;
use temnion_storage::{Store, WriteEvent};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "temnion-protocol-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// ---------------------------------------------------------------------------
// In-Memory Duplex Pipe for Testing Local IPC
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct PipeBuffer {
    data: Arc<Mutex<VecDeque<u8>>>,
    notify: Arc<Condvar>,
    closed: Arc<Mutex<bool>>,
}

impl PipeBuffer {
    fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(Condvar::new()),
            closed: Arc::new(Mutex::new(false)),
        }
    }

    fn close(&self) {
        *self.closed.lock().unwrap() = true;
        self.notify.notify_all();
    }
}

impl Read for PipeBuffer {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut queue = self.data.lock().unwrap();
        loop {
            if !queue.is_empty() {
                let n = std::cmp::min(buf.len(), queue.len());
                for (i, byte) in queue.drain(..n).enumerate() {
                    buf[i] = byte;
                }
                return Ok(n);
            }
            if *self.closed.lock().unwrap() {
                return Ok(0);
            }
            queue = self.notify.wait(queue).unwrap();
        }
    }
}

impl Write for PipeBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut queue = self.data.lock().unwrap();
        queue.extend(buf);
        self.notify.notify_all();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn packet_encode_decode_roundtrip() {
    let payload = b"TEMNION_PROTOCOL_TEST_PAYLOAD".to_vec();
    let packet = TnpPacket::new(TnpMessageType::QueryRequest, 42, payload.clone());

    let encoded = packet.encode().expect("encoding packet should succeed");
    let (decoded, consumed) = TnpPacket::decode(&encoded).expect("decoding packet should succeed");

    assert_eq!(consumed, encoded.len());
    assert_eq!(decoded.version, TNP_VERSION);
    assert_eq!(decoded.message_type, TnpMessageType::QueryRequest);
    assert_eq!(decoded.stream_id, 42);
    assert_eq!(decoded.payload, payload);
}

#[test]
fn packet_corruptions_and_validation() {
    let packet = TnpPacket::new(TnpMessageType::Ping, 1, b"ping".to_vec());
    let encoded = packet.encode().unwrap();

    // 1. Incomplete packet (short)
    assert_eq!(
        TnpPacket::decode(&encoded[..10]),
        Err(TnpError::IncompletePacket)
    );

    // 2. Corrupted magic
    let mut bad_magic = encoded.clone();
    bad_magic[0] = b'X';
    assert_eq!(TnpPacket::decode(&bad_magic), Err(TnpError::InvalidMagic));

    // 3. Checksum corruption
    let mut corrupted = encoded.clone();
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0xFF;
    assert_eq!(
        TnpPacket::decode(&corrupted),
        Err(TnpError::ChecksumMismatch)
    );

    // 4. Unsupported version
    let mut bad_ver = encoded.clone();
    bad_ver[4] = 99;
    // Recompute checksum for the modified payload to test version validation specifically
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&bad_ver[..bad_ver.len() - 4]);
    let ck = hasher.finalize();
    let len = bad_ver.len();
    bad_ver[len - 4..].copy_from_slice(&ck.to_le_bytes());
    assert!(matches!(
        TnpPacket::decode(&bad_ver),
        Err(TnpError::UnsupportedVersion { .. })
    ));
}

#[test]
fn handshake_and_query_codecs() {
    // HandshakeRequest
    let req = HandshakeRequest {
        client_version: TNP_VERSION,
        client_id: "test-client-1".to_string(),
        capability_flags: 0x07,
    };
    let req_enc = req.encode();
    let req_dec = HandshakeRequest::decode(&req_enc).unwrap();
    assert_eq!(req, req_dec);

    // HandshakeResponse
    let resp = HandshakeResponse {
        success: true,
        negotiated_version: TNP_VERSION,
        server_id: "temnion-node-alpha".to_string(),
        capability_flags: 0x07,
    };
    let resp_enc = resp.encode();
    let resp_dec = HandshakeResponse::decode(&resp_enc).unwrap();
    assert_eq!(resp, resp_dec);

    // QueryRequest - TemQL
    let q_req = QueryRequest {
        format: QueryFormat::Temql,
        query_str: "FROM temnion\nENTITY #0:1:1\nTIME valid 10..20".to_string(),
        max_rows: 50,
    };
    let q_enc = q_req.encode();
    let q_dec = QueryRequest::decode(&q_enc).unwrap();
    assert_eq!(q_req, q_dec);

    // QueryRequest - Compact Tem
    let c_req = QueryRequest {
        format: QueryFormat::CompactTem,
        query_str: "tn:#0:1:1@v10..20".to_string(),
        max_rows: 100,
    };
    let c_enc = c_req.encode();
    let c_dec = QueryRequest::decode(&c_enc).unwrap();
    assert_eq!(c_req, c_dec);
}

#[test]
fn local_ipc_channel_and_server_streaming_workflow() {
    let dir = TempDir::new("ipc-server");
    let mut store =
        Store::create(&dir.path, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();

    let entity = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 1,
    };

    let mut records = Vec::new();
    for i in 0..5 {
        records.push(WriteEvent {
            entity,
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 10 + i),
                observed: None,
                known: Timestamp::new(ClockId(1), 10 + i),
            },
            schema: SchemaId(1),
            payload: vec![i as u8, 0xBB],
            causes: Vec::new(),
        });
    }
    store.append(records).unwrap();

    let c2s = PipeBuffer::new();
    let s2c = PipeBuffer::new();

    let mut client_channel = TnpChannel::new(s2c.clone(), c2s.clone());
    let mut server_channel = TnpChannel::new(c2s.clone(), s2c);

    let mut server = TnpServer::new(store, "temnion-ipc-node");

    // Run server in background thread
    let server_handle = thread::spawn(move || server.handle_connection(&mut server_channel));

    // 1. Handshake
    let hs_req = HandshakeRequest {
        client_version: TNP_VERSION,
        client_id: "ipc-client".to_string(),
        capability_flags: 0x07,
    };
    client_channel
        .send(&TnpPacket::new(
            TnpMessageType::HandshakeRequest,
            1,
            hs_req.encode(),
        ))
        .unwrap();

    let hs_resp_pkt = client_channel.recv().unwrap();
    assert_eq!(hs_resp_pkt.message_type, TnpMessageType::HandshakeResponse);
    let hs_resp = HandshakeResponse::decode(&hs_resp_pkt.payload).unwrap();
    assert!(hs_resp.success);
    assert_eq!(hs_resp.negotiated_version, TNP_VERSION);
    assert_eq!(hs_resp.server_id, "temnion-ipc-node");

    // 2. Ping / Pong
    client_channel
        .send(&TnpPacket::new(TnpMessageType::Ping, 2, vec![]))
        .unwrap();
    let pong_pkt = client_channel.recv().unwrap();
    assert_eq!(pong_pkt.message_type, TnpMessageType::Pong);

    // 3. Describe
    client_channel
        .send(&TnpPacket::new(TnpMessageType::DescribeRequest, 3, vec![]))
        .unwrap();
    let desc_pkt = client_channel.recv().unwrap();
    assert_eq!(desc_pkt.message_type, TnpMessageType::DescribeResponse);
    let desc_str = String::from_utf8(desc_pkt.payload).unwrap();
    assert!(desc_str.contains("temnion-ipc-node"));
    assert!(desc_str.contains("arrow-columnar"));

    // 4. Query & Stream
    let q_req = QueryRequest {
        format: QueryFormat::Temql,
        query_str: "FROM temnion\nENTITY #0:1:1\nTIME valid 10..14".to_string(),
        max_rows: 10,
    };
    client_channel
        .send(&TnpPacket::new(
            TnpMessageType::QueryRequest,
            4,
            q_req.encode(),
        ))
        .unwrap();

    let q_resp_pkt = client_channel.recv().unwrap();
    assert_eq!(q_resp_pkt.message_type, TnpMessageType::QueryResponse);

    // Collect stream records until StreamEnd
    let mut records_received = 0;
    loop {
        let pkt = client_channel.recv().unwrap();
        match pkt.message_type {
            TnpMessageType::StreamRecord => {
                records_received += 1;
                let s = String::from_utf8(pkt.payload).unwrap();
                assert!(s.starts_with("0:1:1|"));
            }
            TnpMessageType::StreamEnd => {
                let total = u32::from_le_bytes(pkt.payload.try_into().unwrap());
                assert_eq!(total, 4); // ticks 10, 11, 12, 13
                break;
            }
            other => panic!("Unexpected message type: {other:?}"),
        }
    }
    assert_eq!(records_received, 4);

    // Close client side to let server terminate
    c2s.close();
    server_handle.join().unwrap().unwrap();
}

#[test]
fn columnar_batch_arrow_roundtrip() {
    let mut rows = Vec::new();
    for i in 0..5 {
        rows.push(QueryRow {
            entity: EntityId {
                shard: ShardId(i as u32),
                slot: 100 + i as u32,
                generation: 1,
            },
            schema: SchemaId(2),
            valid_time: Timestamp::new(ClockId(1), 1000 + i as u64),
            known_time: Timestamp::new(ClockId(1), 2000 + i as u64),
            sequence: i as u64,
            fields: Default::default(),
        });
    }

    let batch = ColumnarBatch::from_query_rows(&rows);
    assert_eq!(batch.length, 5);
    assert_eq!(batch.entity_shards, vec![0, 1, 2, 3, 4]);
    assert_eq!(batch.entity_slots, vec![100, 101, 102, 103, 104]);
    assert_eq!(batch.valid_timestamps, vec![1000, 1001, 1002, 1003, 1004]);
    assert_eq!(batch.sequences, vec![0, 1, 2, 3, 4]);

    let roundtrip_rows = batch.to_query_rows();
    assert_eq!(roundtrip_rows.len(), 5);
    for (orig, rt) in rows.iter().zip(roundtrip_rows.iter()) {
        assert_eq!(orig.entity, rt.entity);
        assert_eq!(orig.valid_time, rt.valid_time);
        assert_eq!(orig.known_time, rt.known_time);
        assert_eq!(orig.sequence, rt.sequence);
        assert_eq!(orig.schema, rt.schema);
    }
}

#[test]
fn c_abi_store_and_query_flow() {
    let dir = TempDir::new("c-abi-flow");
    let store_path = dir.path.to_str().unwrap();

    // Create database with some records first
    {
        let mut store =
            Store::create(&dir.path, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();
        let entity = EntityId {
            shard: ShardId(0),
            slot: 1,
            generation: 1,
        };
        store
            .append(vec![
                WriteEvent {
                    entity,
                    times: EventTimes {
                        valid: Timestamp::new(ClockId(1), 10),
                        observed: None,
                        known: Timestamp::new(ClockId(1), 10),
                    },
                    schema: SchemaId(1),
                    payload: vec![1, 2, 3],
                    causes: Vec::new(),
                },
                WriteEvent {
                    entity,
                    times: EventTimes {
                        valid: Timestamp::new(ClockId(1), 20),
                        observed: None,
                        known: Timestamp::new(ClockId(1), 20),
                    },
                    schema: SchemaId(1),
                    payload: vec![4, 5, 6],
                    causes: Vec::new(),
                },
            ])
            .unwrap();
    }

    let mut store_handle = 0u64;
    let status = temnion_c_store_open(store_path, &mut store_handle);
    assert_eq!(status, TEMNION_SUCCESS);
    assert_ne!(store_handle, 0);

    // Execute query via TemQL
    let mut res_handle = 0u64;
    let q_status = temnion_c_query_execute(
        store_handle,
        "FROM temnion\nENTITY #0:1:1\nTIME valid 5..25",
        &mut res_handle,
    );
    assert_eq!(q_status, TEMNION_SUCCESS);
    assert_ne!(res_handle, 0);

    let row_count = temnion_c_result_row_count(res_handle);
    assert_eq!(row_count, 2);

    // Execute query via Compact Tem shorthand
    let mut res_compact_handle = 0u64;
    let qc_status =
        temnion_c_query_execute(store_handle, "tn:#0:1:1@v5..15", &mut res_compact_handle);
    assert_eq!(qc_status, TEMNION_SUCCESS);
    let row_compact_count = temnion_c_result_row_count(res_compact_handle);
    assert_eq!(row_compact_count, 1);

    // Free results
    assert_eq!(temnion_c_result_free(res_handle), TEMNION_SUCCESS);
    assert_eq!(temnion_c_result_free(res_compact_handle), TEMNION_SUCCESS);

    // Error handling checks
    let mut err_handle = 0u64;
    assert_eq!(
        temnion_c_query_execute(999999, "FROM temnion\nENTITY #0:1:1", &mut err_handle),
        TEMNION_ERR_NOT_FOUND
    );
    assert_eq!(
        temnion_c_query_execute(store_handle, "INVALID QUERY SYNTAX !!!", &mut err_handle),
        TEMNION_ERR_INVALID_ARGUMENT
    );

    // Close store
    assert_eq!(temnion_c_store_close(store_handle), TEMNION_SUCCESS);
    assert_eq!(temnion_c_store_close(store_handle), TEMNION_ERR_NOT_FOUND);
}
