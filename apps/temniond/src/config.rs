// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Configuration for the `temniond` standalone background daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    /// Path to the authoritative database directory.
    pub data_dir: PathBuf,
    /// Server identifier reported during handshake.
    pub server_id: String,
    /// Address and port to bind for TNP binary protocol.
    pub tnp_bind: String,
    /// Address and port to bind for Arrow Flight service.
    pub flight_bind: String,
    /// Whether the Model Context Protocol (MCP) server is enabled.
    pub mcp_enabled: bool,
    /// Interval in seconds between periodic background maintenance checks.
    pub maintenance_interval_secs: u64,
    /// Authoritative source ID.
    pub source_id: u32,
    /// Authoritative source epoch.
    pub source_epoch: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("./data/temnion_db"),
            server_id: "temniond-primary".to_string(),
            tnp_bind: "127.0.0.1:9180".to_string(),
            flight_bind: "127.0.0.1:9181".to_string(),
            mcp_enabled: true,
            maintenance_interval_secs: 60,
            source_id: 1,
            source_epoch: 1,
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(io::Error),
    Parse(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error reading configuration: {err}"),
            Self::Parse(msg) => write!(f, "Configuration parse error: {msg}"),
        }
    }
}

impl Error for ConfigError {}

impl From<io::Error> for ConfigError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl DaemonConfig {
    /// Parses configuration from a TOML-formatted string.
    pub fn parse_toml(content: &str) -> Result<Self, ConfigError> {
        let mut config = Self::default();

        for (line_idx, line) in content.lines().enumerate() {
            let line_num = line_idx + 1;
            let trimmed = line.trim();
            // Ignore empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
                continue;
            }

            if let Some((key, val)) = trimmed.split_once('=') {
                let key = key.trim();
                let val = val.trim().trim_matches('"').trim_matches('\'');
                match key {
                    "data_dir" => config.data_dir = PathBuf::from(val),
                    "server_id" => config.server_id = val.to_string(),
                    "tnp_bind" => config.tnp_bind = val.to_string(),
                    "flight_bind" => config.flight_bind = val.to_string(),
                    "mcp_enabled" => {
                        config.mcp_enabled = val.parse::<bool>().map_err(|_| {
                            ConfigError::Parse(format!(
                                "line {line_num}: invalid boolean for mcp_enabled: '{val}'"
                            ))
                        })?;
                    }
                    "maintenance_interval_secs" => {
                        config.maintenance_interval_secs = val.parse::<u64>().map_err(|_| {
                            ConfigError::Parse(format!(
                                "line {line_num}: invalid integer for maintenance_interval_secs: '{val}'"
                            ))
                        })?;
                    }
                    "source_id" => {
                        config.source_id = val.parse::<u32>().map_err(|_| {
                            ConfigError::Parse(format!(
                                "line {line_num}: invalid integer for source_id: '{val}'"
                            ))
                        })?;
                    }
                    "source_epoch" => {
                        config.source_epoch = val.parse::<u64>().map_err(|_| {
                            ConfigError::Parse(format!(
                                "line {line_num}: invalid integer for source_epoch: '{val}'"
                            ))
                        })?;
                    }
                    _ => {
                        // Unknown key ignored for forward compatibility
                    }
                }
            } else {
                return Err(ConfigError::Parse(format!(
                    "line {line_num}: invalid syntax, expected key = value: '{trimmed}'"
                )));
            }
        }

        Ok(config)
    }

    /// Loads configuration from a file path.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        Self::parse_toml(&content)
    }

    /// Serializes configuration into a human-readable TOML string.
    pub fn to_toml_string(&self) -> String {
        format!(
            "# Temnion Standalone Daemon Configuration (temnion.toml)\n\n\
            [storage]\n\
            data_dir = \"{}\"\n\
            source_id = {}\n\
            source_epoch = {}\n\n\
            [network]\n\
            server_id = \"{}\"\n\
            tnp_bind = \"{}\"\n\
            flight_bind = \"{}\"\n\
            mcp_enabled = {}\n\n\
            [maintenance]\n\
            maintenance_interval_secs = {}\n",
            self.data_dir.display(),
            self.source_id,
            self.source_epoch,
            self.server_id,
            self.tnp_bind,
            self.flight_bind,
            self.mcp_enabled,
            self.maintenance_interval_secs,
        )
    }

    /// Saves configuration to a target file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_toml_string())
    }
}
