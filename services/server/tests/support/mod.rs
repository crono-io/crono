//! Test database selection shared by the server's integration tests.
//!
//! Tests that need PostgreSQL use the initialized database named by
//! `CRONO_TEST_DATABASE_URL`. `just test` starts a throwaway PostgreSQL on a
//! random loopback port for them, and CI points the variable at its service
//! container, so neither touches a developer's `crono-postgres` or another
//! project's database on port 5432. Without the variable a test is skipped,
//! unless `CRONO_TEST_REQUIRE_DATABASE=1` (set by `just test` and CI) turns
//! the missing database into a failure: no pipeline can skip these silently.
//!
//! Cargo runs one test binary at a time, and every test creates its own
//! uniquely named resources, so the tests share one database safely. Do not
//! point the variable at a database a running scheduler writes to: the
//! monitor test compares counts read moments apart.

use anyhow::{Result, bail};
use std::env;

/// The initialized test database URL, or `None` when the test should be skipped.
///
/// # Errors
///
/// Fails when `CRONO_TEST_REQUIRE_DATABASE=1` and no database is configured.
pub fn database_url() -> Result<Option<String>> {
    match env::var("CRONO_TEST_DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => Ok(Some(url)),
        _ if database_required() => {
            bail!("CRONO_TEST_REQUIRE_DATABASE=1 but CRONO_TEST_DATABASE_URL is not set")
        }
        _ => {
            eprintln!(
                "skipped: set CRONO_TEST_DATABASE_URL to an initialized database (just test starts one)"
            );
            Ok(None)
        }
    }
}

fn database_required() -> bool {
    matches!(
        env::var("CRONO_TEST_REQUIRE_DATABASE").as_deref(),
        Ok("1" | "true")
    )
}
