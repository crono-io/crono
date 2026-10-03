//! Cancellation returns history only after both read and mutation authorization.

use super::*;
use crono_server::application::VisibilityScope;

#[tokio::test]
async fn cancellation_cannot_disclose_or_modify_an_invocation_without_history_read() -> Result<()> {
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    let running = f.invocation(&f.a).await?;
    let finished = f.invocation(&f.b).await?;
    sqlx::query("UPDATE crono.workflow_runs SET state = 'succeeded', finished_at = statement_timestamp() WHERE id = $1")
        .bind(finished.get()).execute(&f.pool).await?;
    let cancel_only = f.grants_router(&GrantSet::new([
        (
            GrantScope::Namespace {
                namespace_id: f.a.namespace,
            },
            vec![Capability::WorkflowRunCancel],
        ),
        (
            GrantScope::Namespace {
                namespace_id: f.b.namespace,
            },
            vec![Capability::WorkflowRunCancel],
        ),
    ])?)?;
    for id in [running, finished] {
        let before = f.store.get_workflow_run(id, &VisibilityScope::All).await?;
        let (status, error) = request(
            &cancel_only,
            "POST",
            &format!("/api/workflow-runs/{}/cancel", id.get()),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error.pointer("/error/code"), Some(&json!("not_found")));
        assert_eq!(
            f.store.get_workflow_run(id, &VisibilityScope::All).await?,
            before
        );
    }
    let read_only = f.grants_router(&scoped(f.a.namespace, &[Capability::WorkflowRunRead])?)?;
    assert_eq!(
        request(
            &read_only,
            "POST",
            &format!("/api/workflow-runs/{}/cancel", running.get()),
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    assert!(
        !f.store
            .get_workflow_run(running, &VisibilityScope::All)
            .await?
            .cancellation_requested
    );
    let authorized = f.grants_router(&scoped(
        f.a.namespace,
        &[Capability::WorkflowRunRead, Capability::WorkflowRunCancel],
    )?)?;
    assert_eq!(
        request(
            &authorized,
            "POST",
            &format!("/api/workflow-runs/{}/cancel", finished.get()),
            None
        )
        .await?
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, returned) = request(
        &authorized,
        "POST",
        &format!("/api/workflow-runs/{}/cancel", running.get()),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(returned.get("id"), Some(&json!(running.get())));
    assert!(
        f.store
            .get_workflow_run(running, &VisibilityScope::All)
            .await?
            .cancellation_requested
    );
    f.cleanup().await
}
