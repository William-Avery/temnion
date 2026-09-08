// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use std::fs;
use std::net::TcpStream;
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;
use std::time::Duration;

use temnion_core::{
    ClockId, EntityId, EventTimes, SchemaId, ShardId, SourceEpoch, SourceId, Timestamp,
};
use temnion_format::Limits;
use temnion_protocol::{
    HandshakeRequest, HandshakeResponse, QueryFormat, QueryRequest, TNP_VERSION, TnpChannel,
    TnpMessageType, TnpPacket,
};
use temnion_storage::{Store, WriteEvent};
use temniond::{DaemonConfig, DaemonServer};

static PORT_COUNTER: AtomicU16 = AtomicU16::new(19200);

fn next_test_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

struct TestEnv {
    dir: std::path::PathBuf,
    port: u16,
}

impl TestEnv {
    fn new(name: &str) -> Self {
        let port = next_test_port();
        let dir = std::env::temp_dir().join(format!("temniond-test-{name}-{port}"));
        if dir.exists() {
            let _ = fs::remove_dir_all(&dir);
        }
        fs::create_dir_all(&dir).unwrap();
        Self { dir, port }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn config_round_trip_serialization() {
    let config = DaemonConfig {
        data_dir: std::path::PathBuf::from("/var/lib/temnion"),
        server_id: "test-node-42".to_string(),
        tnp_bind: "0.0.0.0:9999".to_string(),
        mcp_enabled: false,
        maintenance_interval_secs: 120,
        source_id: 42,
        source_epoch: 7,
        ..DaemonConfig::default()
    };

    let toml = config.to_toml_string();
    assert!(toml.contains("server_id = \"test-node-42\""));
    assert!(toml.contains("tnp_bind = \"0.0.0.0:9999\""));
    assert!(toml.contains("mcp_enabled = false"));
    assert!(toml.contains("maintenance_interval_secs = 120"));

    let parsed = DaemonConfig::parse_toml(&toml).expect("Parsing config failed");
    assert_eq!(config, parsed);
}

#[test]
fn daemon_tcp_lifecycle_and_query_execution() {
    let env = TestEnv::new("lifecycle");
    let config = DaemonConfig {
        data_dir: env.dir.clone(),
        server_id: "daemon-test-server".to_string(),
        tnp_bind: format!("127.0.0.1:{}", env.port),
        maintenance_interval_secs: 1,
        ..DaemonConfig::default()
    };

    // Pre-populate store with test events
    {
        let mut store =
            Store::create(&env.dir, SourceId(1), SourceEpoch(1), Limits::default()).unwrap();

        let entity = EntityId {
            shard: ShardId(0),
            slot: 1,
            generation: 1,
        };
        let event = WriteEvent {
            entity,
            schema: SchemaId(1),
            times: EventTimes {
                valid: Timestamp::new(ClockId(1), 100),
                observed: None,
                known: Timestamp::new(ClockId(1), 100),
            },
            payload: b"daemon_test_payload".to_vec(),
            causes: Vec::new(),
        };
        store.append(vec![event]).unwrap();
    }

    // Start daemon
    let mut server = DaemonServer::new(config.clone()).expect("Failed to create DaemonServer");
    server.start().expect("Failed to start DaemonServer");
    assert!(server.is_running());

    // Allow thread to bind
    thread::sleep(Duration::from_millis(100));

    // Connect client
    let stream = TcpStream::connect(&config.tnp_bind).expect("Failed to connect to daemon TCP");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let reader = stream.try_clone().unwrap();
    let writer = stream;
    let mut channel = TnpChannel::new(reader, writer);

    // 1. Handshake
    let hs_req = HandshakeRequest {
        client_version: TNP_VERSION,
        client_id: "integration-tester".to_string(),
        capability_flags: 0x07,
    };
    channel
        .send(&TnpPacket::new(
            TnpMessageType::HandshakeRequest,
            1,
            hs_req.encode(),
        ))
        .unwrap();

    let hs_resp_pkt = channel.recv().unwrap();
    assert_eq!(hs_resp_pkt.message_type, TnpMessageType::HandshakeResponse);
    let hs_resp = HandshakeResponse::decode(&hs_resp_pkt.payload).unwrap();
    assert!(hs_resp.success);
    assert_eq!(hs_resp.server_id, "daemon-test-server");

    // 2. Ping
    channel
        .send(&TnpPacket::new(TnpMessageType::Ping, 2, vec![]))
        .unwrap();
    let pong_pkt = channel.recv().unwrap();
    assert_eq!(pong_pkt.message_type, TnpMessageType::Pong);

    // 3. Describe
    channel
        .send(&TnpPacket::new(TnpMessageType::DescribeRequest, 3, vec![]))
        .unwrap();
    let desc_pkt = channel.recv().unwrap();
    assert_eq!(desc_pkt.message_type, TnpMessageType::DescribeResponse);
    let desc_str = String::from_utf8(desc_pkt.payload).unwrap();
    assert!(desc_str.contains("daemon-test-server"));

    // 4. Query via SQL
    let q_req = QueryRequest {
        format: QueryFormat::Sql,
        query_str: "SELECT * FROM events WHERE entity = '#0:1:1'".to_string(),
        max_rows: 10,
    };
    channel
        .send(&TnpPacket::new(
            TnpMessageType::QueryRequest,
            4,
            q_req.encode(),
        ))
        .unwrap();

    let q_resp_pkt = channel.recv().unwrap();
    assert_eq!(q_resp_pkt.message_type, TnpMessageType::QueryResponse);

    // Stream records
    let rec_pkt = channel.recv().unwrap();
    assert_eq!(rec_pkt.message_type, TnpMessageType::StreamRecord);
    let rec_str = String::from_utf8(rec_pkt.payload).unwrap();
    assert!(rec_str.contains("0:1:1"));

    // Stream end
    let end_pkt = channel.recv().unwrap();
    assert_eq!(end_pkt.message_type, TnpMessageType::StreamEnd);

    // 5. Graceful shutdown
    server.shutdown();
    assert!(!server.is_running());
}

#[test]
fn connector_handshake_and_ping_against_daemon() {
    use temnion_adapter::connector::{ActiveConnection, ConnectionConfig};

    let env = TestEnv::new("connector-ping");
    let config = DaemonConfig {
        data_dir: env.dir.clone(),
        server_id: "daemon-connector-target".to_string(),
        tnp_bind: format!("127.0.0.1:{}", env.port),
        ..DaemonConfig::default()
    };

    let mut server = DaemonServer::new(config).expect("DaemonServer::new failed");
    server.start().expect("Failed to start daemon server");

    // Connect via Connector
    let client_cfg = ConnectionConfig {
        host: "127.0.0.1".to_string(),
        port: env.port,
        database: "temnion_default".to_string(),
        username: "admin".to_string(),
        auth_token: None,
    };

    let mut conn = ActiveConnection::connect(client_cfg).expect("Connector failed to connect");
    assert_eq!(conn.server_id(), "daemon-connector-target");
    assert_eq!(conn.negotiated_version(), TNP_VERSION);

    // Ping
    let latency = conn.ping().expect("Ping failed");
    assert!(latency.as_millis() < 1000);

    server.shutdown();
}
