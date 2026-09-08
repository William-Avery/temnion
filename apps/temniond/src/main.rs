// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! `temniond` - Standalone Background Service Daemon for Temnion.
//!
//! Provides headless, long-running multi-protocol server execution for:
//! - TNP (Temnion Network Protocol) binary TCP connections
//! - Arrow Flight remote analytical transport
//! - Model Context Protocol (MCP) server
//! - Background task DAG scheduling, storage tiering, and periodic checkpointing

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use temnion_protocol::{
    HandshakeRequest, HandshakeResponse, TNP_VERSION, TnpChannel, TnpMessageType, TnpPacket,
};
use temniond::{DaemonConfig, DaemonServer};

const HELP: &str = "\
temniond - Temnion Standalone Background Service Daemon

Usage:
  temniond run [--config <path>] [--bind <addr:port>] [--data-dir <path>]
  temniond init [--config <path>] [--data-dir <path>]
  temniond status [--config <path>] [--target <addr:port>]
  temniond version
  temniond help

Commands:
  run      Start the server daemon in foreground or system service mode
  init     Initialize database directory and write default configuration template
  status   Probe running daemon status via TNP protocol health check
  version  Show daemon version and architecture metadata
  help     Show this help message

Options:
  --config <path>      Path to configuration file (default: temnion.toml)
  --bind <addr:port>   Override TNP bind address (default: 127.0.0.1:9180)
  --data-dir <path>    Override database store directory (default: ./data/temnion_db)
  --target <addr:port> Target server address for status probe
";

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let command = args.first().and_then(|a| a.to_str()).unwrap_or("help");

    let result = match command {
        "help" | "--help" | "-h" => {
            println!("{HELP}");
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!(
                "temniond {} ({}-{}) [Rust 2024, #![forbid(unsafe_code)]]",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            Ok(())
        }
        "init" => cmd_init(&args[1..]),
        "status" => cmd_status(&args[1..]),
        "run" => cmd_run(&args[1..]),
        other => {
            eprintln!("temniond: unknown command '{other}'; use 'temniond help'");
            Err("Unknown command".into())
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("temniond error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn parse_option(args: &[OsString], flag: &str) -> Option<String> {
    for (i, arg) in args.iter().enumerate() {
        if let Some(s) = arg.to_str() {
            if s == flag {
                return args.get(i + 1).and_then(|v| v.to_str()).map(str::to_string);
            }
            if let Some(val) = s.strip_prefix(&format!("{flag}=")) {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn cmd_init(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    let config_path = parse_option(args, "--config")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("temnion.toml"));

    let data_dir = parse_option(args, "--data-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data/temnion_db"));

    println!(
        "Initializing Temnion database directory: {}",
        data_dir.display()
    );
    fs::create_dir_all(&data_dir)?;

    let config = DaemonConfig {
        data_dir,
        ..DaemonConfig::default()
    };

    println!("Writing configuration template: {}", config_path.display());
    config.save(&config_path)?;

    println!("Temnion daemon initialized successfully.");
    println!(
        "Start with: temniond run --config {}",
        config_path.display()
    );
    Ok(())
}

fn cmd_status(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    let config_path = parse_option(args, "--config").map(PathBuf::from);
    let target = parse_option(args, "--target").unwrap_or_else(|| {
        if let Some(cp) = &config_path {
            if let Ok(cfg) = DaemonConfig::load(cp) {
                return cfg.tnp_bind;
            }
        }
        "127.0.0.1:9180".to_string()
    });

    println!("Probing Temnion daemon at {} via TNP...", target);
    let stream = TcpStream::connect(&target)
        .map_err(|e| format!("Could not connect to {target}: {e}. Is temniond running?"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let reader = stream.try_clone()?;
    let writer = stream;
    let mut channel = TnpChannel::new(reader, writer);

    // 1. Handshake
    let req = HandshakeRequest {
        client_version: TNP_VERSION,
        client_id: "temniond-probe".to_string(),
        capability_flags: 0x07,
    };
    channel.send(&TnpPacket::new(
        TnpMessageType::HandshakeRequest,
        1,
        req.encode(),
    ))?;

    let resp_pkt = channel.recv()?;
    if resp_pkt.message_type != TnpMessageType::HandshakeResponse {
        return Err(format!("Unexpected handshake response: {:?}", resp_pkt.message_type).into());
    }
    let resp = HandshakeResponse::decode(&resp_pkt.payload)?;
    if !resp.success {
        return Err("Handshake rejected by server".into());
    }

    // 2. Ping
    channel.send(&TnpPacket::new(TnpMessageType::Ping, 2, vec![]))?;
    let pong_pkt = channel.recv()?;
    if pong_pkt.message_type != TnpMessageType::Pong {
        return Err("Ping failed: expected Pong".into());
    }

    // 3. Describe
    channel.send(&TnpPacket::new(TnpMessageType::DescribeRequest, 3, vec![]))?;
    let desc_pkt = channel.recv()?;
    let desc_str = String::from_utf8_lossy(&desc_pkt.payload);

    println!("Daemon Status: HEALTHY");
    println!("  Server ID:           {}", resp.server_id);
    println!("  Negotiated Version:  v{}", resp.negotiated_version);
    println!("  Capability Flags:    0x{:02x}", resp.capability_flags);
    println!("  Server Metadata:     {}", desc_str);

    Ok(())
}

fn cmd_run(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    let config_path = parse_option(args, "--config")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("temnion.toml"));

    let mut config = if config_path.exists() {
        println!("Loading configuration from {}", config_path.display());
        DaemonConfig::load(&config_path)?
    } else {
        println!(
            "Configuration file {} not found; using defaults",
            config_path.display()
        );
        DaemonConfig::default()
    };

    if let Some(bind) = parse_option(args, "--bind") {
        config.tnp_bind = bind;
    }
    if let Some(data) = parse_option(args, "--data-dir") {
        config.data_dir = PathBuf::from(data);
    }

    println!("============================================================");
    println!(
        "Temnion Standalone Daemon (temniond v{})",
        env!("CARGO_PKG_VERSION")
    );
    println!("============================================================");
    println!("  Database Directory:      {}", config.data_dir.display());
    println!("  Server Identifier:       {}", config.server_id);
    println!("  TNP Binary Protocol:     tcp://{}", config.tnp_bind);
    println!("  Arrow Flight Service:    tcp://{}", config.flight_bind);
    println!("  MCP Protocol Enabled:    {}", config.mcp_enabled);
    println!(
        "  Maintenance Interval:    {}s",
        config.maintenance_interval_secs
    );
    println!("============================================================");

    let mut server = DaemonServer::new(config)?;
    server.start()?;
    println!("temniond successfully started. Listening for client connections.");

    // In foreground mode, sleep and wait
    println!("Press Ctrl+C to stop the daemon.");
    while server.is_running() {
        thread::sleep(Duration::from_millis(500));
    }

    server.shutdown();
    println!("temniond shut down cleanly.");
    Ok(())
}
