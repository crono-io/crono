use super::{
    CREDS_FILE_ENV, NKEY_SEED_FILE_ENV, NatsAuth, NatsClientConfig, PASSWORD_ENV, TLS_CA_FILE_ENV,
    USER_ENV, credentials_auth, nkey_auth, parse_credentials, requires_tls, server_urls,
};
use anyhow::{Context, Result};
use nkeys::KeyPair;
use std::{collections::HashMap, path::Path};

const URL: &str = "tls://nats.example.test:4222";

fn config(vars: &[(&str, &str)]) -> Result<NatsClientConfig> {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    NatsClientConfig::from_lookup(URL.to_owned(), |name| vars.get(name).cloned())
}

fn error_text(vars: &[(&str, &str)]) -> String {
    config(vars).map_or_else(|error| format!("{error:#}"), |_| String::new())
}

fn creds_file(jwt: &str, seed: &str) -> String {
    format!(
        "-----BEGIN NATS USER JWT-----\n{jwt}\n------END NATS USER JWT------\n\n\
         ************************* IMPORTANT *************************\n\
         NKEY Seed printed below can be used to sign and prove identity.\n\n\
         -----BEGIN USER NKEY SEED-----\n{seed}\n------END USER NKEY SEED------\n"
    )
}

#[test]
fn from_env_without_variables_connects_anonymously_without_custom_ca() -> Result<()> {
    let config = config(&[])?;
    assert_eq!(config.auth, NatsAuth::Anonymous);
    assert_eq!(config.tls_ca_file, None);
    assert_eq!(config.url, URL);
    Ok(())
}

#[test]
fn from_env_selects_nkey_seed_file_and_private_ca() -> Result<()> {
    let config = config(&[
        (TLS_CA_FILE_ENV, "/etc/crono/nats/ca.crt"),
        (NKEY_SEED_FILE_ENV, "/etc/crono/nats/nkey.seed"),
    ])?;
    assert_eq!(
        config.auth,
        NatsAuth::NKeySeedFile(Path::new("/etc/crono/nats/nkey.seed").into())
    );
    assert_eq!(
        config.tls_ca_file.as_deref(),
        Some(Path::new("/etc/crono/nats/ca.crt"))
    );
    Ok(())
}

#[test]
fn from_env_selects_credentials_file() -> Result<()> {
    let config = config(&[(CREDS_FILE_ENV, "/etc/crono/nats/user.creds")])?;
    assert_eq!(
        config.auth,
        NatsAuth::CredentialsFile(Path::new("/etc/crono/nats/user.creds").into())
    );
    Ok(())
}

#[test]
fn from_env_selects_static_user_and_password() -> Result<()> {
    let config = config(&[(USER_ENV, "crono"), (PASSWORD_ENV, "s3cret")])?;
    assert_eq!(
        config.auth,
        NatsAuth::UserPassword {
            user: "crono".to_owned(),
            password: "s3cret".to_owned(),
        }
    );
    Ok(())
}

#[test]
fn from_env_treats_empty_values_as_unset() -> Result<()> {
    let config = config(&[
        (TLS_CA_FILE_ENV, ""),
        (NKEY_SEED_FILE_ENV, ""),
        (CREDS_FILE_ENV, "/etc/crono/nats/user.creds"),
        (USER_ENV, ""),
        (PASSWORD_ENV, ""),
    ])?;
    assert_eq!(config.tls_ca_file, None);
    assert_eq!(
        config.auth,
        NatsAuth::CredentialsFile(Path::new("/etc/crono/nats/user.creds").into())
    );
    Ok(())
}

#[test]
fn from_env_rejects_user_without_password() {
    let error = error_text(&[(USER_ENV, "crono")]);
    assert!(error.contains("CRONO_NATS_PASSWORD"), "{error}");
}

#[test]
fn from_env_rejects_password_without_user_and_hides_the_value() {
    let error = error_text(&[(PASSWORD_ENV, "s3cret")]);
    assert!(error.contains("CRONO_NATS_USER"), "{error}");
    assert!(!error.contains("s3cret"), "{error}");
}

