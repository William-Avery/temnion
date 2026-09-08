// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

//! `temniond` library providing daemon configuration, server runtime, and connection handlers.

pub mod config;
pub mod server;

pub use config::{ConfigError, DaemonConfig};
pub use server::{DaemonServer, ServerError};
