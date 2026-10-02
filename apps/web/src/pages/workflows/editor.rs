//! Reactive draft state for the structured Workflow form.
//!
//! Temporary row UUIDs keep selection and arrows attached while names change.
//! Only canonical names and Job UUIDs are submitted. Client checks cover obvious
//! input mistakes; cycles and ownership remain server decisions. Removing a Job
//! removes its incident draft dependencies explicitly, never saved history.

use super::graph::{GraphEdge, GraphNode};
use crate::components::{ResourceFeedback, ResourceOption, name_validation_message};
use crono_api::{
    CreateWorkflowRequest, DependencyCondition, WorkflowEdgeRequest, WorkflowNodeRequest,
    WorkflowResource,
};
use leptos::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// A stable row key is independent of its editable canonical node name.
#[derive(Clone, Copy)]
pub(super) struct JobRow {
    pub key: Uuid,
    pub name: RwSignal<String>,
    pub job: RwSignal<Option<Uuid>>,
}

impl JobRow {
    /// Start with an empty reference; no Job is created or copied here.
    pub fn empty() -> Self {
        Self {
            key: Uuid::now_v7(),
            name: RwSignal::new(String::new()),
            job: RwSignal::new(None),
        }
    }
}

/// Endpoints refer to row identities until request construction resolves names.
#[derive(Clone, Copy)]
pub(super) struct EdgeRow {
    pub key: Uuid,
    pub from: RwSignal<Option<Uuid>>,
    pub to: RwSignal<Option<Uuid>>,
    pub condition: RwSignal<DependencyCondition>,
}

impl EdgeRow {
    /// Only the three API-supported conditions are represented in the draft.
    pub fn empty() -> Self {
        Self {
            key: Uuid::now_v7(),
            from: RwSignal::new(None),
            to: RwSignal::new(None),
            condition: RwSignal::new(DependencyCondition::Success),
        }
    }
}

/// Page-owned state survives save errors and feedback dialogs.
#[derive(Clone, Copy)]
pub(super) struct Editor {
    pub edit_id: Option<Uuid>,
    pub namespace: RwSignal<Option<Uuid>>,
    pub namespace_label: StoredValue<String>,
    pub name: RwSignal<String>,
    pub description: RwSignal<String>,
    pub nodes: RwSignal<Vec<JobRow>>,
    pub edges: RwSignal<Vec<EdgeRow>>,
    pub revision: RwSignal<u64>,
    pub attempted: RwSignal<bool>,
    pub busy: RwSignal<bool>,
    pub error: RwSignal<Option<String>>,
    pub feedback: RwSignal<Option<ResourceFeedback>>,
    pub saved_id: RwSignal<Option<Uuid>>,
    pub saved: RwSignal<Option<CreateWorkflowRequest>>,
}

impl Editor {
    /// Restore exactly the editable API definition and retain its update revision.
    pub fn new(initial: Option<&WorkflowResource>) -> Self {
        let nodes: Vec<JobRow> = initial.map_or_else(
            || vec![JobRow::empty()],
            |workflow| {
                workflow
                    .nodes
                    .iter()
                    .map(|node| JobRow {
                        key: node.id,
                        name: RwSignal::new(node.name.clone()),
                        job: RwSignal::new(Some(node.job_id)),
                    })
                    .collect()
            },
        );
        let edges = initial.map_or_else(Vec::new, |workflow| {
            workflow
                .edges
                .iter()
                .map(|edge| EdgeRow {
                    key: Uuid::now_v7(),
                    from: RwSignal::new(
                        nodes
                            .iter()
                            .find(|node| node.name.get_untracked() == edge.from)
                            .map(|node| node.key),
                    ),
                    to: RwSignal::new(
                        nodes
                            .iter()
                            .find(|node| node.name.get_untracked() == edge.to)
                            .map(|node| node.key),
                    ),
                    condition: RwSignal::new(edge.condition),
                })
                .collect()
        });
        let saved = initial.map(|workflow| CreateWorkflowRequest {
            name: workflow.name.clone(),
            description: workflow.description.clone(),
            nodes: workflow
                .nodes
                .iter()
                .map(|node| WorkflowNodeRequest {
                    name: node.name.clone(),
                    job_id: node.job_id,
                })
                .collect(),
            edges: workflow.edges.clone(),
        });
        Self {
            edit_id: initial.map(|workflow| workflow.id),
            namespace: RwSignal::new(initial.map(|workflow| workflow.namespace_id)),
            namespace_label: StoredValue::new(
                initial.map_or_else(String::new, |workflow| workflow.namespace.clone()),
            ),
            name: RwSignal::new(initial.map_or_else(String::new, |workflow| workflow.name.clone())),
            description: RwSignal::new(
                initial
                    .and_then(|workflow| workflow.description.clone())
                    .unwrap_or_default(),
            ),
            nodes: RwSignal::new(nodes),
            edges: RwSignal::new(edges),
            revision: RwSignal::new(initial.map_or(0, |workflow| workflow.revision)),
            attempted: RwSignal::new(false),
            busy: RwSignal::new(false),
            error: RwSignal::new(None),
            feedback: RwSignal::new(None),
            saved_id: RwSignal::new(initial.map(|workflow| workflow.id)),
            saved: RwSignal::new(saved),
        }
    }

