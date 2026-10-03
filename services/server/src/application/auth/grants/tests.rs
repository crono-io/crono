//! Freeze the external permission vocabulary and reject ambiguous authority.

use super::*;
use anyhow::Result;
use serde_json::json;

fn namespace(value: u128) -> GrantScope {
    GrantScope::Namespace {
        namespace_id: NamespaceId::new(Uuid::from_u128(value)),
    }
}

#[test]
fn scoped_execution_never_crosses_into_a_read_only_namespace() -> Result<()> {
    let grants = GrantSet::new([
        (
            namespace(1),
            vec![Capability::JobRead, Capability::JobExecute],
        ),
        (namespace(2), vec![Capability::JobRead]),
        (GrantScope::Global, vec![Capability::QueueRead]),
    ])?;
    assert!(grants.allows_namespace(Capability::JobExecute, NamespaceId::new(Uuid::from_u128(1))));
    assert!(!grants.allows_namespace(Capability::JobExecute, NamespaceId::new(Uuid::from_u128(2))));
    assert!(!grants.allows_global(Capability::JobExecute));
    assert_eq!(
        grants.visibility(Capability::JobRead),
        VisibilityScope::Namespaces(BTreeSet::from([
            NamespaceId::new(Uuid::from_u128(1)),
            NamespaceId::new(Uuid::from_u128(2))
        ]))
    );
    assert_eq!(
        grants.visibility(Capability::OverviewRead),
        VisibilityScope::None
    );
    Ok(())
}

#[test]
fn all_namespaces_grants_only_the_listed_permissions() -> Result<()> {
    let grants = GrantSet::new([(GrantScope::AllNamespaces, vec![Capability::JobRead])])?;
    assert!(grants.allows_namespace(Capability::JobRead, NamespaceId::new(Uuid::now_v7())));
    assert!(!grants.allows_global(Capability::QueueRead));
    assert!(!grants.allows_namespace(Capability::JobExecute, NamespaceId::new(Uuid::now_v7())));
    assert_eq!(grants.visibility(Capability::JobRead), VisibilityScope::All);
    Ok(())
}

