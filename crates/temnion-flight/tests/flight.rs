// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

use temnion_core::{
    ClockId, EntityId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_flight::{
    FlightData, FlightDescriptor, FlightError, FlightHandshakeRequest, FlightService, Ticket,
};
use temnion_format::Limits;
use temnion_storage::{Store, WriteEvent};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("temnion-flight-test-{name}-{}", std::process::id()));
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

#[test]
fn flight_descriptors_and_tickets_roundtrip() {
    // 1. None
    let desc_none = FlightDescriptor::None;
    let enc_none = desc_none.encode();
    let dec_none = FlightDescriptor::decode(&enc_none).unwrap();
    assert_eq!(desc_none, dec_none);

    // 2. Cmd
    let desc_cmd = FlightDescriptor::Cmd("FROM temnion\nENTITY 0:1:1".to_string());
    let enc_cmd = desc_cmd.encode();
    let dec_cmd = FlightDescriptor::decode(&enc_cmd).unwrap();
    assert_eq!(desc_cmd, dec_cmd);

    // 3. Path
    let desc_path = FlightDescriptor::Path(vec!["temnion".to_string(), "logs".to_string()]);
    let enc_path = desc_path.encode();
    let dec_path = FlightDescriptor::decode(&enc_path).unwrap();
    assert_eq!(desc_path, dec_path);

    // 4. Ticket
    let ticket = Ticket::new("FROM temnion\nENTITY 0:1:1", 500);
    let (q, limit) = ticket.parse().unwrap();
    assert_eq!(q, "FROM temnion\nENTITY 0:1:1");
    assert_eq!(limit, 500);
}

#[test]
fn flight_data_framing_and_checksum_verification() {
    let data = FlightData::new(
        Some(FlightDescriptor::Cmd("SELECT health".to_string())),
        vec![1, 2, 3, 4],
        vec![10, 20, 30, 40, 50],
    );

    let encoded = data.encode().unwrap();
    let (decoded, consumed) = FlightData::decode(&encoded).unwrap();
    assert_eq!(consumed, encoded.len());
    assert_eq!(decoded.descriptor, data.descriptor);
    assert_eq!(decoded.data_header, data.data_header);
    assert_eq!(decoded.data_body, data.data_body);

    // Incomplete
    assert_eq!(
        FlightData::decode(&encoded[..10]),
        Err(FlightError::IncompleteMessage)
    );

    // Corrupted magic
    let mut bad_magic = encoded.clone();
    bad_magic[0] = b'X';
    assert_eq!(
        FlightData::decode(&bad_magic),
        Err(FlightError::CorruptedData)
    );

    // Corrupted checksum
    let mut bad_checksum = encoded.clone();
    let last = bad_checksum.len() - 1;
    bad_checksum[last] ^= 0xFF;
    assert_eq!(
        FlightData::decode(&bad_checksum),
        Err(FlightError::CorruptedData)
    );
}

#[test]
fn flight_service_authentication_and_actions() {
    let dir = TempDir::new("auth-actions");
    let store = Store::create(&dir.path, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();
    let mut service = FlightService::new(store, "grpc+tcp://127.0.0.1:50051", "secret-token-123");

    // Invalid authentication
    let bad_req = FlightHandshakeRequest {
        auth_token: "wrong-secret".to_string(),
    };
    assert_eq!(
        service.handshake(&bad_req),
        Err(FlightError::Unauthenticated)
    );

    // Valid authentication
    let good_req = FlightHandshakeRequest {
        auth_token: "secret-token-123".to_string(),
    };
    let resp = service.handshake(&good_req).unwrap();
    assert!(resp.authenticated);
    assert!(resp.session_token.starts_with("flight-sess-"));

    // Actions
    let ping_resp = service.do_action(&resp.session_token, "ping", &[]).unwrap();
    assert_eq!(ping_resp, b"PONG");

    let status_resp = service
        .do_action(&resp.session_token, "status", &[])
        .unwrap();
    let status_str = String::from_utf8(status_resp).unwrap();
    assert!(status_str.contains("online"));
    assert!(status_str.contains("grpc+tcp://127.0.0.1:50051"));

    // Unimplemented action
    assert!(matches!(
        service.do_action(&resp.session_token, "unknown_action", &[]),
        Err(FlightError::ActionNotImplemented(_))
    ));

    // Unauthenticated action call
    assert_eq!(
        service.do_action("invalid-sess", "ping", &[]),
        Err(FlightError::InvalidToken)
    );
}

#[test]
fn flight_service_get_flight_info_and_do_get_pipeline() {
    let dir = TempDir::new("flight-pipeline");
    let mut store =
        Store::create(&dir.path, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();

    let entity = EntityId {
        shard: ShardId(0),
        slot: 1,
        generation: 1,
    };

    let mut records = Vec::new();
    for i in 0..4 {
        records.push(WriteEvent {
            entity,
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 10 + i),
                observed: None,
                known: Timestamp::new(ClockId(1), 10 + i),
            },
            schema: SchemaId(1),
            payload: vec![i as u8, 0xFE],
            causes: Vec::new(),
        });
    }
    store.append(records).unwrap();

    let mut service = FlightService::new(store, "grpc+tcp://127.0.0.1:50051", "m20-secret");
    let hs = service
        .handshake(&FlightHandshakeRequest {
            auth_token: "m20-secret".to_string(),
        })
        .unwrap();

    // 1. get_flight_info
    let desc = FlightDescriptor::Cmd("FROM temnion\nENTITY 0:1:1\nTIME valid 10..13".to_string());
    let info = service.get_flight_info(&hs.session_token, &desc).unwrap();
    assert_eq!(info.descriptor, desc);
    assert_eq!(info.endpoints.len(), 1);
    assert_eq!(
        info.endpoints[0].locations,
        vec!["grpc+tcp://127.0.0.1:50051"]
    );

    // 2. do_get
    let batches = service
        .do_get(&hs.session_token, &info.endpoints[0].ticket)
        .unwrap();
    assert_eq!(batches.len(), 1);

    let batch_data = &batches[0];
    let row_count = u32::from_le_bytes([
        batch_data.data_header[0],
        batch_data.data_header[1],
        batch_data.data_header[2],
        batch_data.data_header[3],
    ]);
    assert_eq!(row_count, 3); // ticks 10, 11, 12 (13 exclusive)

    // Verify encode/decode roundtrip of the emitted FlightData batch
    let enc_flight = batch_data.encode().unwrap();
    let (dec_flight, len) = FlightData::decode(&enc_flight).unwrap();
    assert_eq!(len, enc_flight.len());
    assert_eq!(dec_flight.data_header, batch_data.data_header);

    // 3. do_get via SQL query
    let sql_ticket = Ticket::new(
        "SELECT * FROM temnion WHERE entity = '0:1:1' AND valid_time >= 10 AND valid_time < 13",
        100,
    );
    let sql_batches = service.do_get(&hs.session_token, &sql_ticket).unwrap();
    assert_eq!(sql_batches.len(), 1);
    let sql_row_count = u32::from_le_bytes([
        sql_batches[0].data_header[0],
        sql_batches[0].data_header[1],
        sql_batches[0].data_header[2],
        sql_batches[0].data_header[3],
    ]);
    assert_eq!(sql_row_count, 3);
}
