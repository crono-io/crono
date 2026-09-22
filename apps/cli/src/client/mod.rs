//! Minimal boundary for future communication with `crono-server`.
//!
//! The client owns validated API connection configuration and deliberately has
//! no HTTP transport yet. It is independent from clap, server implementation
//! types, PostgreSQL, NATS, and worker internals. Authentication and request
//! methods belong here only after public server contracts exist.

pub mod config;

use config::ClientConfig;
use url::Url;

/// Client-side representation of a connection to the public Crono API.
pub struct CronoClient {
    config: ClientConfig,
}

impl CronoClient {
    /// Construct a client without opening a network connection.
    #[must_use]
    pub const fn new(config: ClientConfig) -> Self {
        Self { config }
    }

    /// Return the validated base URL for future public API requests.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        self.config.base_url()
    }
}
