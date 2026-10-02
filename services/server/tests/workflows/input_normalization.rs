//! Shared structured-input writes reuse the authenticated Workflow test fixture.
//!
//! HTTP requests exercise trimming before PostgreSQL persistence, while ordinary
//! Run snapshots prove literal content and previously committed paths stay exact.
//! The fixture's serial lock and scoped cleanup keep global reconciliation isolated.

use super::{Fixture, child, fixture_router, http};
use anyhow::{Result, anyhow};
use axum::{Router, http::StatusCode};
use crono_api::ExecutionSnapshot;
use serde_json::{Value, json};
use uuid::Uuid;

/// Build a complete write with deliberately significant whitespace in literal fields.
fn job_write() -> Value {
    json!({
        "name":" \techo\u{a0}", "queue_id":Uuid::nil(),
        "executor":"process", "executable":" \t/usr/bin/echo \n",
        "arguments":[" literal ","{{message}}"], "inputs":{"message":" value "},
        "idempotent":false, "dry_run":false, "max_attempts":1,
        "retry_initial_seconds":1, "retry_max_seconds":60,
        "retry_multiplier":2.0, "retry_jitter":0.2
    })
}

/// Read an expected response field without assuming the server supplied it.
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .get(key)
        .ok_or_else(|| anyhow!("missing response field {key}"))
}

/// Replace a request field while retaining all unrelated literal values.
fn set(value: &mut Value, key: &str, replacement: Value) -> Result<()> {
    value
        .as_object_mut()
        .ok_or_else(|| anyhow!("expected an object"))?
        .insert(key.to_owned(), replacement);
    Ok(())
}

fn id(value: &Value) -> Result<Uuid> {
    Ok(Uuid::parse_str(
        value
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("resource omitted its ID"))?,
    )?)
}

/// Exercise the actual HTTP response rather than normalizing values in the fixture.
async fn write(
    router: &Router,
    token: &str,
    method: &str,
    path: &str,
    body: Value,
) -> Result<Value> {
    let (status, response) = http(router, Some(token), method, path, Some(body)).await?;
    assert!(status.is_success(), "{status}: {response}");
    Ok(response)
}

async fn snapshot(f: &Fixture, run_id: Uuid) -> Result<ExecutionSnapshot> {
    Ok(serde_json::from_value(
        sqlx::query_scalar::<_, Value>("SELECT execution_snapshot FROM crono.runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&f.pool)
            .await?,
    )?)
}

#[tokio::test]
async fn job_writes_trim_paths_without_rewriting_literal_content_or_history() -> Result<()> {
    let Some(f) = Fixture::new(&["a"]).await? else {
        return Ok(());
    };
    let (router, token) = fixture_router(&f)?;
    let job = f.jobs.first().ok_or_else(|| anyhow!("missing Job"))?.1;
    // Simulate a definition saved before this fix, with its exact immutable history.
    sqlx::query(
        "UPDATE crono.jobs SET executor = 'process', executable = '/usr/bin/echo ' WHERE id = $1",
    )
    .bind(job.get())
    .execute(&f.pool)
    .await?;
    let graph = f.graph(&[]).await?;
    let legacy = f.start(&graph).await?;
    let legacy_id = child(&legacy, "a")?.get();
    let legacy_snapshot = snapshot(&f, legacy_id).await?;
    assert_eq!(
        legacy_snapshot.executable.as_deref(),
        Some("/usr/bin/echo ")
    );

    let mut body = job_write();
    let queue: Uuid = sqlx::query_scalar("SELECT queue_id FROM crono.jobs WHERE id = $1")
        .bind(job.get())
        .fetch_one(&f.pool)
        .await?;
    set(&mut body, "queue_id", json!(queue))?;
    let path = format!("/api/jobs/{}", job.get());
    let response = write(&router, &token, "PUT", &path, body.clone()).await?;
    assert_eq!(field(&response, "name")?, "echo");
    assert_eq!(field(&response, "executable")?, "/usr/bin/echo");
    assert_eq!(field(&response, "arguments")?, field(&body, "arguments")?);
    assert_eq!(field(&response, "inputs")?, field(&body, "inputs")?);
    let started = f.start(&graph).await?;
    let current = snapshot(&f, child(&started, "a")?.get()).await?;
    assert_eq!(current.executable.as_deref(), Some("/usr/bin/echo"));
    assert_eq!(current.arguments, [" literal ", " value "]);
    assert_eq!(current.argument_templates, [" literal ", "{{message}}"]);
    assert_eq!(current.inputs.get("message"), Some(&json!(" value ")));
    assert_eq!(snapshot(&f, legacy_id).await?, legacy_snapshot);

    set(&mut body, "name", json!(" \tshell\n"))?;
    set(&mut body, "executor", json!("shell"))?;
    set(&mut body, "executable", json!(" \t/bin/sh\u{a0}"))?;
    let script = "\n  printf '%s' \"$1\"  \n";
    set(&mut body, "shell_command", json!(script))?;
    let created = write(
        &router,
        &token,
        "POST",
        &format!("/api/namespaces/{}/jobs", f.namespace_id.get()),
        body.clone(),
    )
    .await?;
    assert_eq!(field(&created, "executable")?, "/bin/sh");
    assert_eq!(field(&created, "shell_command")?, script);
    assert_eq!(field(&created, "arguments")?, field(&body, "arguments")?);
    assert_eq!(field(&created, "inputs")?, field(&body, "inputs")?);

    for path_value in [" \t\n", "relative ", "/bin/sh\0 "] {
        set(&mut body, "executable", json!(path_value))?;
        let (status, error) = http(&router, Some(&token), "PUT", &path, Some(body.clone())).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error.pointer("/error/field"), Some(&json!("executable")));
    }
    f.cleanup().await
}