    /// Install a successful authorized reload under the existing form owner.
    /// Caller confirmation permits replacement; failures must never call this method.
    /// Matching node IDs reuse their signals so keyed controls and request data agree.
    pub fn replace(self, workflow: &WorkflowResource) {
        let loaded = Self::new(Some(workflow));
        let existing = self.nodes.get_untracked();
        let nodes = loaded
            .nodes
            .get_untracked()
            .into_iter()
            .map(|loaded| {
                if let Some(row) = existing.iter().find(|row| row.key == loaded.key) {
                    row.name.set(loaded.name.get_untracked());
                    row.job.set(loaded.job.get_untracked());
                    *row
                } else {
                    loaded
                }
            })
            .collect();
        self.namespace.set(loaded.namespace.get_untracked());
        self.name.set(loaded.name.get_untracked());
        self.description.set(loaded.description.get_untracked());
        self.nodes.set(nodes);
        self.edges.set(loaded.edges.get_untracked());
        self.revision.set(workflow.revision);
        self.saved.set(loaded.saved.get_untracked());
        self.saved_id.set(Some(workflow.id));
        self.attempted.set(false);
        self.error.set(None);
        self.feedback.set(None);
    }

    /// Validate names, references, and obvious edge errors without checking cycles.
    pub fn request(self) -> Result<CreateWorkflowRequest, String> {
        let name = self.name.get();
        if let Some(error) = name_validation_message(&name, true) {
            return Err(error);
        }
        if self.namespace.get().is_none() {
            return Err("Select a Namespace.".to_string());
        }
        let rows = self.nodes.get();
        if rows.is_empty() || rows.len() > 64 {
            return Err("Choose 1–64 Jobs for this Workflow.".to_string());
        }
        let mut names = BTreeMap::new();
        let mut unique = BTreeSet::new();
        let mut nodes = Vec::new();
        for row in rows {
            let node_name = row.name.get();
            if let Some(error) = name_validation_message(&node_name, true) {
                return Err(format!("Job node name: {error}"));
            }
            if !unique.insert(node_name.clone()) {
                return Err("Job node names must be unique within the Workflow.".to_string());
            }
            let job_id = row
                .job
                .get()
                .ok_or("Choose an existing Job for every node.")?;
            names.insert(row.key, node_name.clone());
            nodes.push(WorkflowNodeRequest {
                name: node_name,
                job_id,
            });
        }
        let edges = request_edges(self.edges.get(), &names)?;
        let description = self.description.get();
        if description.chars().count() > 500 {
            return Err("Description must be at most 500 characters.".to_string());
        }
        Ok(CreateWorkflowRequest {
            name,
            description: (!description.is_empty()).then_some(description),
            nodes,
            edges,
        })
    }

    /// Remove only draft data, including dependencies pointing at this exact row.
    pub fn remove_node(self, key: Uuid) {
        self.nodes
            .update(|nodes| nodes.retain(|node| node.key != key));
        self.edges.update(|edges| {
            edges.retain(|edge| {
                edge.from.get_untracked() != Some(key) && edge.to.get_untracked() != Some(key)
            });
        });
    }

    /// Changing Namespace preserves names but clears every incompatible Job UUID.
    pub fn clear_jobs(self) {
        for row in self.nodes.get_untracked() {
            row.job.set(None);
        }
    }

