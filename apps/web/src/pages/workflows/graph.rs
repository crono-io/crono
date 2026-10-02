//! Lightweight graph presentation: fixed HTML cards and labeled SVG arrows.
//!
//! Callers supply display data from a draft or immutable API snapshot. Layout
//! only determines coordinates; it never validates persistence or fetches Jobs.
//! Structured lists accompany the visual, and scrolling preserves readable cards
//! without widening the app shell. Definition and invocation graphs share this view.

use super::{StateBadge, condition_label};
use crate::components::ResourceOption;
use crate::{
    navigation::run_details_path,
    workflow_layout::{GraphLayout, NODE_HEIGHT, NODE_WIDTH, NodePosition, layered_layout},
};
use crono_api::{
    DependencyCondition, WorkflowNodeRunResource, WorkflowNodeRunState, WorkflowResource,
};
use leptos::prelude::*;
use leptos_router::components::A;
use uuid::Uuid;

/// Stable card identity, plain labels, and only an optional ordinary Run link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GraphNode {
    pub key: String,
    pub name: String,
    pub job: String,
    pub state: Option<WorkflowNodeRunState>,
    pub runs: Vec<Uuid>,
}

/// Endpoints use stable draft/snapshot identities even while node names change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GraphEdge {
    pub from: String,
    pub to: String,
    pub condition: DependencyCondition,
}

/// Resolve display labels only from already-authorized option data.
pub(super) fn definition_nodes(
    workflow: &WorkflowResource,
    jobs: &[ResourceOption],
    states: &[WorkflowNodeRunResource],
) -> Vec<GraphNode> {
    workflow
        .nodes
        .iter()
        .map(|node| {
            let progress = states
                .iter()
                .find(|state| state.workflow_node_id == node.id);
            GraphNode {
                key: node.id.to_string(),
                name: node.name.clone(),
                job: jobs
                    .iter()
                    .find(|job| job.id == node.job_id)
                    .map_or_else(|| format!("Job {}", node.job_id), |job| job.label.clone()),
                state: progress.map(|state| state.state),
                runs: progress
                    .map(|state| state.runs.iter().map(|run| run.run_id).collect())
                    .unwrap_or_default(),
            }
        })
        .collect()
}

/// Translate the definition's canonical names to its snapshot node identities.
pub(super) fn definition_edges(workflow: &WorkflowResource) -> Vec<GraphEdge> {
    workflow
        .edges
        .iter()
        .filter_map(|edge| {
            let from = workflow.nodes.iter().find(|node| node.name == edge.from)?;
            let to = workflow.nodes.iter().find(|node| node.name == edge.to)?;
            Some(GraphEdge {
                from: from.id.to_string(),
                to: to.id.to_string(),
                condition: edge.condition,
            })
        })
        .collect()
}

/// Show all cards at fixed size; the graph is a preview, never an interactive editor.
#[component]
pub(super) fn WorkflowGraph(
    nodes: Signal<Vec<GraphNode>>,
    edges: Signal<Vec<GraphEdge>>,
) -> impl IntoView {
    let arrow_id = Uuid::now_v7().to_string();
    let marker = format!("workflow-arrow-{arrow_id}");
    let arrow = StoredValue::new(format!("url(#{marker})"));
    let layout = Memo::new(move |_| {
        let items = nodes.get();
        let links = edges
            .get()
            .iter()
            .filter_map(|edge| {
                Some((
                    items.iter().position(|node| node.key == edge.from)?,
                    items.iter().position(|node| node.key == edge.to)?,
                ))
            })
            .collect::<Vec<_>>();
        layered_layout(items.len(), &links)
    });
    let viewport = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        center_viewport(viewport, layout.get().width);
    });
    let resize = window_event_listener(leptos::ev::resize, move |_| {
        untrack(move || center_viewport(viewport, layout.get_untracked().width));
    });
    on_cleanup(move || resize.remove());
    view! {
        <figure class="min-w-0 space-y-3" aria-label="Workflow Jobs and dependencies">
            <div node_ref=viewport class="max-h-[42rem] w-full overflow-auto rounded-lg bg-zinc-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary" tabindex="0" aria-label="Workflow preview. Scroll to explore all Jobs.">
                <div class="relative mx-auto" style=move || format!("width:{}px;height:{}px", layout.get().width.max(NODE_WIDTH + 144), layout.get().height)>
                    <svg class="absolute inset-0 overflow-visible" width=move || layout.get().width.to_string() height=move || layout.get().height.to_string() aria-hidden="true">
                        <defs><marker id=marker markerWidth="10" markerHeight="8" refX="9" refY="4" orient="auto" markerUnits="strokeWidth"><path d="M 0 0 L 10 4 L 0 8 z" fill="context-stroke" /></marker></defs>
                        {move || {
                            let items = nodes.get();
                            let positions = layout.get();
                            edges.get().into_iter().enumerate().filter_map(|(index, edge)| {
                                let from = items.iter().position(|node| node.key == edge.from).and_then(|index| positions.nodes.get(index))?;
                                let to = items.iter().position(|node| node.key == edge.to).and_then(|index| positions.nodes.get(index))?;
                                Some(edge_view(&edge, from, to, &positions, index, arrow.get_value()))
                            }).collect_view()
                        }}
                    </svg>
                    <For each=move || nodes.get() key=|node| node.key.clone() children=move |initial| {
                        let key = StoredValue::new(initial.key.clone());
                        let fallback = StoredValue::new(initial);
                        let current = Signal::derive(move || nodes.get().into_iter().find(|node| node.key == key.get_value()).unwrap_or_else(|| fallback.get_value()));
                        let position = Signal::derive(move || {
                            let index = nodes.get().iter().position(|node| node.key == key.get_value());
                            layout.get().nodes.into_iter().find(|position| Some(position.index) == index)
                        });
                        view! { <GraphCard node=current position /> }
                    } />
                </div>
            </div>
            <figcaption class="text-xs text-crono-muted">"Arrows show required predecessor results. All incoming dependencies must match. Jobs on the same level can run in parallel."</figcaption>
            <Show when=move || layout.get().unresolved><p class="text-sm text-crono-muted" role="status">"Some draft dependencies cannot be layered yet. Check the connections; saving validates the complete Workflow."</p></Show>
        </figure>
    }
}

