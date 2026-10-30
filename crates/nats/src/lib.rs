//! NATS client configuration and authenticated connection setup shared by the
//! Crono server and worker.
//!
//! Both native services reach the same NATS deployment: the server from inside
//! the cluster and workers from virtual machines outside it. They therefore
//! share one configuration contract read from `CRONO_NATS_*` environment
//! variables, so a worker and the server can never disagree about how TLS or
//! authentication is selected. Local development keeps working unchanged:
//! with none of the variables set, the client connects anonymously to the
//! given URL exactly as before.
//!
//! # Flow Overview
//!
//! [`NatsClientConfig::from_env`] takes the service URL (`CRONO_NATS_URL` or
//! the worker's `--nats-url`, optionally a comma-separated server list) and
//! reads the optional private CA and exactly
//! one authentication mode: an `NKey` seed file, a JWT `.creds` file, or a
//! static user and password. Selecting more than one mode, or half of the
//! user/password pair, is a startup error rather than a silent precedence
//! rule, because guessing which identity an operator meant would connect
//! with the wrong subject permissions. [`NatsClientConfig::connect`] then
//! builds `async-nats` options with unlimited reconnects and returns the
//! connected client.
//!
//! # Rotation and file handling
//!
//! The `NKey` seed and `.creds` files are read inside the `async-nats`
//! authentication callback, which runs on the initial connection and on every
//! reconnect. The CA file is likewise re-read by `async-nats` whenever it
//! builds a TLS session. A Kubernetes Secret update or an operator replacing a
//! file on a VM is therefore picked up by the next reconnect without a
//! restart. The static user and password come from environment variables,
//! which a running process cannot observe changing; rotating them requires a
//! restart.
//!
//! # Trust boundaries
//!
//! A configured CA, or any `tls://` or `wss://` server in the URL, forces TLS
//! for every connection client-wide (`require_tls`), including reconnects to
//! scheme-less addresses the cluster advertises, so credentials are never sent
//! in plaintext to a server that does not demand TLS itself. A configured CA
//! replaces the platform trust store, so only servers issued by the private CA
//! are accepted; without one TLS verifies against the platform roots.
//! Credentials never appear in errors or `Debug`
//! output: the URL is omitted entirely because it may embed `user:password`,
//! the password is redacted, and seed material is never formatted. Errors name
//! only environment variables and file paths.

use anyhow::{Context, Result, anyhow, bail};
use async_nats::{Auth, AuthError, Client, ConnectOptions};
use nkeys::KeyPair;
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

/// PEM bundle of the private CA that issued the NATS server certificate.
pub const TLS_CA_FILE_ENV: &str = "CRONO_NATS_TLS_CA_FILE";
/// File holding an `NKey` user seed (`SU…`), the default Kubernetes identity.
pub const NKEY_SEED_FILE_ENV: &str = "CRONO_NATS_NKEY_SEED_FILE";
/// JWT `.creds` file for operator/account-based NATS deployments.
pub const CREDS_FILE_ENV: &str = "CRONO_NATS_CREDS_FILE";
/// Static user name; only valid together with [`PASSWORD_ENV`].
pub const USER_ENV: &str = "CRONO_NATS_USER";
/// Static password; only valid together with [`USER_ENV`].
pub const PASSWORD_ENV: &str = "CRONO_NATS_PASSWORD";

const REDACTED: &str = "<redacted>";

/// Validated NATS connection settings for one Crono process.
///
/// Construction performs no I/O; files are read when connecting so that a
/// rotated secret is used by the next (re)connection attempt.
#[derive(Clone)]
pub struct NatsClientConfig {
    url: String,
    tls_ca_file: Option<PathBuf>,
    auth: NatsAuth,
}

/// The single authentication mode selected from the environment.
#[derive(Clone, PartialEq, Eq)]
enum NatsAuth {
    Anonymous,
    NKeySeedFile(Arc<Path>),
    CredentialsFile(Arc<Path>),
    UserPassword { user: String, password: String },
}

impl NatsAuth {
    /// Environment variables that selected this mode, for configuration errors.
    const fn source(&self) -> &'static str {
        match self {
            Self::Anonymous => "none",
            Self::NKeySeedFile(_) => NKEY_SEED_FILE_ENV,
            Self::CredentialsFile(_) => CREDS_FILE_ENV,
            Self::UserPassword { .. } => "CRONO_NATS_USER/CRONO_NATS_PASSWORD",
        }
    }
}