#[test]
fn from_env_rejects_nkey_and_credentials_together() {
    let error = error_text(&[(NKEY_SEED_FILE_ENV, "/a"), (CREDS_FILE_ENV, "/b")]);
    assert!(error.contains(NKEY_SEED_FILE_ENV), "{error}");
    assert!(error.contains(CREDS_FILE_ENV), "{error}");
}

#[test]
fn from_env_rejects_every_mode_at_once_without_leaking_the_password() {
    let error = error_text(&[
        (NKEY_SEED_FILE_ENV, "/a"),
        (CREDS_FILE_ENV, "/b"),
        (USER_ENV, "crono"),
        (PASSWORD_ENV, "s3cret"),
    ]);
    assert!(error.contains("at most one"), "{error}");
    assert!(
        error.contains("CRONO_NATS_USER/CRONO_NATS_PASSWORD"),
        "{error}"
    );
    assert!(!error.contains("s3cret"), "{error}");
}

#[test]
fn from_env_rejects_nkey_with_user_and_password() {
    let error = error_text(&[
        (NKEY_SEED_FILE_ENV, "/a"),
        (USER_ENV, "crono"),
        (PASSWORD_ENV, "s3cret"),
    ]);
    assert!(error.contains(NKEY_SEED_FILE_ENV), "{error}");
}

#[test]
fn debug_output_omits_url_and_redacts_password() -> Result<()> {
    let config = NatsClientConfig::from_lookup(
        "nats://embedded:url-secret@127.0.0.1:4222".to_owned(),
        |name| match name {
            USER_ENV => Some("crono".to_owned()),
            PASSWORD_ENV => Some("s3cret".to_owned()),
            _ => None,
        },
    )?;
    let debug = format!("{config:?}");
    assert!(debug.contains("crono"), "{debug}");
    assert!(debug.contains("<redacted>"), "{debug}");
    assert!(!debug.contains("s3cret"), "{debug}");
    assert!(!debug.contains("url-secret"), "{debug}");
    Ok(())
}

#[test]
fn requires_tls_for_tls_and_secure_websocket_schemes_in_any_case() {
    for url in [
        "tls://nats:4222",
        "TLS://nats:4222",
        "wss://nats:443",
        "Wss://nats",
    ] {
        assert!(requires_tls(&server_urls(url)), "{url}");
    }
}

#[test]
fn requires_tls_is_off_for_plain_and_scheme_less_urls() {
    for url in [
        "nats://127.0.0.1:4222",
        "ws://nats:80",
        "127.0.0.1:4222",
        "",
        "nats://tls.example.test:4222",
    ] {
        assert!(!requires_tls(&server_urls(url)), "{url}");
    }
}

#[test]
fn requires_tls_when_any_server_in_a_list_uses_tls() {
    let urls = server_urls(" nats://a:4222 , TLS://b:4222,, ");
    assert_eq!(urls, ["nats://a:4222", "TLS://b:4222"]);
    assert!(requires_tls(&urls));
    assert!(!requires_tls(&server_urls("nats://a:4222,nats://b:4222")));
}

fn parse_error(contents: &str) -> String {
    parse_credentials(contents).map_or_else(|error| format!("{error:#}"), |_| String::new())
}

#[test]
fn parse_credentials_extracts_jwt_then_seed() -> Result<()> {
    let contents = creds_file("eyJ0eXAi.payload.sig", "SUAEXAMPLESEED");
    assert_eq!(
        parse_credentials(&contents)?,
        ("eyJ0eXAi.payload.sig", "SUAEXAMPLESEED")
    );
    Ok(())
}

#[test]
fn parse_credentials_skips_blank_lines_after_a_begin_marker() -> Result<()> {
    let contents = creds_file("\n  \neyJ0eXAi.payload.sig", "\nSUAEXAMPLESEED");
    assert_eq!(
        parse_credentials(&contents)?,
        ("eyJ0eXAi.payload.sig", "SUAEXAMPLESEED")
    );
    Ok(())
}

#[test]
fn parse_credentials_rejects_an_empty_jwt_block() {
    let contents = "-----BEGIN NATS USER JWT-----\n------END NATS USER JWT------\n\
                    -----BEGIN USER NKEY SEED-----\nSUAEXAMPLESEED\n------END USER NKEY SEED------\n";
    let error = parse_error(contents);
    assert!(error.contains("user JWT block is empty"), "{error}");
}