#[test]
fn canonical_round_trip_normalizes_duplicates_without_moving_permissions() -> Result<()> {
    let original = GrantSet::new([
        (namespace(1), vec![Capability::JobRead, Capability::JobRead]),
        (namespace(1), vec![Capability::JobExecute]),
        (namespace(2), vec![Capability::RunRead]),
    ])?;
    let bytes = original.to_json()?;
    assert_eq!(GrantSet::from_verified_json(Some(&bytes))?, original);
    let document: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(
        document
            .pointer("/grants/0/permissions")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(document.get("version"), Some(&json!(1)));
    assert_eq!(GrantSet::from_verified_json(None)?, GrantSet::default());
    assert_eq!(
        GrantSet::from_verified_json(Some(br#"{"version":1,"grants":[]}"#))?,
        GrantSet::default()
    );
    Ok(())
}

#[test]
fn malformed_or_unknown_authority_is_never_partially_accepted() {
    for bytes in [
        br#"{"version":1,"grants":[],"role":"admin"}"#.as_slice(),
        br#"{"version":1}"#,
        br#"{"version":1,"version":1,"grants":[]}"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"global","namespace_id":"00000000-0000-0000-0000-000000000001"},"permissions":[]}] }"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"all_namespaces","namespace_id":null},"permissions":[]}] }"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"global","kind":"global"},"permissions":[]}] }"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"namespace","namespace_id":"00000000-0000-0000-0000-000000000001","namespace_id":"00000000-0000-0000-0000-000000000002"},"permissions":[]}] }"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"namespace","namespace_id":"invalid"},"permissions":[]}] }"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"namespace","namespace_id":"00000000000000000000000000000001"},"permissions":[]}] }"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"all_namespaces"},"permissions":["crono.job.read","*"]}] }"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"global"},"permissions":["crono.job.read"]}] }"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"all_namespaces"},"permissions":["crono.monitor.read"]}] }"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"global"},"permissions":["CRONO.QUEUE.READ"]}] }"#,
        br#"{"version":2,"grants":[]}"#,
        br#"{"version":1,"grants":[{"scope":{"kind":"admin"},"permissions":[]}] }"#,
    ] {
        assert!(GrantSet::from_verified_json(Some(bytes)).is_err(), "accepted invalid fixture: {}", String::from_utf8_lossy(bytes));
    }
}

#[test]
fn size_and_raw_assignment_limits_apply_before_deduplication() -> Result<()> {
    assert_eq!(
        GrantSet::from_verified_json(Some(&vec![b' '; MAX_GRANT_DOCUMENT_BYTES + 1])),
        Err(GrantError::TooLarge)
    );
    let assignment = json!({"scope":{"kind":"global"},"permissions":[]});
    let at_limit = serde_json::to_vec(
        &json!({"version":1,"grants":vec![assignment.clone();MAX_GRANT_ENTRIES]}),
    )?;
    assert!(GrantSet::from_verified_json(Some(&at_limit)).is_ok());
    let excessive =
        serde_json::to_vec(&json!({"version":1,"grants":vec![assignment;MAX_GRANT_ENTRIES + 1]}))?;
    assert_eq!(
        GrantSet::from_verified_json(Some(&excessive)),
        Err(GrantError::TooLarge)
    );
    let grants = GrantSet::new((0..MAX_GRANT_ENTRIES).map(|value| {
        (
            namespace(value as u128),
            ALL_CAPABILITIES
                .iter()
                .copied()
                .filter(|capability| capability.definition().scope == PermissionScope::Namespace)
                .collect(),
        )
    }))?;
    assert_eq!(grants.to_json(), Err(GrantError::TooLarge));
    Ok(())
}

#[test]
fn published_schema_and_registry_preserve_version_one_identifiers() -> Result<()> {
    let expected = [
        "crono.namespace.create",
        "crono.namespace.read",
        "crono.namespace.delete",
        "crono.queue.create",
        "crono.queue.read",
        "crono.queue.update",
        "crono.queue.delete",
        "crono.job.create",
        "crono.job.read",
        "crono.job.update",
        "crono.job.execute",
        "crono.target.create",
        "crono.target.read",
        "crono.target.update",
        "crono.target.delete",
        "crono.target.use",
        "crono.target_set.create",
        "crono.target_set.read",
        "crono.target_set.update",
        "crono.target_set.use",
        "crono.schedule.create",
        "crono.schedule.read",
        "crono.schedule.update",
        "crono.workflow.create",
        "crono.workflow.read",
        "crono.workflow.update",
        "crono.workflow.delete",
        "crono.workflow.execute",
        "crono.workflow_run.read",
        "crono.workflow_run.cancel",
        "crono.run.create",
        "crono.run.read",
        "crono.worker.read",
        "crono.monitor.read",
        "crono.overview.read",
    ];
    assert!(
        ALL_CAPABILITIES
            .iter()
            .map(|capability| capability.definition().identifier)
            .eq(expected)
    );
    let schema: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/authorization/grants-v1.schema.json"
    )))?;
    for (path, scope) in [
        ("globalGrant", PermissionScope::Global),
        ("namespaceGrant", PermissionScope::Namespace),
        ("allNamespacesGrant", PermissionScope::Namespace),
    ] {
        let names = schema
            .pointer(&format!("/$defs/{path}/properties/permissions/items/enum"))
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("schema permissions missing"))?;
        assert!(
            ALL_CAPABILITIES
                .iter()
                .filter(|capability| capability.definition().scope == scope)
                .map(|capability| capability.definition().identifier)
                .eq(names.iter().filter_map(serde_json::Value::as_str))
        );
    }
    Ok(())
}