impl NatsClientConfig {
    /// Connect to `url` without a private CA or credentials.
    ///
    /// This is the shape of a loopback development broker and of tests that
    /// never reach NATS; deployed services use [`Self::from_env`].
    #[must_use]
    pub fn anonymous(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            tls_ca_file: None,
            auth: NatsAuth::Anonymous,
        }
    }

    /// Read the TLS and authentication settings that accompany `url`.
    ///
    /// Unset and empty variables are equivalent, so a rendered but blank
    /// value cannot select a mode. A value that is not valid Unicode is
    /// treated as unset, matching [`std::env::var`].
    ///
    /// # Errors
    ///
    /// Returns when more than one authentication mode is configured, or when
    /// only one of `CRONO_NATS_USER` and `CRONO_NATS_PASSWORD` is set. The
    /// error names the variables but never their values.
    pub fn from_env(url: impl Into<String>) -> Result<Self> {
        Self::from_lookup(url.into(), |name| std::env::var(name).ok())
    }

    /// Resolve the configuration through `lookup`, which stands in for the
    /// process environment so validation can be tested without mutating it.
    fn from_lookup(url: String, lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let value = |name: &str| lookup(name).filter(|value| !value.is_empty());
        let tls_ca_file = value(TLS_CA_FILE_ENV).map(PathBuf::from);

        let mut modes = Vec::with_capacity(3);
        if let Some(path) = value(NKEY_SEED_FILE_ENV) {
            modes.push(NatsAuth::NKeySeedFile(Arc::from(Path::new(&path))));
        }
        if let Some(path) = value(CREDS_FILE_ENV) {
            modes.push(NatsAuth::CredentialsFile(Arc::from(Path::new(&path))));
        }
        match (value(USER_ENV), value(PASSWORD_ENV)) {
            (Some(user), Some(password)) => modes.push(NatsAuth::UserPassword { user, password }),
            (Some(_), None) => bail!("{USER_ENV} is set but {PASSWORD_ENV} is not; set both"),
            (None, Some(_)) => bail!("{PASSWORD_ENV} is set but {USER_ENV} is not; set both"),
            (None, None) => {}
        }

        if modes.len() > 1 {
            let sources = modes
                .iter()
                .map(NatsAuth::source)
                .collect::<Vec<_>>()
                .join(", ");
            bail!("configure at most one NATS authentication mode; found {sources}");
        }
        let auth = modes.pop().unwrap_or(NatsAuth::Anonymous);
        Ok(Self {
            url,
            tls_ca_file,
            auth,
        })
    }

    /// Connect to NATS with unlimited automatic reconnects.
    ///
    /// The returned client keeps reconnecting in the background after a
    /// broker restart, re-reading the CA and secret files on every attempt.
    /// Only the initial connection is awaited here; callers that tolerate a
    /// missing broker at startup retry this method themselves.
    ///
    /// # Errors
    ///
    /// Returns when the initial connection, TLS handshake, or authentication
    /// fails, including an unreadable or malformed CA, seed, or `.creds` file.
    /// The message includes file paths but not the URL or any secret.
    pub async fn connect(&self) -> Result<Client> {
        let options = match &self.auth {
            NatsAuth::Anonymous => ConnectOptions::new(),
            NatsAuth::UserPassword { user, password } => {
                ConnectOptions::with_user_and_password(user.clone(), password.clone())
            }
            NatsAuth::NKeySeedFile(path) => {
                let path = Arc::clone(path);
                ConnectOptions::with_auth_callback(move |nonce| {
                    let path = Arc::clone(&path);
                    async move {
                        nkey_auth(&path, &nonce)
                            .await
                            .map_err(|error| callback_error(&error))
                    }
                })
            }
            NatsAuth::CredentialsFile(path) => {
                let path = Arc::clone(path);
                ConnectOptions::with_auth_callback(move |nonce| {
                    let path = Arc::clone(&path);
                    async move {
                        credentials_auth(&path, &nonce)
                            .await
                            .map_err(|error| callback_error(&error))
                    }
                })
            }
        };
        let urls = server_urls(&self.url);
        let mut options = options
            .max_reconnects(None)
            .require_tls(self.tls_ca_file.is_some() || requires_tls(&urls));
        if let Some(ca) = &self.tls_ca_file {
            options = options.add_root_certificates(ca.clone());
        }
        options
            .connect(urls.as_slice())
            .await
            .map_err(|error| anyhow!("failed to connect to NATS: {error}"))
    }
}