#[tokio::test]
async fn namespace_queue_and_target_names_trim_padding_before_validation() -> Result<()> {
    let Some(f) = Fixture::new(&["a"]).await? else {
        return Ok(());
    };
    let (router, token) = fixture_router(&f)?;
    let label = format!("trim-{}", Uuid::now_v7().simple());
    let namespace = write(
        &router,
        &token,
        "POST",
        "/api/namespaces",
        json!({"name":format!(" \t{label}\u{85}")}),
    )
    .await?;
    assert_eq!(field(&namespace, "name")?, &json!(label));
    assert_eq!(
        http(
            &router,
            Some(&token),
            "DELETE",
            &format!("/api/namespaces/{}", id(&namespace)?),
            None
        )
        .await?
        .0,
        StatusCode::NO_CONTENT
    );
    let queue = write(
        &router,
        &token,
        "POST",
        "/api/queues",
        json!({"name":format!(" {label} "),"description":" keep description \n"}),
    )
    .await?;
    assert_eq!(field(&queue, "name")?, &json!(label));
    assert_eq!(field(&queue, "description")?, " keep description \n");
    let queue_path = format!("/api/queues/{}", id(&queue)?);
    let updated = write(&router, &token, "PUT", &queue_path, json!({"name":format!(" {label}-edit "),"description":" another description ","enabled":true})).await?;
    assert_eq!(field(&updated, "name")?, &json!(format!("{label}-edit")));
    assert_eq!(field(&updated, "description")?, " another description ");
    assert_eq!(
        http(&router, Some(&token), "DELETE", &queue_path, None)
            .await?
            .0,
        StatusCode::NO_CONTENT
    );
    let targets = format!("/api/namespaces/{}/targets", f.namespace_id.get());
    let target_body = json!({"name":format!(" {} ", "a".repeat(63)),"arguments":[" target "],"inputs":{"value":" target value "}});
    let target = write(&router, &token, "POST", &targets, target_body.clone()).await?;
    assert_eq!(field(&target, "name")?, &json!("a".repeat(63)));
    assert_eq!(
        field(&target, "arguments")?,
        field(&target_body, "arguments")?
    );
    assert_eq!(field(&target, "inputs")?, field(&target_body, "inputs")?);
    let target_id = id(&target)?;
    let updated = write(
        &router,
        &token,
        "PUT",
        &format!("/api/targets/{target_id}"),
        json!({"name":" edited-target ","arguments":[" unchanged "],"inputs":{}}),
    )
    .await?;
    assert_eq!(field(&updated, "name")?, "edited-target");
    let sets = format!("/api/namespaces/{}/target-sets", f.namespace_id.get());
    let set = write(
        &router,
        &token,
        "POST",
        &sets,
        json!({"name":" set ","target_ids":[target_id],"inputs":{"value":" set value "}}),
    )
    .await?;
    assert_eq!(field(&set, "name")?, "set");
    let updated = write(
        &router,
        &token,
        "PUT",
        &format!("/api/target-sets/{}", id(&set)?),
        json!({"name":" edited-set ","target_ids":[target_id],"inputs":{}}),
    )
    .await?;
    assert_eq!(field(&updated, "name")?, "edited-set");
    for name in [" \t ", " bad name ", " UPPER ", " -bad "] {
        let (status, _) = http(
            &router,
            Some(&token),
            "POST",
            &targets,
            Some(json!({"name":name,"arguments":[],"inputs":{}})),
        )
        .await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    f.cleanup().await
}

#[tokio::test]
async fn workflow_names_and_endpoints_normalize_before_collision_and_cycle_checks() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let (router, token) = fixture_router(&f)?;
    let body = json!({"name":" padded-flow ","description":" keep this description ",
        "nodes":f.jobs.iter().map(|(name,id)| json!({"name":format!(" {name} "),"job_id":id.get()})).collect::<Vec<_>>(),
        "edges":[{"from":" a ","to":" b ","condition":"success"}]});
    let collection = format!("/api/namespaces/{}/workflows", f.namespace_id.get());
    let created = write(&router, &token, "POST", &collection, body.clone()).await?;
    assert_eq!(field(&created, "name")?, "padded-flow");
    assert_eq!(
        field(&created, "description")?,
        field(&body, "description")?
    );
    assert_eq!(created.pointer("/edges/0/from"), Some(&json!("a")));
    assert_eq!(created.pointer("/edges/0/to"), Some(&json!("b")));
    let mut updated_body = body.clone();
    set(
        &mut updated_body,
        "revision",
        field(&created, "revision")?.clone(),
    )?;
    set(&mut updated_body, "name", json!(" edited-flow "))?;
    let updated = write(
        &router,
        &token,
        "PUT",
        &format!("/api/workflows/{}", id(&created)?),
        updated_body,
    )
    .await?;
    assert_eq!(field(&updated, "name")?, "edited-flow");
    let first = f
        .jobs
        .first()
        .ok_or_else(|| anyhow!("missing Job"))?
        .1
        .get();
    let duplicate = json!({"name":" invalid-flow ","nodes":[{"name":" a ","job_id":first},{"name":"a","job_id":first}],"edges":[]});
    let (status, error) = http(&router, Some(&token), "POST", &collection, Some(duplicate)).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error.to_string().contains("unique"));
    let mut cycle = body;
    set(
        &mut cycle,
        "edges",
        json!([{"from":" a ","to":"b","condition":"success"},{"from":"b","to":" a ","condition":"always"}]),
    )?;
    let (status, error) = http(&router, Some(&token), "POST", &collection, Some(cycle)).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error.to_string().contains("cycles"));
    f.cleanup().await
}

