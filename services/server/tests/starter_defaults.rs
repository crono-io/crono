//! Verify the seeded resources and the editable default Target boundary.
//!
//! Bootstrap runs before these database tests. This test reads its starter
//! records without changing shared test state, then checks database edits in
//! a transaction that is rolled back before another test can use the Target.

mod support;

use anyhow::{Result, bail};
use crono_server::{
    application::{
        Application, ApplicationError, PermitAllAuthorizer, Principal, PrincipalKind,
        RequestContext,
    },
    infrastructure::{DatabasePoolConfig, PostgresStore},
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn default_target_keeps_its_name_but_accepts_argument_edits() -> Result<()> {
    let Some(database_url) = support::database_url()? else {
        return Ok(());
    };
    let pool = PgPool::connect(&database_url).await?;
    let target_id: Uuid = sqlx::query_scalar(
        "SELECT t.id FROM crono.targets AS t
         JOIN crono.namespaces AS n ON n.id = t.namespace_id
         WHERE n.name = 'default' AND t.name = 'default'",
    )
    .fetch_one(&pool)
    .await?;
    let store = PostgresStore::connect(&database_url, &DatabasePoolConfig::default()).await?;
    let app = Application::new(Arc::new(store), Arc::new(PermitAllAuthorizer));
    let context = RequestContext::new(
        Uuid::now_v7(),
        Principal::new("development/local".to_string(), PrincipalKind::Development),
    );
    assert!(matches!(
        app.update_target(
            &context,
            target_id,
            "renamed",
            Vec::new(),
            serde_json::json!({}),
        )
        .await,
        Err(ApplicationError::InvalidInput {
            field: Some("name"),
            ..
        })
    ));

    let mut transaction = pool.begin().await?;
    sqlx::query("SAVEPOINT default_target_rename")
        .execute(&mut *transaction)
        .await?;
    let rename = sqlx::query("UPDATE crono.targets SET name = 'renamed' WHERE id = $1")
        .bind(target_id)
        .execute(&mut *transaction)
        .await;
    let Err(error) = rename else {
        bail!("the default Target was renamed directly in PostgreSQL");
    };
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("23514")
    );
    sqlx::query("ROLLBACK TO SAVEPOINT default_target_rename")
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE crono.targets SET arguments = '[\"sample\"]'::jsonb WHERE id = $1")
        .bind(target_id)
        .execute(&mut *transaction)
        .await?;
    let arguments: serde_json::Value =
        sqlx::query_scalar("SELECT arguments FROM crono.targets WHERE id = $1")
            .bind(target_id)
            .fetch_one(&mut *transaction)
            .await?;
    assert_eq!(arguments, serde_json::json!(["sample"]));
    transaction.rollback().await?;
    Ok(())
}
