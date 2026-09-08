// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! Temnion database connector for external clients and consumers (Tzeentch).
//!
//! Provides connection parameter resolution (URI, environment variables, configuration files),
//! authentication negotiation, and live session management over TNP (Temnion Network Protocol).

use std::error::Error;
use std::fmt;
use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use temnion_protocol::{
    HandshakeRequest, HandshakeResponse, TNP_VERSION, TnpChannel, TnpError, TnpMessageType,
    TnpPacket,
};

/// Default TNP port for Temnion database daemon.
pub const DEFAULT_TNP_PORT: u16 = 9180;
/// Default Arrow Flight analytical port.
pub const DEFAULT_FLIGHT_PORT: u16 = 9181;
/// Default logical database name.
pub const DEFAULT_DATABASE_NAME: &str = "temnion_default";
/// Default superuser username.
pub const DEFAULT_ADMIN_USER: &str = "temnion_admin";

/// Configuration settings for connecting to a Temnion database instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    /// Host name or IP address of the target server.
    pub host: String,
    /// TNP protocol port.
    pub port: u16,
    /// Logical database name.
    pub database: String,
    /// User account name for authentication.
    pub username: String,
    /// Authentication token or password.
    pub auth_token: Option<String>,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: DEFAULT_TNP_PORT,
            database: DEFAULT_DATABASE_NAME.to_string(),
            username: DEFAULT_ADMIN_USER.to_string(),
            auth_token: None,
        }
    }
}

impl ConnectionConfig {
    /// Creates a connection URI string: `temnion://[user[:password]@]host[:port]/database`.
    pub fn to_uri(&self) -> String {
        let auth_part = match (&self.username, &self.auth_token) {
            (user, Some(pass)) if !pass.is_empty() => format!("{user}:{pass}@"),
            (user, _) if !user.is_empty() => format!("{user}@"),
            _ => String::new(),
        };
        format!(
            "temnion://{}{}:{}/{}",
            auth_part, self.host, self.port, self.database
        )
    }

    /// Parses connection parameters from a `temnion://` URI or `host:port` string.
    pub fn from_uri(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        let stripped = trimmed.strip_prefix("temnion://").unwrap_or(trimmed);

        let mut config = Self::default();

        // Check for database path suffix: .../database
        let (host_auth_part, db_part) = match stripped.split_once('/') {
            Some((front, back)) => (front, Some(back)),
            None => (stripped, None),
        };

        if let Some(db) = db_part {
            let db_clean = db.trim();
            if !db_clean.is_empty() {
                config.database = db_clean.to_string();
            }
        }

        // Check for user:pass@ credentials prefix
        let (auth_part, host_port_part) = match host_auth_part.split_once('@') {
            Some((user_pass, hp)) => (Some(user_pass), hp),
            None => (None, host_auth_part),
        };

        if let Some(user_pass) = auth_part {
            if let Some((user, pass)) = user_pass.split_once(':') {
                config.username = user.to_string();
                if !pass.is_empty() {
                    config.auth_token = Some(pass.to_string());
                }
            } else if !user_pass.is_empty() {
                config.username = user_pass.to_string();
            }
        }

        // Parse host:port
        if let Some((h, p)) = host_port_part.split_once(':') {
            let h_clean = h.trim();
            if !h_clean.is_empty() {
                config.host = h_clean.to_string();
            }
            let p_parsed = p
                .trim()
                .parse::<u16>()
                .map_err(|e| format!("Invalid port number '{p}': {e}"))?;
            config.port = p_parsed;
        } else if !host_port_part.is_empty() {
            config.host = host_port_part.to_string();
        }

        Ok(config)
    }

    /// Reads connection settings from environment variables if present.
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(host) = std::env::var("TEMNION_HOST") {
            if !host.trim().is_empty() {
                config.host = host.trim().to_string();
            }
        }
        if let Ok(port) = std::env::var("TEMNION_PORT") {
            if let Ok(p) = port.trim().parse::<u16>() {
                config.port = p;
            }
        }
        if let Ok(db) = std::env::var("TEMNION_DATABASE") {
            if !db.trim().is_empty() {
                config.database = db.trim().to_string();
            }
        }
        if let Ok(user) = std::env::var("TEMNION_USER") {
            if !user.trim().is_empty() {
                config.username = user.trim().to_string();
            }
        }
        if let Ok(token) = std::env::var("TEMNION_AUTH_TOKEN") {
            if !token.trim().is_empty() {
                config.auth_token = Some(token.trim().to_string());
            }
        }

