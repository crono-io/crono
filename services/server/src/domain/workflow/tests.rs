//! Pure graph and AND-join tests cover invalid definitions before PostgreSQL writes.

use super::{
    DependencyCondition as Condition, DependencyDecision as Decision, WorkflowDefinition,
    WorkflowEdge, WorkflowNode, WorkflowNodeState as State, dependency_decision,
};
use crate::domain::{JobId, ResourceName};
use anyhow::Result;
use uuid::Uuid;

fn graph(edges: &[(&str, &str, Condition)]) -> Result<WorkflowDefinition> {
    Ok(WorkflowDefinition {
        name: ResourceName::parse("flow")?,
        description: None,
        nodes: ["a", "b", "c"]
            .into_iter()
            .map(|name| {
                Ok(WorkflowNode {
                    name: ResourceName::parse(name)?,
                    job_id: JobId::new(Uuid::now_v7()),
                })
            })
            .collect::<Result<_>>()?,
        edges: edges
            .iter()
            .map(|(from, to, condition)| {
                Ok(WorkflowEdge {
                    from: ResourceName::parse(from)?,
                    to: ResourceName::parse(to)?,
                    condition: *condition,
                })
            })
            .collect::<Result<_>>()?,
    })
}

#[test]
fn cycles_self_edges_missing_nodes_and_duplicate_pairs_are_rejected() -> Result<()> {
    for edges in [
        vec![
            ("a", "b", Condition::Success),
            ("b", "c", Condition::Success),
            ("c", "a", Condition::Failure),
        ],
        vec![("a", "a", Condition::Always)],
        vec![("missing", "b", Condition::Success)],
        vec![
            ("a", "b", Condition::Success),
            ("a", "b", Condition::Failure),
        ],
    ] {
        assert!(graph(&edges)?.validate().is_err());
    }
    let mut duplicate = graph(&[])?;
    let node = duplicate
        .nodes
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing fixture node"))?
        .clone();
    duplicate.nodes.push(node);
    assert!(duplicate.validate().is_err());
    assert!(
        graph(&[
            ("a", "b", Condition::Success),
            ("a", "c", Condition::Failure)
        ])?
        .validate()
        .is_ok()
    );
    Ok(())
}

#[test]
fn and_joins_wait_and_terminal_mismatches_propagate_skips() {
    assert_eq!(dependency_decision([]), Decision::Ready);
    assert_eq!(
        dependency_decision([
            (Condition::Success, State::Succeeded),
            (Condition::Success, State::Running)
        ]),
        Decision::Waiting
    );
    assert_eq!(
        dependency_decision([
            (Condition::Success, State::Succeeded),
            (Condition::Success, State::Succeeded)
        ]),
        Decision::Ready
    );
    assert_eq!(
        dependency_decision([
            (Condition::Success, State::Failed),
            (Condition::Success, State::Pending)
        ]),
        Decision::Skipped
    );
    assert_eq!(
        dependency_decision([(Condition::Success, State::Skipped)]),
        Decision::Skipped
    );
    assert_eq!(
        dependency_decision([(Condition::Failure, State::Unknown)]),
        Decision::Ready
    );
    assert_eq!(
        dependency_decision([(Condition::Failure, State::Succeeded)]),
        Decision::Skipped
    );
    for state in [
        State::Succeeded,
        State::Failed,
        State::Skipped,
        State::Cancelled,
        State::Unknown,
    ] {
        assert_eq!(
            dependency_decision([(Condition::Always, state)]),
            Decision::Ready
        );
    }
}