    /// Build a visualization from draft identities; no execution metadata is fabricated.
    pub fn graph_nodes(self, jobs: &[ResourceOption]) -> Vec<GraphNode> {
        self.nodes
            .get()
            .into_iter()
            .map(|row| GraphNode {
                key: row.key.to_string(),
                name: row.name.get(),
                job: jobs
                    .iter()
                    .find(|job| Some(job.id) == row.job.get())
                    .map_or_else(|| "Select a Job".to_string(), |job| job.label.clone()),
                state: None,
                runs: Vec::new(),
            })
            .collect()
    }

    /// Missing draft endpoints are omitted visually and still rejected on submission.
    pub fn graph_edges(self) -> Vec<GraphEdge> {
        self.edges
            .get()
            .into_iter()
            .filter_map(|edge| {
                Some(GraphEdge {
                    from: edge.from.get()?.to_string(),
                    to: edge.to.get()?.to_string(),
                    condition: edge.condition.get(),
                })
            })
            .collect()
    }
}

/// Reject self edges and repeated endpoint pairs, including differently conditioned duplicates.
fn request_edges(
    rows: Vec<EdgeRow>,
    names: &BTreeMap<Uuid, String>,
) -> Result<Vec<WorkflowEdgeRequest>, String> {
    if rows.len() > 256 {
        return Err("A Workflow supports at most 256 dependencies.".to_string());
    }
    let mut pairs = BTreeSet::new();
    rows.into_iter()
        .map(|row| {
            let from = row
                .from
                .get()
                .and_then(|id| names.get(&id).cloned())
                .ok_or("Choose a From Job for every dependency.")?;
            let to = row
                .to
                .get()
                .and_then(|id| names.get(&id).cloned())
                .ok_or("Choose a To Job for every dependency.")?;
            if from == to {
                return Err("A Job cannot depend on itself.".to_string());
            }
            if !pairs.insert((from.clone(), to.clone())) {
                return Err(
                    "These Jobs already have a dependency. Each From/To pair must be unique."
                        .to_string(),
                );
            }
            Ok(WorkflowEdgeRequest {
                from,
                to,
                condition: row.condition.get(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{EdgeRow, Editor, JobRow};
    use crono_api::DependencyCondition;
    use leptos::prelude::*;
    use uuid::Uuid;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn nodes_edges_self_dependencies_and_safe_removal_use_stable_row_identities() {
        let owner = Owner::new();
        owner.with(|| {
            untrack(|| {
                let draft = Editor::new(None);
                draft.name.set("upgrade".to_string());
                draft.namespace.set(Some(Uuid::now_v7()));
                let first = draft.nodes.get().first().copied();
                assert!(first.is_some());
                let Some(first) = first else {
                    return;
                };
                first.name.set("backup".to_string());
                first.job.set(Some(Uuid::now_v7()));
                draft.description.set("é".repeat(500));
                assert!(
                    draft.request().is_ok(),
                    "Server accepts 500 Unicode characters"
                );
                draft.description.set("é".repeat(501));
                assert!(draft.request().is_err());
                draft.description.set(String::new());
                let second = JobRow::empty();
                second.name.set("upgrade".to_string());
                second.job.set(Some(Uuid::now_v7()));
                draft.nodes.update(|rows| rows.push(second));
                let edge = EdgeRow::empty();
                edge.from.set(Some(first.key));
                edge.to.set(Some(first.key));
                draft.edges.update(|rows| rows.push(edge));
                assert!(draft.request().is_err_and(|error| error.contains("itself")));
                edge.to.set(Some(second.key));
                edge.condition.set(DependencyCondition::Always);
                assert!(
                    draft
                        .request()
                        .is_ok_and(|request| request.nodes.len() == 2 && request.edges.len() == 1)
                );
                let duplicate = EdgeRow::empty();
                duplicate.from.set(Some(first.key));
                duplicate.to.set(Some(second.key));
                draft.edges.update(|rows| rows.push(duplicate));
                assert!(draft.request().is_err_and(|error| error.contains("unique")));
                draft.remove_node(first.key);
                assert_eq!(draft.nodes.get().len(), 1);
                assert!(draft.edges.get().is_empty());
                draft.clear_jobs();
                assert!(second.job.get().is_none());
            });
        });
    }
}