        config
    }

    /// Parses a simple `connections.toml` file.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let mut config = Self::default();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                let key = k.trim();
                let val = v.trim().trim_matches('"').trim_matches('\'');
                match key {
                    "host" => config.host = val.to_string(),
                    "port" => {
                        if let Ok(p) = val.parse::<u16>() {
                            config.port = p;
                        }
                    }
                    "database" | "database_name" => config.database = val.to_string(),
                    "username" | "admin_user" => config.username = val.to_string(),
                    "auth_token" | "password" if !val.is_empty() => {
                        config.auth_token = Some(val.to_string());
                    }
                    _ => {}
                }
            }
        }

        Ok(config)
    }

    /// Hierarchically resolves configuration using priority:
    /// 1. Explicit CLI arguments
    /// 2. Connection URI parameter
    /// 3. Explicit config file
    /// 4. Environment variables
    /// 5. Standard user profile file (`~/.temnion/connections.toml` or `./temnion-connect.toml`)
    /// 6. Default settings
    pub fn resolve(
        uri: Option<&str>,
        host: Option<&str>,
        port: Option<u16>,
        db: Option<&str>,
        user: Option<&str>,
        auth_token: Option<&str>,
        config_path: Option<&Path>,
    ) -> Self {
        // Base: defaults
        let mut resolved = Self::default();

        // Check standard file locations
        let standard_paths = [
            PathBuf::from("temnion-connect.toml"),
            PathBuf::from("connections.toml"),
        ];

        let mut candidate_file = config_path.map(PathBuf::from);
        if candidate_file.is_none() {
            for p in &standard_paths {
                if p.exists() {
                    candidate_file = Some(p.clone());
                    break;
                }
            }
        }
        if candidate_file.is_none() {
            if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
            {
                let user_conf = PathBuf::from(home)
                    .join(".temnion")
                    .join("connections.toml");
                if user_conf.exists() {
                    candidate_file = Some(user_conf);
                }
            }
        }

        if let Some(ref path) = candidate_file {
            if let Ok(file_cfg) = Self::from_file(path) {
                resolved = file_cfg;
            }
        }

        // Apply environment variables
        let env_cfg = Self::from_env();
        if std::env::var_os("TEMNION_HOST").is_some() {
            resolved.host = env_cfg.host;
        }
        if std::env::var_os("TEMNION_PORT").is_some() {
            resolved.port = env_cfg.port;
        }
        if std::env::var_os("TEMNION_DATABASE").is_some() {
            resolved.database = env_cfg.database;
        }
        if std::env::var_os("TEMNION_USER").is_some() {
            resolved.username = env_cfg.username;
        }
        if std::env::var_os("TEMNION_AUTH_TOKEN").is_some() {
            resolved.auth_token = env_cfg.auth_token;
        }

        // Apply URI if provided
        if let Some(u) = uri {
            if let Ok(uri_cfg) = Self::from_uri(u) {
                resolved = uri_cfg;
            }
        }

        // Apply explicit command-line overrides
        if let Some(h) = host {
            if !h.is_empty() {
                resolved.host = h.to_string();
            }
        }
        if let Some(p) = port {
            resolved.port = p;
        }
        if let Some(d) = db {
            if !d.is_empty() {
                resolved.database = d.to_string();
            }
        }
        if let Some(u) = user {
            if !u.is_empty() {
                resolved.username = u.to_string();
            }
        }
        if let Some(t) = auth_token {
            resolved.auth_token = if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            };
        }

        resolved
    }
}

/// Errors occurring during connector setup or live communication.
#[derive(Debug)]
pub enum ConnectorError {
    Network(String),
    Protocol(TnpError),
    HandshakeRejected { server_id: String, reason: String },
    AuthFailed(String),
    ServerTimeout,
}

impl fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(msg) => write!(f, "Connector network error: {msg}"),
            Self::Protocol(err) => write!(f, "Connector protocol error: {err}"),
            Self::HandshakeRejected { server_id, reason } => {
                write!(f, "Connection rejected by server '{server_id}': {reason}")
            }
            Self::AuthFailed(msg) => write!(f, "Authentication failed: {msg}"),
            Self::ServerTimeout => write!(f, "Connection timed out"),
        }
    }
}

impl Error for ConnectorError {}

impl From<TnpError> for ConnectorError {
    fn from(err: TnpError) -> Self {
        Self::Protocol(err)
    }
}

/// An established, authenticated network session with a Temnion database daemon.
pub struct ActiveConnection {
    config: ConnectionConfig,
    server_id: String,
    negotiated_version: u16,
    capability_flags: u64,
    channel: TnpChannel<TcpStream, TcpStream>,
}

