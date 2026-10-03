//! Missing authority denies before lookup; missing references preserve existing resources.

use super::*;

#[tokio::test]
async fn target_set_update_without_authority_does_not_reveal_whether_the_set_exists() -> Result<()>
{
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    let router = f.grants_router(&GrantSet::default())?;
    let before = f.store.get_target_set(f.a.target_set).await?;
    for id in [f.a.target_set.get(), Uuid::now_v7()] {
        let (status, error) = request(
            &router,
            "PUT",
            &format!("/api/target-sets/{id}"),
            Some(json!({
                "name":"set", "target_ids":[f.a.target.get()], "inputs":{}
            })),
        )
        .await?;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(error.pointer("/error/code"), Some(&json!("forbidden")));
    }
    assert_eq!(f.store.get_target_set(f.a.target_set).await?, before);
    f.cleanup().await
}

#[tokio::test]
async fn unknown_workload_references_do_not_make_their_namespace_unavailable() -> Result<()> {
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    let router = f.grants_router(&GrantSet::development())?;
    for (resource, payload) in [
        (
            "workflows",
            json!({"name":"missing-job", "nodes":[{"name":"node", "job_id":Uuid::now_v7()}], "edges":[]}),
        ),
        (
            "schedules",
            json!({"name":"missing-job", "job_id":Uuid::now_v7(),
            "target":{"kind":"target","id":f.a.target.get()}, "cron_expression":"0 * * * *", "misfire_policy":"run_late"}),
        ),
    ] {
        let (status, error) = request(
            &router,
            "POST",
            &format!("/api/namespaces/{}/{resource}", f.a.namespace.get()),
            Some(payload),
        )
        .await?;
        // A referenced Job can be missing even when the parent Namespace exists.
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error.pointer("/error/code"), Some(&json!("not_found")));
        assert_eq!(
            request(
                &router,
                "GET",
                &format!("/api/namespaces/{}", f.a.namespace.get()),
                None
            )
            .await?
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(f.intent_counts().await?, (0, 0, 0));
    f.cleanup().await
}
