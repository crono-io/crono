//! Execution permissions are required before durable intent or schedule changes.

use super::*;
use crono_server::{
    application::CreateScheduleInput,
    domain::{CatchupPolicy, MisfirePolicy, ScheduleTiming},
};

fn schedule(job_id: JobId, target: TargetSelection) -> CreateScheduleInput {
    CreateScheduleInput {
        name: "scheduled".to_owned(),
        job_id,
        target,
        inputs: json!({}),
        timing: ScheduleTiming::Once {
            execute_at: time::OffsetDateTime::now_utc() + time::Duration::days(1),
        },
        misfire_policy: MisfirePolicy::RunLate,
        misfire_grace_seconds: None,
        catchup_policy: CatchupPolicy::Skip,
        max_catchup_runs: 1,
        max_catchup_age_seconds: 3600,
    }
}

fn without(permissions: &[Capability], omitted: Capability) -> Vec<Capability> {
    permissions
        .iter()
        .copied()
        .filter(|permission| *permission != omitted)
        .collect()
}

#[tokio::test]
async fn schedule_creation_and_enabling_require_all_execution_grants_but_disabling_does_not()
-> Result<()> {
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    let permissions = [
        Capability::ScheduleCreate,
        Capability::ScheduleRead,
        Capability::ScheduleUpdate,
        Capability::JobRead,
        Capability::JobExecute,
        Capability::RunCreate,
        Capability::TargetUse,
        Capability::TargetSetUse,
    ];
    let selection = TargetSelection::TargetSet(f.a.target_set);
    for missing in [
        Capability::JobExecute,
        Capability::RunCreate,
        Capability::TargetUse,
        Capability::TargetSetUse,
    ] {
        let caller = context(scoped(f.a.namespace, &without(&permissions, missing))?);
        forbidden(
            f.app
                .create_schedule(&caller, f.a.namespace.get(), schedule(f.a.job, selection))
                .await,
        );
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM crono.schedules WHERE namespace_id = $1")
            .bind(f.a.namespace.get())
            .fetch_one(&f.pool)
            .await?;
    assert_eq!(count, 0);
    assert_eq!(f.intent_counts().await?, (0, 0, 0));

    let admin = context(GrantSet::development());
    let created = f
        .app
        .create_schedule(&admin, f.a.namespace.get(), schedule(f.a.job, selection))
        .await?;
    assert!(created.schedule.enabled);
    let manager = context(scoped(
        f.a.namespace,
        &[Capability::ScheduleRead, Capability::ScheduleUpdate],
    )?);
    let disabled = f
        .app
        .set_schedule_enabled(
            &manager,
            created.schedule.id.get(),
            created.schedule.revision,
            false,
        )
        .await?;
    assert!(!disabled.schedule.enabled);
    for missing in [
        Capability::JobRead,
        Capability::JobExecute,
        Capability::RunCreate,
        Capability::TargetUse,
        Capability::TargetSetUse,
    ] {
        let caller = context(scoped(f.a.namespace, &without(&permissions, missing))?);
        forbidden(
            f.app
                .set_schedule_enabled(
                    &caller,
                    disabled.schedule.id.get(),
                    disabled.schedule.revision,
                    true,
                )
                .await,
        );
        assert_eq!(
            f.store.get_schedule(disabled.schedule.id).await?.schedule,
            disabled.schedule
        );
    }
    let executor = context(scoped(f.a.namespace, &permissions)?);
    let enabled = f
        .app
        .set_schedule_enabled(
            &executor,
            disabled.schedule.id.get(),
            disabled.schedule.revision,
            true,
        )
        .await?;
    assert!(enabled.schedule.enabled);
    assert_eq!(enabled.schedule.revision, disabled.schedule.revision + 1);
    assert_eq!(f.intent_counts().await?, (0, 0, 0));
    f.cleanup().await
}

#[tokio::test]
async fn manual_runs_and_workflow_launches_recheck_all_grants_on_idempotent_replay() -> Result<()> {
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    let permissions = [
        Capability::JobExecute,
        Capability::RunCreate,
        Capability::TargetUse,
        Capability::TargetSetUse,
        Capability::WorkflowExecute,
    ];
    let requests = (Uuid::now_v7(), Uuid::now_v7());
    for missing in [
        Capability::JobExecute,
        Capability::RunCreate,
        Capability::TargetUse,
        Capability::TargetSetUse,
    ] {
        let caller = context(scoped(f.a.namespace, &without(&permissions, missing))?);
        denied_launches(&f, &caller, requests).await;
    }
    let no_workflow = context(scoped(
        f.a.namespace,
        &without(&permissions, Capability::WorkflowExecute),
    )?);
    forbidden(
        f.app
            .start_workflow(
                &no_workflow,
                f.a.workflow.id.get(),
                requests.1,
                TargetSelection::TargetSet(f.a.target_set),
                json!({}),
            )
            .await,
    );
    assert_eq!(f.intent_counts().await?, (0, 0, 0));
    let allowed = context(scoped(f.a.namespace, &permissions)?);
    assert_eq!(launches(&f, &allowed, requests).await?, (true, true));
    let committed = f.intent_counts().await?;
    // One manual request plus the Workflow node's own durable child request.
    assert_eq!(committed, (2, 1, 2));
    for missing in [
        Capability::JobExecute,
        Capability::RunCreate,
        Capability::TargetUse,
        Capability::TargetSetUse,
    ] {
        let caller = context(scoped(f.a.namespace, &without(&permissions, missing))?);
        denied_launches(&f, &caller, requests).await;
    }
    assert_eq!(f.intent_counts().await?, committed);
    assert_eq!(launches(&f, &allowed, requests).await?, (false, false));
    f.cleanup().await
}

async fn denied_launches(f: &Fixture, caller: &RequestContext, requests: (Uuid, Uuid)) {
    let selection = TargetSelection::TargetSet(f.a.target_set);
    forbidden(
        f.app
            .create_run(caller, requests.0, f.a.job.get(), selection, json!({}))
            .await,
    );
    forbidden(
        f.app
            .start_workflow(
                caller,
                f.a.workflow.id.get(),
                requests.1,
                selection,
                json!({}),
            )
            .await,
    );
}

async fn launches(
    f: &Fixture,
    caller: &RequestContext,
    requests: (Uuid, Uuid),
) -> Result<(bool, bool)> {
    let selection = TargetSelection::TargetSet(f.a.target_set);
    Ok((
        f.app
            .create_run(caller, requests.0, f.a.job.get(), selection, json!({}))
            .await?
            .created,
        f.app
            .start_workflow(
                caller,
                f.a.workflow.id.get(),
                requests.1,
                selection,
                json!({}),
            )
            .await?
            .1,
    ))
}

#[tokio::test]
async fn reruns_require_history_read_and_every_underlying_execution_permission() -> Result<()> {
    let Some(f) = Fixture::new().await? else {
        return Ok(());
    };
    let admin = context(GrantSet::development());
    let created = f
        .app
        .create_run(
            &admin,
            Uuid::now_v7(),
            f.a.job.get(),
            TargetSelection::Target(f.a.target),
            json!({}),
        )
        .await?;
    let source = created
        .runs
        .first()
        .ok_or_else(|| anyhow!("missing Run"))?
        .run
        .id();
    // Mark this fixture snapshot terminal without any worker or transport dependency.
    sqlx::query("UPDATE crono.runs SET status = 'succeeded', completed_at = statement_timestamp() WHERE id = $1")
        .bind(source.get()).execute(&f.pool).await?;
    let permissions = [
        Capability::RunRead,
        Capability::JobExecute,
        Capability::TargetUse,
        Capability::RunCreate,
    ];
    let committed = f.intent_counts().await?;
    for missing in [
        Capability::JobExecute,
        Capability::TargetUse,
        Capability::RunCreate,
    ] {
        let caller = context(scoped(f.a.namespace, &without(&permissions, missing))?);
        forbidden(f.app.rerun_run(&caller, source.get(), Uuid::now_v7()).await);
    }
    let no_history = context(scoped(
        f.a.namespace,
        &without(&permissions, Capability::RunRead),
    )?);
    assert!(matches!(
        f.app
            .rerun_run(&no_history, source.get(), Uuid::now_v7())
            .await,
        Err(ApplicationError::Authorization(
            AuthorizationError::NotFound
        ))
    ));
    assert_eq!(f.intent_counts().await?, committed);
    let allowed = context(scoped(f.a.namespace, &permissions)?);
    assert!(
        f.app
            .rerun_run(&allowed, source.get(), Uuid::now_v7())
            .await?
            .created
    );
    f.cleanup().await
}
