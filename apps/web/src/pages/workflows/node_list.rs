//! Accessible node progress, branch explanations, and ordinary child Run links.
//!
//! Counts use actual node states. Skips are described from persisted predecessor
//! results only after the server has skipped a node; this is explanatory text,
//! never a second dependency evaluator. Every Target Set child Run stays reachable.

use super::{PANEL_CLASS, StateBadge, condition_label, duration, node_state_label};
use crate::navigation::run_details_path;
use crono_api::{
    DependencyCondition, WorkflowNodeRunResource, WorkflowNodeRunState, WorkflowRunResource,
};
use leptos::prelude::*;
use leptos_router::components::A;

/// Summarize real node dispositions, including expected skipped branches.
#[component]
pub(super) fn NodeSummary(nodes: Vec<WorkflowNodeRunResource>) -> impl IntoView {
    let total = nodes.len();
    let statuses = nodes.into_iter().map(|node| node.state).collect::<Vec<_>>();
    let states = [
        WorkflowNodeRunState::Pending,
        WorkflowNodeRunState::Ready,
        WorkflowNodeRunState::Running,
        WorkflowNodeRunState::Succeeded,
        WorkflowNodeRunState::Failed,
        WorkflowNodeRunState::Skipped,
        WorkflowNodeRunState::Cancelled,
        WorkflowNodeRunState::Unknown,
    ];
    view! { <section class=PANEL_CLASS aria-label="Node status summary"><h2 class="font-semibold">{format!("{total} Nodes")}</h2><ul class="flex flex-wrap gap-x-6 gap-y-2">{states.into_iter().filter_map(|state| {
        let count = statuses.iter().filter(|actual| **actual == state).count();
        (count > 0).then(|| view! { <li class="flex items-center gap-2"><span class="text-sm font-semibold">{count}</span><StateBadge state /></li> })
    }).collect_view()}</ul></section> }
}

/// All child Runs are linked; detailed output remains on the ordinary Run pages.
#[component]
pub(super) fn NodeExecutions(value: WorkflowRunResource) -> impl IntoView {
    let notes = value
        .nodes
        .iter()
        .map(|node| {
            (node.state == WorkflowNodeRunState::Skipped).then(|| skip_explanation(&value, node))
        })
        .collect::<Vec<_>>();
    view! { <section class=PANEL_CLASS><h2 class="font-semibold">"Job executions"</h2><p class="text-sm text-crono-muted">"Skipped dependencies are expected branch behavior. Follow a Run for Attempts, timeline and output."</p>
        <div class="overflow-x-auto"><table class="w-full text-left text-sm"><thead class="border-b border-crono-border text-xs text-crono-muted"><tr><th class="p-3">"Node / Job"</th><th class="p-3">"State"</th><th class="p-3">"Started / finished (UTC)"</th><th class="p-3">"Duration"</th><th class="p-3">"Runs"</th></tr></thead><tbody class="divide-y divide-crono-border">{value.nodes.into_iter().zip(notes).map(|(node, note)| {
            view! { <tr><td class="p-3"><p class="font-medium">{node.name.clone()}</p><p class="break-all text-xs text-crono-muted">{node.job_id.to_string()}</p></td><td class="min-w-44 p-3"><StateBadge state=node.state />{note.map(|note| view! { <p class="mt-2 max-w-xs text-xs text-crono-muted">{note}</p> })}</td><td class="whitespace-nowrap p-3"><p>{node.started_at.as_deref().map_or_else(|| "Not started".to_string(), super::super::runs::display_time)}</p><p class="text-xs text-crono-muted">{node.finished_at.as_deref().map_or_else(|| "—".to_string(), super::super::runs::display_time)}</p></td><td class="whitespace-nowrap p-3">{duration(node.started_at.as_deref(), node.finished_at.as_deref())}</td><td class="p-3"><ul class="space-y-2">{node.runs.into_iter().map(|child| view! { <li><A href=run_details_path(child.run_id) attr:class="rounded text-crono-primary hover:underline focus-visible:ring-2 focus-visible:ring-crono-primary">"View Run "{child.run_id.to_string()}</A><p class="break-all text-xs text-crono-muted">{format!("Target {}", child.target_id)}</p></li> }).collect_view()}</ul></td></tr> }
        }).collect_view()}</tbody></table></div>
    </section> }
}

/// Explain a proven terminal mismatch; unknown/failed satisfy Failure, all terminals Always.
fn skip_explanation(value: &WorkflowRunResource, node: &WorkflowNodeRunResource) -> String {
    for edge in value
        .workflow
        .edges
        .iter()
        .filter(|edge| edge.to == node.name)
    {
        let Some(predecessor) = value
            .nodes
            .iter()
            .find(|previous| previous.name == edge.from)
        else {
            continue;
        };
        let terminal = !matches!(
            predecessor.state,
            WorkflowNodeRunState::Pending
                | WorkflowNodeRunState::Ready
                | WorkflowNodeRunState::Running
        );
        let matches = match edge.condition {
            DependencyCondition::Success => predecessor.state == WorkflowNodeRunState::Succeeded,
            DependencyCondition::Failure => matches!(
                predecessor.state,
                WorkflowNodeRunState::Failed | WorkflowNodeRunState::Unknown
            ),
            DependencyCondition::Always => terminal,
        };
        if terminal && !matches {
            return format!(
                "Expected branch skip: {} ended {} and did not match {}.",
                predecessor.name,
                node_state_label(predecessor.state).to_lowercase(),
                condition_label(edge.condition)
            );
        }
    }
    "This node was skipped by normal execution. Follow any associated Run for its outcome."
        .to_string()
}