/// Split a comma-separated server list into trimmed, non-empty entries.
fn server_urls(url: &str) -> Vec<&str> {
    url.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// Whether any configured server uses a TLS scheme (`tls://` or `wss://`).
///
/// The answer becomes the client-wide `require_tls` setting. Without it,
/// `async-nats` decides per server, and addresses learned from cluster gossip
/// carry no scheme, so a reconnect to one of them would only use TLS if that
/// server's INFO asked for it, sending credentials in plaintext otherwise.
fn requires_tls(urls: &[&str]) -> bool {
    urls.iter().any(|url| {
        url.split_once("://").is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("tls") || scheme.eq_ignore_ascii_case("wss")
        })
    })
}

/// Omit the URL (it may embed credentials) and redact the static password.
impl fmt::Debug for NatsClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NatsClientConfig")
            .field("tls_ca_file", &self.tls_ca_file)
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for NatsAuth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anonymous => formatter.write_str("Anonymous"),
            Self::NKeySeedFile(path) => formatter.debug_tuple("NKeySeedFile").field(path).finish(),
            Self::CredentialsFile(path) => formatter
                .debug_tuple("CredentialsFile")
                .field(path)
                .finish(),
            Self::UserPassword { user, .. } => formatter
                .debug_struct("UserPassword")
                .field("user", user)
                .field("password", &REDACTED)
                .finish(),
        }
    }
}

/// Flatten the context chain into the string-only error `async-nats` accepts.
fn callback_error(error: &anyhow::Error) -> AuthError {
    AuthError::new(format!("{error:#}"))
}

/// Sign the server nonce with the `NKey` seed currently stored at `path`.
///
/// Seed parsing and signing errors are replaced by fixed messages because the
/// underlying error types are not guaranteed to omit key material.
async fn nkey_auth(path: &Path, nonce: &[u8]) -> Result<Auth> {
    let seed = read_secret_file(path, "NKey seed").await?;
    let key_pair = key_pair_from_seed(seed.trim(), path)?;
    let mut auth = Auth::new();
    auth.signature = Some(sign_nonce(&key_pair, nonce)?);
    auth.nkey = Some(key_pair.public_key());
    Ok(auth)
}

/// Present the user JWT and sign the nonce with the `.creds` file's seed.
async fn credentials_auth(path: &Path, nonce: &[u8]) -> Result<Auth> {
    let contents = read_secret_file(path, "credentials").await?;
    let (jwt, seed) = parse_credentials(&contents)
        .with_context(|| format!("invalid NATS credentials file {}", path.display()))?;
    let key_pair = key_pair_from_seed(seed, path)?;
    let mut auth = Auth::new();
    auth.signature = Some(sign_nonce(&key_pair, nonce)?);
    auth.jwt = Some(jwt.to_owned());
    Ok(auth)
}

async fn read_secret_file(path: &Path, label: &str) -> Result<String> {
    tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read the NATS {label} file {}", path.display()))
}

fn key_pair_from_seed(seed: &str, path: &Path) -> Result<KeyPair> {
    KeyPair::from_seed(seed)
        .map_err(|_| anyhow!("{} does not contain a valid NATS NKey seed", path.display()))
}

fn sign_nonce(key_pair: &KeyPair, nonce: &[u8]) -> Result<Vec<u8>> {
    key_pair
        .sign(nonce)
        .map_err(|_| anyhow!("failed to sign the NATS server nonce"))
}

/// Extract the user JWT and `NKey` seed from the decorated `.creds` format.
///
/// The first dashed `BEGIN` block holds the JWT and the second the seed,
/// matching the layout `nsc` generates. A block's value is its first
/// non-blank line; a block that reaches its closing marker first is empty
/// and rejected. Errors never quote file contents.
fn parse_credentials(contents: &str) -> Result<(&str, &str)> {
    let mut lines = contents.lines().map(str::trim);
    let mut next_block = |label: &str| -> Result<&str> {
        lines
            .by_ref()
            .find(|line| is_marker(line) && line.contains("BEGIN"))
            .with_context(|| format!("missing the {label} block"))?;
        lines
            .by_ref()
            .find(|line| !line.is_empty())
            .filter(|line| !is_marker(line))
            .with_context(|| format!("the {label} block is empty"))
    };
    let jwt = next_block("user JWT")?;
    let seed = next_block("NKey seed")?;
    Ok((jwt, seed))
}

/// A dashed decoration line such as `-----BEGIN NATS USER JWT-----`.
fn is_marker(line: &str) -> bool {
    line.starts_with("---") && line.ends_with("---")
}

#[cfg(test)]
mod tests;