/// Keep centered roots visible on narrow screens without shrinking readable cards.
fn center_viewport(viewport: NodeRef<leptos::html::Div>, width: usize) {
    if let Some(element) = viewport.get() {
        let available = usize::try_from(element.client_width()).unwrap_or_default();
        let offset = width.saturating_sub(available) / 2;
        element.set_scroll_left(i32::try_from(offset).unwrap_or(i32::MAX));
    }
}

/// Route long edges outside intermediate cards and retain explicit condition text.
fn edge_view(
    edge: &GraphEdge,
    from: &NodePosition,
    to: &NodePosition,
    layout: &GraphLayout,
    index: usize,
    arrow: String,
) -> impl IntoView + use<> {
    let (sx, sy, tx, ty) = (
        from.x + NODE_WIDTH / 2,
        from.y + NODE_HEIGHT,
        to.x + NODE_WIDTH / 2,
        to.y,
    );
    let mid = usize::midpoint(sy, ty);
    let (path, label_x, label_y) = if to.depth == from.depth + 1 {
        (
            format!("M {sx} {sy} C {sx} {mid}, {tx} {mid}, {tx} {ty}"),
            usize::midpoint(sx, tx),
            mid.saturating_sub(8),
        )
    } else {
        let lane = layout.width.saturating_sub(18 + (index % 4) * 10);
        (
            format!(
                "M {sx} {sy} V {} H {lane} V {} H {tx} V {ty}",
                sy + 32,
                ty.saturating_sub(32)
            ),
            lane.saturating_sub(42),
            mid,
        )
    };
    let (color, dash) = match edge.condition {
        DependencyCondition::Success => ("#64748b", ""),
        DependencyCondition::Failure => ("#b91c1c", "6 4"),
        DependencyCondition::Always => ("#6366f1", "3 3"),
    };
    view! {
        <g><path d=path fill="none" stroke=color stroke-width="2" stroke-dasharray=dash marker-end=arrow />
            <text x=label_x.to_string() y=label_y.to_string() text-anchor="middle" fill=color stroke="#fafafa" stroke-width="5" paint-order="stroke" class="text-xs font-medium">{condition_label(edge.condition)}</text>
        </g>
    }
}

/// Native links keep ordinary Run output out of the orchestration UI.
#[component]
fn GraphCard(node: Signal<GraphNode>, position: Signal<Option<NodePosition>>) -> impl IntoView {
    let class = Signal::derive(move || match node.get().state {
        Some(WorkflowNodeRunState::Failed | WorkflowNodeRunState::Unknown) => "border-red-200",
        Some(WorkflowNodeRunState::Running) => "border-blue-300",
        Some(WorkflowNodeRunState::Succeeded) => "border-emerald-200",
        _ => "border-crono-border",
    });
    view! {
        <div data-workflow-node=move || node.get().name class=move || format!("absolute flex flex-col justify-between rounded-lg border bg-white p-3 shadow-sm {}", class.get()) style=move || position.get().map(|position| format!("left:{}px;top:{}px;width:{NODE_WIDTH}px;height:{NODE_HEIGHT}px", position.x, position.y)).unwrap_or_default()>
            <div><p class="truncate font-semibold text-crono-text" title=move || node.get().name>{move || { let name = node.get().name; if name.is_empty() { "Unnamed Job".to_string() } else { name } }}</p><p class="truncate text-xs text-crono-muted" title=move || node.get().job>{move || node.get().job}</p></div>
            <div class="flex items-center justify-between gap-2">
                {move || node.get().state.map(|state| view! { <StateBadge state /> })}
                <Show when=move || !node.get().runs.is_empty()><A href=move || node.get().runs.first().copied().map(run_details_path).unwrap_or_default() attr:class="rounded text-xs font-medium text-crono-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary">"View Run"</A></Show>
            </div>
            <Show when=move || { node.get().runs.len() > 1 }><p class="text-xs text-crono-muted">{move || format!("{} target Runs; all links below", node.get().runs.len())}</p></Show>
        </div>
    }
}
