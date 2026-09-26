//! Typed environment lookups shared by infrastructure configuration.
//!
//! An unset variable selects the caller's default, while a present but
//! malformed value is an error so a typo never silently falls back to a
//! different runtime shape. Range validation stays with each configuration
//! type because only it knows which combinations are safe.

use anyhow::{Context, Result};
use std::env;

/// Parse `name` as `T`, returning `default` when the variable is unset.
///
/// A value that is not valid Unicode is treated as unset, matching
/// [`env::var`]; a Unicode value that fails to parse is an error.
pub(super) fn env_value<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    env::var(name).map_or_else(
        |_| Ok(default),
        |value| value.parse().with_context(|| format!("invalid {name}")),
    )
}
