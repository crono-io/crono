//! Read visibility is permission-specific before pagination and aggregation.

use super::*;
use crono_server::application::{ResourceNamespaceResolver, ResourceScope};

#[tokio::test]
async fn two_provider_layouts_keep_permissions_attached_to_their_namespaces() -> Result<()> {
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    let assignments = vec![
        (
            GrantScope::Namespace {
                namespace_id: f.a.namespace,
            },
            vec![Capability::JobExecute],
        ),
        (
            GrantScope::Namespace {
                namespace_id: f.b.namespace,
            },
            vec![Capability::JobRead],
        ),
    ];
    let grants = GrantSet::new(assignments.clone())?;
    for authority in [
        Authority::Assignments(assignments),
        Authority::Document(Some(grants.to_json()?)),
    ] {
        let router = f.router(authority);
        assert_eq!(
            request(
                &router,
                "GET",
                &format!("/api/jobs/{}", f.b.job.get()),
                None
            )
            .await?
            .0,
            StatusCode::OK
        );
        assert_eq!(
            request(
                &router,
                "GET",
                &format!("/api/jobs/{}", f.a.job.get()),
                None
            )
            .await?
            .0,
            StatusCode::FORBIDDEN
        );
        let (status, page) = request(
            &router,
            "GET",
            &format!("/api/namespaces/{}/jobs?limit=1", f.b.namespace.get()),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(items(&page)?.len(), 1);
        let (status, _) = request(
            &router,
            "POST",
            "/api/runs",
            Some(json!({
                "request_id":Uuid::now_v7(), "job_id":f.b.job.get(),
                "target":{"kind":"target","id":f.b.target.get()}, "inputs":{}
            })),
        )
        .await?;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = request(
            &router,
            "POST",
            "/api/runs",
            Some(json!({
                "request_id":Uuid::now_v7(), "job_id":f.b.job.get(),
                "target":{"kind":"target","id":f.b.target.get()}, "inputs":{},
                "grants":{"version":1,"grants":[]}, "roles":["admin"]
            })),
        )
        .await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    assert_eq!(f.intent_counts().await?, (0, 0, 0));
    f.cleanup().await
}

#[tokio::test]
async fn empty_verified_authority_yields_empty_lists_zero_counts_and_global_denials() -> Result<()>
{
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    for document in [None, Some(br#"{"version":1,"grants":[]}"#.to_vec())] {
        let router = f.router(Authority::Document(document));
        for path in ["/api/namespaces?limit=1", "/api/runs?limit=1"] {
            let (status, page) = request(&router, "GET", path, None).await?;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(items(&page)?.len(), 0);
            assert_eq!(page.get("next_cursor"), Some(&Value::Null));
        }
        let (status, overview) = request(&router, "GET", "/api/overview", None).await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            overview,
            json!({"namespaces":0,"jobs":0,"targets":0,"target_sets":0,"schedules":0,"runs":0})
        );
        for path in ["/api/queues", "/api/workers", "/api/monitor"] {
            assert_eq!(
                request(&router, "GET", path, None).await?.0,
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/namespaces",
                Some(json!({"name":"forged-admin"}))
            )
            .await?
            .0,
            StatusCode::FORBIDDEN
        );
    }
    for document in [
        br#"{"version":2,"grants":[]}"#.as_slice(),
        br#"{"version":1,"grants":[{"scope":{"kind":"global"},"permissions":["crono.admin"]}]}"#,
    ] {
        let router = f.router(Authority::Document(Some(document.to_vec())));
        assert_eq!(
            request(&router, "GET", "/api/overview", None).await?.0,
            StatusCode::UNAUTHORIZED
        );
    }
    f.cleanup().await
}

#[tokio::test]
async fn namespace_metadata_and_overview_counts_have_independent_visibility() -> Result<()> {
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    let grants = GrantSet::new([
        (
            GrantScope::Namespace {
                namespace_id: f.a.namespace,
            },
            vec![Capability::NamespaceRead],
        ),
        (
            GrantScope::Namespace {
                namespace_id: f.b.namespace,
            },
            vec![Capability::OverviewRead],
        ),
    ])?;
    let router = f.grants_router(&grants)?;
    let (status, page) = request(&router, "GET", "/api/namespaces?limit=1", None).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items(&page)?.len(), 1);
    assert_eq!(
        items(&page)?.first().and_then(|item| item.get("id")),
        Some(&json!(f.a.namespace.get()))
    );
    assert_eq!(page.get("next_cursor"), Some(&Value::Null));
    let (status, overview) = request(&router, "GET", "/api/overview", None).await?;
    assert_eq!(status, StatusCode::OK);
    // Store-created fixture Namespaces contain exactly the definitions created above.
    assert_eq!(
        overview,
        json!({"namespaces":1,"jobs":1,"targets":1,"target_sets":1,"schedules":0,"runs":0})
    );
    assert_eq!(
        request(
            &router,
            "GET",
            &format!("/api/namespaces/{}", f.b.namespace.get()),
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    for path in [
        format!("/api/jobs/{}", f.b.job.get()),
        format!("/api/targets/{}", f.b.target.get()),
    ] {
        assert_eq!(
            request(&router, "GET", &path, None).await?.0,
            StatusCode::FORBIDDEN
        );
    }
    let only_metadata = f.grants_router(&scoped(f.a.namespace, &[Capability::NamespaceRead])?)?;
    assert_eq!(
        request(&only_metadata, "GET", "/api/overview", None)
            .await?
            .1
            .get("namespaces"),
        Some(&json!(0))
    );
    f.cleanup().await
}

#[tokio::test]
async fn resource_ownership_hides_history_graphs_invocations_and_child_output() -> Result<()> {
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    let a_run = f.run(&f.a).await?;
    let b_run = f.run(&f.b).await?;
    let a_invocation = f.invocation(&f.a).await?;
    let b_invocation = f.invocation(&f.b).await?;
    let grants = scoped(
        f.a.namespace,
        &[
            Capability::RunRead,
            Capability::WorkflowRead,
            Capability::WorkflowRunRead,
        ],
    )?;
    let router = f.grants_router(&grants)?;
    for path in [
        format!("/api/runs/{}", b_run.get()),
        format!("/api/runs/{}/attempts", b_run.get()),
        format!("/api/runs/{}/events", b_run.get()),
        format!("/api/workflows/{}", f.b.workflow.id.get()),
        format!("/api/workflows/{}/runs", f.b.workflow.id.get()),
        format!("/api/workflow-runs/{}", b_invocation.get()),
    ] {
        let (status, error) = request(&router, "GET", &path, None).await?;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(error.pointer("/error/code"), Some(&json!("not_found")));
    }
    assert_eq!(
        request(
            &router,
            "GET",
            &format!("/api/runs/{}/attempts", a_run.get()),
            None
        )
        .await?
        .0,
        StatusCode::OK
    );
    let (_, page) = request(&router, "GET", "/api/runs?limit=1", None).await?;
    assert_eq!(items(&page)?.len(), 1);
    assert_eq!(
        items(&page)?.first().and_then(|item| item.get("namespace")),
        Some(&json!(f.a.workflow.namespace.as_str()))
    );
    let graph_only = f.grants_router(&scoped(f.a.namespace, &[Capability::WorkflowRunRead])?)?;
    assert_eq!(
        request(
            &graph_only,
            "GET",
            &format!("/api/workflow-runs/{}", a_invocation.get()),
            None
        )
        .await?
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &graph_only,
            "GET",
            &format!("/api/runs/{}/attempts", a_run.get()),
            None
        )
        .await?
        .0,
        StatusCode::NOT_FOUND
    );
    f.cleanup().await
}

struct UnavailableResolver;

#[async_trait]
impl ResourceNamespaceResolver for UnavailableResolver {
    async fn namespace_for(
        &self,
        _: &ResourceScope,
    ) -> Result<Option<NamespaceId>, AuthorizationError> {
        Err(AuthorizationError::Unavailable)
    }
}

#[tokio::test]
async fn membership_outages_fail_closed_and_global_grants_do_not_widen_namespace_access()
-> Result<()> {
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    let grants = GrantSet::new([
        (GrantScope::Global, vec![Capability::QueueRead]),
        (
            GrantScope::Namespace {
                namespace_id: f.a.namespace,
            },
            vec![Capability::JobRead],
        ),
    ])?;
    let router = f.grants_router(&grants)?;
    assert_eq!(
        request(&router, "GET", "/api/queues", None).await?.0,
        StatusCode::OK
    );
    for path in ["/api/workers", "/api/monitor"] {
        assert_eq!(
            request(&router, "GET", path, None).await?.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(
            &router,
            "GET",
            &format!("/api/jobs/{}", f.b.job.get()),
            None
        )
        .await?
        .0,
        StatusCode::FORBIDDEN
    );
    let unavailable = build_router(
        Application::new(
            Arc::new(f.store.clone()),
            Arc::new(GrantAuthorizer::new(Arc::new(UnavailableResolver))),
        ),
        Arc::new(f.store.clone()),
        NatsPublisher::new("nats://127.0.0.1:1"),
        Arc::new(Provider(Authority::Document(Some(grants.to_json()?)))),
    );
    assert_eq!(
        request(
            &unavailable,
            "GET",
            &format!("/api/jobs/{}", f.a.job.get()),
            None
        )
        .await?
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(f.intent_counts().await?, (0, 0, 0));
    f.cleanup().await
}
