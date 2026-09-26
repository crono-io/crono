//! Bounded PostgreSQL connection-pool configuration.
//!
//! Every server instance shares one pool between the HTTP API and the
//! scheduler, reconciler, outbox publisher, and worker-control loops. Those
//! loops poll continuously, so the pool keeps a small warm floor
//! (`min_connections`, default 2) instead of reopening a connection after each
//! idle reap. The ceiling (default 20) bounds how much of PostgreSQL's
//! `max_connections` a single instance can take; operators running several
//! instances should size it so the fleet stays under the database limit.
//!
//! A short acquire timeout (default 3 seconds) turns pool exhaustion into a
//! prompt `Unavailable` response instead of letting requests queue for
//! sqlx's 30-second default. Idle connections above the floor close after
//! 10 minutes and every connection is recycled after 30 minutes. Those two
//! values match sqlx's implicit defaults but are stated here so they are
//! visible and tunable. Crono deliberately does not use a lifetime of a few
//! minutes: services that recycle that quickly usually do so because their
//! database credentials rotate, and without rotation the only effect is more
//! reconnects and TLS handshakes.
//!
//! `test_before_acquire` stays enabled so a connection the database or a
//! proxy dropped while it sat idle is replaced before a query uses it.
//!
//! Values come from `CRONO_DATABASE_*` environment variables. An unset
//! variable selects its default; a malformed or out-of-range value fails
//! startup, because a pool that cannot serve the background loops is worse
//! than a clear configuration error.

use super::environment::env_value;
use anyhow::{Result, bail};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

const DEFAULT_MAX_CONNECTIONS: u32 = 20;
const DEFAULT_MIN_CONNECTIONS: u32 = 2;
const DEFAULT_ACQUIRE_TIMEOUT_MS: u64 = 3_000;
const DEFAULT_IDLE_TIMEOUT_SECONDS: u64 = 10 * 60;
const DEFAULT_MAX_LIFETIME_SECONDS: u64 = 30 * 60;

const MAX_CONNECTIONS_LIMIT: u32 = 200;
const ACQUIRE_TIMEOUT_MIN: Duration = Duration::from_millis(100);
const ACQUIRE_TIMEOUT_MAX: Duration = Duration::from_secs(30);
const IDLE_TIMEOUT_MIN: Duration = Duration::from_secs(10);
const MAX_LIFETIME_MIN: Duration = Duration::from_mins(1);
const MAX_LIFETIME_MAX: Duration = Duration::from_hours(24);

/// Validated shape of the server's PostgreSQL pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatabasePoolConfig {
    /// Upper bound on open connections, between 1 and 200.
    pub max_connections: u32,
    /// Connections kept open while idle; never above `max_connections`.
    pub min_connections: u32,
    /// Longest wait for a free connection before the store reports `Unavailable`.
    pub acquire_timeout: Duration,
    /// Idle time after which a connection above the floor is closed.
    pub idle_timeout: Duration,
    /// Age after which any connection is closed and replaced.
    pub max_lifetime: Duration,
}

impl Default for DatabasePoolConfig {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_MAX_CONNECTIONS,
            min_connections: DEFAULT_MIN_CONNECTIONS,
            acquire_timeout: Duration::from_millis(DEFAULT_ACQUIRE_TIMEOUT_MS),
            idle_timeout: Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS),
            max_lifetime: Duration::from_secs(DEFAULT_MAX_LIFETIME_SECONDS),
        }
    }
}

impl DatabasePoolConfig {
    /// Load pool settings from `CRONO_DATABASE_*` environment variables.
    ///
    /// # Errors
    ///
    /// Returns when a configured value is malformed or outside its safe range.
    pub fn from_env() -> Result<Self> {
        let config = Self {
            max_connections: env_value("CRONO_DATABASE_MAX_CONNECTIONS", DEFAULT_MAX_CONNECTIONS)?,
            min_connections: env_value("CRONO_DATABASE_MIN_CONNECTIONS", DEFAULT_MIN_CONNECTIONS)?,
            acquire_timeout: Duration::from_millis(env_value(
                "CRONO_DATABASE_ACQUIRE_TIMEOUT_MS",
                DEFAULT_ACQUIRE_TIMEOUT_MS,
            )?),
            idle_timeout: Duration::from_secs(env_value(
                "CRONO_DATABASE_IDLE_TIMEOUT_SECONDS",
                DEFAULT_IDLE_TIMEOUT_SECONDS,
            )?),
            max_lifetime: Duration::from_secs(env_value(
                "CRONO_DATABASE_MAX_LIFETIME_SECONDS",
                DEFAULT_MAX_LIFETIME_SECONDS,
            )?),
        };
        config.validate()?;
        Ok(config)
    }