#[test]
fn parse_credentials_rejects_a_file_without_a_seed_block() {
    let contents = "-----BEGIN NATS USER JWT-----\neyJ0eXAi\n------END NATS USER JWT------\n";
    let error = parse_error(contents);
    assert!(error.contains("missing the NKey seed block"), "{error}");
}

#[test]
fn parse_credentials_rejects_unparseable_contents_without_echoing_them() {
    let error = parse_error("not a creds file SUSECRETMATERIAL");
    assert!(error.contains("missing the user JWT block"), "{error}");
    assert!(!error.contains("SUSECRETMATERIAL"), "{error}");
}

#[tokio::test]
async fn credentials_auth_reports_a_missing_file_by_path() {
    let error = credentials_auth(Path::new("/nonexistent/crono/user.creds"), b"nonce")
        .await
        .map_or_else(|error| format!("{error:#}"), |_| String::new());
    assert!(error.contains("/nonexistent/crono/user.creds"), "{error}");
}

#[tokio::test]
async fn credentials_auth_reports_an_unparseable_file_by_path() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("user.creds");
    std::fs::write(&path, "garbage")?;
    let error = credentials_auth(&path, b"nonce")
        .await
        .map_or_else(|error| format!("{error:#}"), |_| String::new());
    assert!(error.contains("invalid NATS credentials file"), "{error}");
    assert!(error.contains("missing the user JWT block"), "{error}");
    Ok(())
}

#[tokio::test]
async fn credentials_auth_rejects_a_bad_seed_without_echoing_it() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("user.creds");
    std::fs::write(
        &path,
        creds_file("eyJ0eXAi.payload.sig", "SUNOTAREALSEEDVALUE"),
    )?;
    let error = credentials_auth(&path, b"nonce")
        .await
        .map_or_else(|error| format!("{error:#}"), |_| String::new());
    assert!(
        error.contains("does not contain a valid NATS NKey seed"),
        "{error}"
    );
    assert!(!error.contains("SUNOTAREALSEEDVALUE"), "{error}");
    Ok(())
}

#[tokio::test]
async fn nkey_auth_rereads_a_rotated_seed_file_on_every_attempt() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nkey.seed");
    let nonce = b"server-nonce";
    for key_pair in [KeyPair::new_user(), KeyPair::new_user()] {
        std::fs::write(&path, format!("{}\n", key_pair.seed()?))?;
        let auth = nkey_auth(&path, nonce).await?;
        assert_eq!(auth.nkey.as_deref(), Some(key_pair.public_key().as_str()));
        let signature = auth.signature.context("NKey auth must sign the nonce")?;
        key_pair.verify(nonce, &signature)?;
    }
    Ok(())
}

#[tokio::test]
async fn nkey_auth_reports_an_invalid_seed_without_echoing_it() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nkey.seed");
    std::fs::write(&path, "SUNOTAREALSEEDVALUE")?;
    let error = nkey_auth(&path, b"nonce")
        .await
        .map_or_else(|error| format!("{error:#}"), |_| String::new());
    assert!(
        error.contains("does not contain a valid NATS NKey seed"),
        "{error}"
    );
    assert!(!error.contains("SUNOTAREALSEEDVALUE"), "{error}");
    Ok(())
}

#[tokio::test]
async fn nkey_auth_reports_a_missing_seed_file_by_path() {
    let error = nkey_auth(Path::new("/nonexistent/crono/nkey.seed"), b"nonce")
        .await
        .map_or_else(|error| format!("{error:#}"), |_| String::new());
    assert!(error.contains("/nonexistent/crono/nkey.seed"), "{error}");
}

#[tokio::test]
async fn credentials_auth_presents_jwt_and_signs_with_the_creds_seed() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("user.creds");
    let key_pair = KeyPair::new_user();
    std::fs::write(&path, creds_file("eyJ0eXAi.payload.sig", &key_pair.seed()?))?;
    let nonce = b"server-nonce";
    let auth = credentials_auth(&path, nonce).await?;
    assert_eq!(auth.jwt.as_deref(), Some("eyJ0eXAi.payload.sig"));
    assert_eq!(auth.nkey, None);
    let signature = auth.signature.context("creds auth must sign the nonce")?;
    key_pair.verify(nonce, &signature)?;
    Ok(())
}
