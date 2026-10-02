//! Bounded acyclic graphs decide when existing Jobs may create ordinary Runs.
//!
//! Definitions contain references, never executable Job copies. Launch snapshots
//! preserve the graph and execution context independently of later catalog edits.
//! Dependencies use AND joins: a terminal mismatch skips a node, and a waiting
//! predecessor delays it. Skips propagate through the same rules; Always accepts
//! every terminal disposition. This layer has no transport, policy, or dispatch.

use super::{JobId, ResourceName};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A typed predecessor outcome, with no expression or scripting language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DependencyCondition {
    Success,
    Failure,
    Always,
}

/// One executable Job reference in a definition; names address edges locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowNode {
    pub name: ResourceName,
    pub job_id: JobId,
}

/// An AND dependency between two named nodes in the same graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowEdge {
    pub from: ResourceName,
    pub to: ResourceName,
    pub condition: DependencyCondition,
}

/// Editable catalog definition. Persistence separately verifies Job ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinition {
    pub name: ResourceName,
    pub description: Option<String>,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
}

impl WorkflowDefinition {
    /// Validate bounds, unique canonical identities, edge endpoints, and acyclicity.
    ///
    /// Kahn's algorithm visits every node and edge, including disconnected roots.
    /// Even differently conditioned duplicate endpoint pairs are rejected: their
    /// AND semantics would otherwise make mutually exclusive branches confusing.
    /// Job existence/Namespace checks belong to the transactional repository.
    ///
    /// # Errors
    /// Returns a safe explanation before any invalid graph can be persisted.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.nodes.is_empty() || self.nodes.len() > 64 || self.edges.len() > 256 {
            return Err("a Workflow requires 1–64 nodes and at most 256 edges");
        }
        if self
            .description
            .as_ref()
            .is_some_and(|value| value.contains('\0') || value.chars().count() > 500)
        {
            return Err("description must be NUL-free and at most 500 characters");
        }
        let mut incoming = BTreeMap::new();
        for node in &self.nodes {
            if incoming.insert(node.name.as_str(), 0_usize).is_some() {
                return Err("node names must be unique within the Workflow");
            }
        }
        let mut pairs = BTreeSet::new();
        let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for edge in &self.edges {
            let from = edge.from.as_str();
            let to = edge.to.as_str();
            if from == to || !incoming.contains_key(from) || !incoming.contains_key(to) {
                return Err("edges must reference distinct existing nodes in this Workflow");
            }
            if !pairs.insert((from, to)) {
                return Err("duplicate dependency endpoints are not allowed");
            }
            if let Some(count) = incoming.get_mut(to) {
                *count += 1;
            }
            outgoing.entry(from).or_default().push(to);
        }
        let mut ready: VecDeque<&str> = incoming
            .iter()
            .filter_map(|(name, count)| (*count == 0).then_some(*name))
            .collect();
        let mut visited = 0;
        while let Some(node) = ready.pop_front() {
            visited += 1;
            for downstream in outgoing.get(node).into_iter().flatten() {
                if let Some(count) = incoming.get_mut(downstream) {
                    *count -= 1;
                    if *count == 0 {
                        ready.push_back(downstream);
                    }
                }
            }
        }
        if visited != self.nodes.len() {
            return Err("Workflow dependencies must not contain cycles");
        }
        Ok(())
    }
}

/// Durable node disposition; Unknown preserves ambiguous ordinary Run outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowNodeState {
    Pending,
    Ready,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
    Unknown,
}

impl WorkflowNodeState {
    /// Only terminal predecessors may satisfy or invalidate dependencies.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Pending | Self::Ready | Self::Running)
    }
}

/// Overall result is derived only after every node is terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Side-effect-free dependency decision for a single downstream node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyDecision {
    Waiting,
    Ready,
    Skipped,
}

/// Evaluate an AND join without trusting client-supplied execution states.
///
/// Any terminal mismatch makes the node impossible even while another edge
/// waits. Empty incoming dependencies identify roots. Failure includes unknown
/// outcomes; Skipped/Cancelled satisfy only Always and cannot claim success.
#[must_use]
pub fn dependency_decision(
    incoming: impl IntoIterator<Item = (DependencyCondition, WorkflowNodeState)>,
) -> DependencyDecision {
    let mut waiting = false;
    for (condition, state) in incoming {
        if !state.is_terminal() {
            waiting = true;
            continue;
        }
        let matches = match condition {
            DependencyCondition::Success => state == WorkflowNodeState::Succeeded,
            DependencyCondition::Failure => matches!(
                state,
                WorkflowNodeState::Failed | WorkflowNodeState::Unknown
            ),
            DependencyCondition::Always => true,
        };
        if !matches {
            return DependencyDecision::Skipped;
        }
    }
    if waiting {
        DependencyDecision::Waiting
    } else {
        DependencyDecision::Ready
    }
}

#[cfg(test)]
mod tests;
