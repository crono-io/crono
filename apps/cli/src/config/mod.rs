//! Resolve application configuration independently from command syntax.
//!
//! Address precedence is explicit CLI input, then `CRONO_ADDR`, then the local
//! development default. Resolution validates the selected value before action
//! execution and never contacts `crono-server`, allowing configuration to be
//! tested without PostgreSQL, NATS, workers, or any external service.

use crate::client::config::ClientConfig;
use anyhow::{Result, bail};
use std::env::{self, VarError};

/// Local-development server address used when no explicit source is present.
pub const DEFAULT_ADDRESS: &str = "http://127.0.0.1:8080";

/// Fully resolved configuration for one CLI invocation.
pub struct Config {
    client: ClientConfig,
}

impl Config {
    /// Resolve the address using CLI, environment, and default precedence.
    ///
    /// # Errors
    ///
    /// Returns an error when `CRONO_ADDR` is not valid Unicode or when the
    /// selected address fails client validation.
    pub fn load(explicit_address: Option<&str>) -> Result<Self> {
        let address = if let Some(address) = explicit_address {
            address.to_string()
        } else {
            match env::var("CRONO_ADDR") {
                Ok(address) => address,
                Err(VarError::NotPresent) => DEFAULT_ADDRESS.to_string(),
                Err(VarError::NotUnicode(_)) => {
                    bail!("CRONO_ADDR must contain valid Unicode")
                }
            }
        };
        let client = ClientConfig::parse(&address)?;
        Ok(Self { client })
    }

    /// Consume the application configuration and return client settings.
    #[must_use]
    pub fn into_client(self) -> ClientConfig {
        self.client
    }
}
