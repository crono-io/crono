//! Browser coverage for labeled dependencies, real states, and retained Run links.

use super::graph::{GraphEdge, GraphNode, WorkflowGraph};
use crono_api::{DependencyCondition, WorkflowNodeRunState};
use leptos::prelude::*;
use leptos_router::components::Router;
use std::{cell::RefCell, rc::Rc};
use uuid::Uuid;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::HtmlElement;

#[wasm_bindgen_test]
async fn branches_show_actual_states_labels_and_keep_run_link_focused_on_refresh() {
    let host = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| {
            let host = document
                .create_element("div")
                .ok()?
                .dyn_into::<HtmlElement>()
                .ok()?;
            document.body()?.append_child(&host).ok()?;
            Some(host)
        });
    assert!(host.is_some());
    let Some(host) = host else {
        return;
    };
    let captured = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&captured);
    let run_id = Uuid::now_v7();
    let handle = leptos::mount::mount_to(host.clone(), move || {
        let nodes = RwSignal::new(vec![
            node("deploy", WorkflowNodeRunState::Succeeded, vec![run_id]),
            node("verify", WorkflowNodeRunState::Running, Vec::new()),
            node("rollback", WorkflowNodeRunState::Skipped, Vec::new()),
            node("cleanup", WorkflowNodeRunState::Pending, Vec::new()),
        ]);
        *capture.borrow_mut() = Some(nodes);
        let edges = vec![
            edge("verify", DependencyCondition::Success),
            edge("rollback", DependencyCondition::Failure),
            edge("cleanup", DependencyCondition::Always),
        ];
        view! { <Router><WorkflowGraph nodes=Signal::derive(move || nodes.get()) edges=Signal::derive(move || edges.clone()) /></Router> }
    });
    leptos::task::tick().await;
    assert!(host.text_content().is_some_and(|text| {
        [
            "Succeeded",
            "Running",
            "Skipped",
            "On success",
            "On failure",
            "Always",
        ]
        .iter()
        .all(|label| text.contains(label))
    }));
    let link = host
        .query_selector(&format!("a[href='/runs/{run_id}']"))
        .ok()
        .flatten()
        .and_then(|element| element.dyn_into::<HtmlElement>().ok());
    assert!(link.is_some());
    let Some(link) = link else {
        return;
    };
    assert!(link.focus().is_ok());
    let signals = *captured.borrow();
    assert!(signals.is_some());
    if let Some(nodes) = signals {
        nodes.update(|nodes| {
            if let Some(node) = nodes.iter_mut().find(|node| node.name == "verify") {
                node.state = Some(WorkflowNodeRunState::Succeeded);
            }
        });
    }
    leptos::task::tick().await;
    assert!(
        web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.active_element())
            .is_some_and(|active| active == *link)
    );
    assert!(
        host.query_selector("[data-workflow-node='verify']")
            .ok()
            .flatten()
            .and_then(|element| element.text_content())
            .is_some_and(|text| text.contains("Succeeded"))
    );
    drop(handle);
    host.remove();
}

fn node(name: &str, state: WorkflowNodeRunState, runs: Vec<Uuid>) -> GraphNode {
    GraphNode {
        key: name.to_string(),
        name: name.to_string(),
        job: format!("Job {name}"),
        state: Some(state),
        runs,
    }
}

fn edge(to: &str, condition: DependencyCondition) -> GraphEdge {
    GraphEdge {
        from: "deploy".to_string(),
        to: to.to_string(),
        condition,
    }
}