#[tokio::test]
async fn schedule_writes_trim_timing_values_but_preserve_invocation_inputs() -> Result<()> {
    let Some(f) = Fixture::new(&["a"]).await? else {
        return Ok(());
    };
    let (router, token) = fixture_router(&f)?;
    let job = f
        .jobs
        .first()
        .ok_or_else(|| anyhow!("missing Job"))?
        .1
        .get();
    let mut body = json!({"name":" padded-cron ","job_id":job,"target":{"kind":"target","id":f.target.get()},
        "inputs":{"value":" invocation value "},"cron_expression":" \t0  0 * * * \n","execute_at":null,"timezone":" UTC\u{a0}",
        "misfire_policy":"run_late","misfire_grace_seconds":null});
    let collection = format!("/api/namespaces/{}/schedules", f.namespace_id.get());
    let cron = write(&router, &token, "POST", &collection, body.clone()).await?;
    assert_eq!(field(&cron, "name")?, "padded-cron");
    assert_eq!(field(&cron, "cron_expression")?, "0  0 * * *");
    assert_eq!(field(&cron, "timezone")?, "UTC");
    assert_eq!(field(&cron, "inputs")?, field(&body, "inputs")?);
    set(&mut body, "name", json!(" padded-once "))?;
    set(&mut body, "cron_expression", Value::Null)?;
    set(&mut body, "execute_at", json!(" \t2030-01-01T00:00:00Z\n"))?;
    let once = write(&router, &token, "POST", &collection, body.clone()).await?;
    assert_eq!(field(&once, "name")?, "padded-once");
    assert_eq!(field(&once, "execute_at")?, "2030-01-01T00:00:00Z");
    assert_eq!(field(&once, "inputs")?, field(&body, "inputs")?);
    set(&mut body, "execute_at", json!(" \t "))?;
    let (status, error) = http(&router, Some(&token), "POST", &collection, Some(body)).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error.pointer("/error/field"), Some(&json!("execute_at")));
    // This fixture normally has no Schedules; delete only these test-created rows.
    sqlx::query("DELETE FROM crono.schedules WHERE namespace_id = $1")
        .bind(f.namespace_id.get())
        .execute(&f.pool)
        .await?;
    f.cleanup().await
}