    /// Reject pool shapes that could exhaust PostgreSQL or starve the server.
    ///
    /// The idle timeout must be shorter than the lifetime; otherwise idle
    /// reaping never happens before recycling and the setting is misleading.
    fn validate(self) -> Result<()> {
        if !(1..=MAX_CONNECTIONS_LIMIT).contains(&self.max_connections) {
            bail!("CRONO_DATABASE_MAX_CONNECTIONS must be between 1 and {MAX_CONNECTIONS_LIMIT}");
        }
        if self.min_connections > self.max_connections {
            bail!("CRONO_DATABASE_MIN_CONNECTIONS must not exceed CRONO_DATABASE_MAX_CONNECTIONS");
        }
        if !(ACQUIRE_TIMEOUT_MIN..=ACQUIRE_TIMEOUT_MAX).contains(&self.acquire_timeout) {
            bail!("CRONO_DATABASE_ACQUIRE_TIMEOUT_MS must be between 100 and 30000");
        }
        if !(MAX_LIFETIME_MIN..=MAX_LIFETIME_MAX).contains(&self.max_lifetime) {
            bail!("CRONO_DATABASE_MAX_LIFETIME_SECONDS must be between 60 and 86400");
        }
        if self.idle_timeout < IDLE_TIMEOUT_MIN || self.idle_timeout >= self.max_lifetime {
            bail!(
                "CRONO_DATABASE_IDLE_TIMEOUT_SECONDS must be at least 10 and below CRONO_DATABASE_MAX_LIFETIME_SECONDS"
            );
        }
        Ok(())
    }

    /// Translate this configuration into sqlx pool options.
    ///
    /// Every option is set explicitly, including ones that equal the sqlx
    /// default, so a dependency upgrade cannot silently change pool behavior.
    pub(super) fn pool_options(self) -> PgPoolOptions {
        PgPoolOptions::new()
            .max_connections(self.max_connections)
            .min_connections(self.min_connections)
            .acquire_timeout(self.acquire_timeout)
            .idle_timeout(self.idle_timeout)
            .max_lifetime(self.max_lifetime)
            .test_before_acquire(true)
    }
}

#[cfg(test)]
mod tests {
    use super::DatabasePoolConfig;
    use std::time::Duration;

    #[test]
    fn pool_config_defaults_are_within_bounds() {
        let config = DatabasePoolConfig::default();
        assert!(config.validate().is_ok());
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.min_connections, 2);
        assert_eq!(config.acquire_timeout, Duration::from_secs(3));
    }

    #[test]
    fn pool_config_rejects_zero_max() {
        let config = DatabasePoolConfig {
            max_connections: 0,
            min_connections: 0,
            ..DatabasePoolConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn pool_config_rejects_max_above_limit() {
        let config = DatabasePoolConfig {
            max_connections: 201,
            ..DatabasePoolConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn pool_config_rejects_min_above_max() {
        let config = DatabasePoolConfig {
            max_connections: 4,
            min_connections: 5,
            ..DatabasePoolConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn pool_config_rejects_acquire_timeout_outside_bounds() {
        for acquire_timeout in [Duration::from_millis(99), Duration::from_millis(30_001)] {
            let config = DatabasePoolConfig {
                acquire_timeout,
                ..DatabasePoolConfig::default()
            };
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn pool_config_rejects_idle_timeout_beyond_lifetime() {
        let config = DatabasePoolConfig {
            idle_timeout: Duration::from_mins(30),
            max_lifetime: Duration::from_mins(30),
            ..DatabasePoolConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn pool_config_rejects_lifetime_outside_bounds() {
        for max_lifetime in [Duration::from_secs(59), Duration::from_secs(86_401)] {
            let config = DatabasePoolConfig {
                idle_timeout: Duration::from_secs(10),
                max_lifetime,
                ..DatabasePoolConfig::default()
            };
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn pool_options_apply_every_setting() {
        let config = DatabasePoolConfig {
            max_connections: 7,
            min_connections: 3,
            acquire_timeout: Duration::from_millis(1_500),
            idle_timeout: Duration::from_mins(2),
            max_lifetime: Duration::from_mins(15),
        };
        let options = config.pool_options();
        assert_eq!(options.get_max_connections(), 7);
        assert_eq!(options.get_min_connections(), 3);
        assert_eq!(options.get_acquire_timeout(), Duration::from_millis(1_500));
        assert_eq!(options.get_idle_timeout(), Some(Duration::from_mins(2)));
        assert_eq!(options.get_max_lifetime(), Some(Duration::from_mins(15)));
        assert!(options.get_test_before_acquire());
    }
}