impl ActiveConnection {
    /// Connects to a Temnion server using the provided `ConnectionConfig`.
    pub fn connect(config: ConnectionConfig) -> Result<Self, ConnectorError> {
        let addr = format!("{}:{}", config.host, config.port);
        let stream = TcpStream::connect_timeout(
            &addr.parse().map_err(|e| {
                ConnectorError::Network(format!("Invalid socket address '{addr}': {e}"))
            })?,
            Duration::from_secs(5),
        )
        .map_err(|e| ConnectorError::Network(format!("Failed to connect to {addr}: {e}")))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| ConnectorError::Network(e.to_string()))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| ConnectorError::Network(e.to_string()))?;

        let reader = stream
            .try_clone()
            .map_err(|e| ConnectorError::Network(e.to_string()))?;
        let writer = stream;
        let mut channel = TnpChannel::new(reader, writer);

        // Perform Handshake
        let req = HandshakeRequest {
            client_version: TNP_VERSION,
            client_id: format!("{}:{}", config.username, "tzeentch-client"),
            capability_flags: 0x01, // standard query & stream capability
        };
        let packet = TnpPacket::new(TnpMessageType::HandshakeRequest, 1, req.encode());
        channel.send(&packet).map_err(ConnectorError::Protocol)?;

        let response_packet = channel.recv().map_err(ConnectorError::Protocol)?;
        if response_packet.message_type != TnpMessageType::HandshakeResponse {
            return Err(ConnectorError::Protocol(TnpError::ProtocolViolation(
                format!(
                    "Expected HandshakeResponse, got {:?}",
                    response_packet.message_type
                ),
            )));
        }

        let resp = HandshakeResponse::decode(&response_packet.payload)
            .map_err(ConnectorError::Protocol)?;
        if !resp.success {
            return Err(ConnectorError::HandshakeRejected {
                server_id: resp.server_id,
                reason: "Negotiation failed on server".to_string(),
            });
        }

        Ok(Self {
            config,
            server_id: resp.server_id,
            negotiated_version: resp.negotiated_version,
            capability_flags: resp.capability_flags,
            channel,
        })
    }

    /// Pings the remote server and measures round-trip latency.
    pub fn ping(&mut self) -> Result<Duration, ConnectorError> {
        let start = Instant::now();
        let ping_packet = TnpPacket::new(TnpMessageType::Ping, 2, vec![1, 2, 3, 4]);
        self.channel
            .send(&ping_packet)
            .map_err(ConnectorError::Protocol)?;

        let pong = self.channel.recv().map_err(ConnectorError::Protocol)?;
        if pong.message_type != TnpMessageType::Pong {
            return Err(ConnectorError::Protocol(TnpError::ProtocolViolation(
                format!("Expected Pong, got {:?}", pong.message_type),
            )));
        }

        Ok(start.elapsed())
    }

    /// Server identifier reported during handshake.
    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    /// Negotiated protocol version.
    pub fn negotiated_version(&self) -> u16 {
        self.negotiated_version
    }

    /// Capability flags reported by server.
    pub fn capability_flags(&self) -> u64 {
        self.capability_flags
    }

    /// Reference to connection settings.
    pub fn config(&self) -> &ConnectionConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_parsing_and_formatting() {
        let uri = "temnion://admin:secret123@192.168.1.50:9199/production";
        let config = ConnectionConfig::from_uri(uri).expect("Parse failed");
        assert_eq!(config.host, "192.168.1.50");
        assert_eq!(config.port, 9199);
        assert_eq!(config.database, "production");
        assert_eq!(config.username, "admin");
        assert_eq!(config.auth_token.as_deref(), Some("secret123"));

        let formatted = config.to_uri();
        assert_eq!(formatted, uri);
    }

    #[test]
    fn test_uri_defaults() {
        let uri = "10.0.0.1:8000";
        let config = ConnectionConfig::from_uri(uri).expect("Parse failed");
        assert_eq!(config.host, "10.0.0.1");
        assert_eq!(config.port, 8000);
        assert_eq!(config.database, DEFAULT_DATABASE_NAME);
        assert_eq!(config.username, DEFAULT_ADMIN_USER);
        assert_eq!(config.auth_token, None);
    }

    #[test]
    fn test_resolve_hierarchy() {
        let resolved = ConnectionConfig::resolve(
            Some("temnion://custom_user:custom_tok@127.0.0.1:9180/custom_db"),
            Some("override_host"),
            Some(9200),
            None,
            None,
            None,
            None,
        );

        // Explicit host and port should override URI host and port
        assert_eq!(resolved.host, "override_host");
        assert_eq!(resolved.port, 9200);
        // Database and credentials preserved from URI
        assert_eq!(resolved.database, "custom_db");
        assert_eq!(resolved.username, "custom_user");
        assert_eq!(resolved.auth_token.as_deref(), Some("custom_tok"));
    }
}
