//! Deterministic presentation coordinates for bounded Workflow definitions.
//!
//! Kahn traversal assigns each Job the greatest predecessor depth, keeping
//! parallel siblings together and joins after every predecessor. Incomplete or
//! cyclic drafts still get a finite preview; this is layout, never authoritative
//! graph validation. No execution state, API, or policy belongs in this module.

use std::collections::VecDeque;

pub const NODE_WIDTH: usize = 224;
pub const NODE_HEIGHT: usize = 128;
const COLUMN_GAP: usize = 88;
const ROW_GAP: usize = 96;
const PADDING: usize = 72;

/// Fixed-size cards remain readable while their container scrolls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePosition {
    pub index: usize,
    pub depth: usize,
    pub x: usize,
    pub y: usize,
}

/// Cyclic drafts are displayed together after any resolved predecessors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphLayout {
    pub nodes: Vec<NodePosition>,
    pub width: usize,
    pub height: usize,
    pub unresolved: bool,
}

/// Lay out nodes in their stable definition order, ignoring missing endpoints.
///
/// Bounds are supplied by the Workflow editor/API (64 nodes, 256 edges).
/// Traversal terminates for self edges and cycles so a live draft cannot hang
/// the browser. An unresolved preview never blocks or approves server persistence.
#[must_use]
pub fn layered_layout(count: usize, edges: &[(usize, usize)]) -> GraphLayout {
    let mut incoming = vec![0_usize; count];
    let mut outgoing = vec![Vec::new(); count];
    for &(from, to) in edges {
        if from >= count || to >= count {
            continue;
        }
        if let Some(value) = incoming.get_mut(to) {
            *value += 1;
        }
        if let Some(values) = outgoing.get_mut(from) {
            values.push(to);
        }
    }
    let mut ready: VecDeque<usize> = incoming
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (*value == 0).then_some(index))
        .collect();
    let mut depths = vec![0_usize; count];
    let mut visited = vec![false; count];
    while let Some(index) = ready.pop_front() {
        if let Some(value) = visited.get_mut(index) {
            *value = true;
        }
        let next_depth = depths.get(index).copied().unwrap_or_default() + 1;
        for &downstream in outgoing.get(index).into_iter().flatten() {
            if let Some(depth) = depths.get_mut(downstream) {
                *depth = (*depth).max(next_depth);
            }
            if let Some(value) = incoming.get_mut(downstream) {
                *value = value.saturating_sub(1);
                if *value == 0 {
                    ready.push_back(downstream);
                }
            }
        }
    }
    let unresolved = visited.iter().any(|value| !value);
    let draft_depth = depths
        .iter()
        .zip(&visited)
        .filter_map(|(depth, seen)| seen.then_some(*depth))
        .max()
        .map_or(0, |depth| depth + 1);
    for (depth, seen) in depths.iter_mut().zip(&visited) {
        if !seen {
            *depth = draft_depth;
        }
    }
    let layers = depths.iter().max().map_or(1, |depth| depth + 1);
    let mut sizes = vec![0_usize; layers];
    for &depth in &depths {
        if let Some(size) = sizes.get_mut(depth) {
            *size += 1;
        }
    }
    let row_width = |size: usize| {
        size.saturating_mul(NODE_WIDTH + COLUMN_GAP)
            .saturating_sub(COLUMN_GAP)
    };
    let widest = row_width(sizes.iter().copied().max().unwrap_or(1));
    let mut slots = vec![0_usize; layers];
    let nodes = depths
        .into_iter()
        .enumerate()
        .map(|(index, depth)| {
            let offset = slots.get(depth).copied().unwrap_or_default();
            if let Some(slot) = slots.get_mut(depth) {
                *slot += 1;
            }
            let width = row_width(sizes.get(depth).copied().unwrap_or_default());
            NodePosition {
                index,
                depth,
                x: PADDING + widest.saturating_sub(width) / 2 + offset * (NODE_WIDTH + COLUMN_GAP),
                y: PADDING + depth * (NODE_HEIGHT + ROW_GAP),
            }
        })
        .collect();
    GraphLayout {
        nodes,
        width: widest + 2 * PADDING,
        height: layers * (NODE_HEIGHT + ROW_GAP) - ROW_GAP + 2 * PADDING,
        unresolved,
    }
}

#[cfg(test)]
mod tests {
    use super::layered_layout;

    #[test]
    fn linear_nodes_increase_depth_and_join_uses_the_longest_path() {
        let graph = layered_layout(3, &[(0, 1), (1, 2), (0, 2)]);
        assert_eq!(
            graph
                .nodes
                .iter()
                .map(|node| node.depth)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(!graph.unresolved);
    }

    #[test]
    fn parallel_siblings_share_a_layer_and_join_is_below_both() {
        let graph = layered_layout(4, &[(0, 1), (0, 2), (1, 3), (2, 3)]);
        assert_eq!(
            graph
                .nodes
                .iter()
                .map(|node| node.depth)
                .collect::<Vec<_>>(),
            vec![0, 1, 1, 2]
        );
        let left = graph.nodes.get(1);
        let right = graph.nodes.get(2);
        assert!(
            left.zip(right)
                .is_some_and(|(left, right)| left.y == right.y && left.x < right.x)
        );
        assert_eq!(graph, layered_layout(4, &[(0, 2), (2, 3), (0, 1), (1, 3)]));
    }

    #[test]
    fn cyclic_and_incomplete_drafts_have_finite_repeatable_previews() {
        let edges = [(0, 1), (1, 0), (4, 2)];
        let graph = layered_layout(3, &edges);
        assert!(graph.unresolved);
        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph, layered_layout(3, &edges));
        assert!(!layered_layout(0, &[]).unresolved);
    }
}
