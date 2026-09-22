//! Validated connection configuration for the public Crono API.
//!
//! Server addresses must be absolute HTTP or HTTPS URLs with a host. User-info,
//! query strings, and fragments are rejected so credentials and request-specific
//! data cannot be smuggled into the persistent base address. HTTP is restricted
//! to loopback development addresses; deployed servers must use HTTPS. No transport or TLS
//! override exists yet; a future HTTPS transport must retain certificate and
//! hostname verification.

use anyhow::{Context, Result, bail};
use url::{Host, Url};

/// Validated connection settings owned by [`super::CronoClient`].
pub struct ClientConfig {
    base_url: Url,
}

impl ClientConfig {
    /// Parse and validate a public Crono server base address.
    ///
    /// # Errors
    ///
    /// Rejects malformed URLs, non-HTTP schemes, missing hosts, embedded
    /// user-info, query strings, and fragments. Errors never echo the address.
    pub fn parse(address: &str) -> Result<Self> {
        let base_url = Url::parse(address).context("invalid Crono server address")?;
        if !matches!(base_url.scheme(), "http" | "https") {
            bail!("Crono server address must use http or https");
        }
        if base_url.host_str().is_none() {
            bail!("Crono server address must include a host");
        }
        if base_url.scheme() == "http" && !is_loopback(&base_url) {
            bail!("http Crono server addresses are limited to loopback development hosts");
        }
        if !base_url.username().is_empty() || base_url.password().is_some() {
            bail!("Crono server address must not include user-info");
        }
        if base_url.query().is_some() || base_url.fragment().is_some() {
            bail!("Crono server address must not include a query or fragment");
        }

        Ok(Self { base_url })
    }

    /// Return the validated server base URL.
    #[must_use]
    pub const fn base_url(&self) -> &Url {
        &self.base_url
    }
}

/// Return whether a parsed URL targets an explicit local-development host.
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain == "localhost",
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}
