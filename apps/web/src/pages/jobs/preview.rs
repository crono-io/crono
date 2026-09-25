//! Transient Job argv/input preview against a selected Target or Target Set.
//!
//! Preview never creates a Run and excludes later Schedule or manual inputs.
//! It reads the current form signals but keeps rendering and destination loads
//! out of the create/edit form workflow.

use crate::{
    api,
    components::{ResourceOption, parse_input_object},
};
use crono_api::ExecutorKind;
use leptos::prelude::*;
use uuid::Uuid;

/// The execution-relevant form fields consumed by preview rendering.
#[derive(Clone, Copy)]
pub(super) struct PreviewFields {
    pub executor: RwSignal<ExecutorKind>,
    pub executable: RwSignal<String>,
    pub shell_command: RwSignal<String>,
    pub arguments: RwSignal<Vec<String>>,
    pub inputs: RwSignal<String>,
}

/// Options are transient destinations, not Job fields persisted on save.
pub(super) fn job_preview_options(
    targets: LocalResource<api::ApiResult<Vec<crono_api::TargetResource>>>,
    sets: LocalResource<api::ApiResult<Vec<crono_api::TargetSetResource>>>,
) -> Signal<Vec<ResourceOption>> {
    Signal::derive(move || {
        let mut options = targets
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .map(|target| ResourceOption {
                id: target.id,
                label: format!("Target · {}", target.name),
            })
            .collect::<Vec<_>>();
        options.extend(
            sets.get()
                .and_then(Result::ok)
                .unwrap_or_default()
                .into_iter()
                .map(|set| ResourceOption {
                    id: set.id,
                    label: format!("Target Set · {}", set.name),
                }),
        );
        options
    })
}

/// Render the current form values for one selected destination without side effects.
pub(super) fn preview_text(
    fields: PreviewFields,
    selected: Option<Uuid>,
    targets: Option<api::ApiResult<Vec<crono_api::TargetResource>>>,
    sets: Option<api::ApiResult<Vec<crono_api::TargetSetResource>>>,
) -> String {
    let Some(selected) = selected else {
        return "Select a Target or Target Set to preview its command and inputs.".to_string();
    };
    let Ok(job_inputs) = parse_input_object(&fields.inputs.get()) else {
        return "Fix the Job inputs to preview.".to_string();
    };
    let targets = targets.and_then(Result::ok).unwrap_or_default();
    let sets = sets.and_then(Result::ok).unwrap_or_default();
    let executable = if fields.executor.get() == ExecutorKind::Noop {
        "noop".to_string()
    } else {
        format!("{:?}", fields.executable.get())
    };
    let selected_targets = if let Some(target) = targets.iter().find(|target| target.id == selected)
    {
        vec![(target, None)]
    } else if let Some(set) = sets.iter().find(|set| set.id == selected) {
        set.targets
            .iter()
            .filter_map(|member| {
                targets
                    .iter()
                    .find(|target| target.id == member.id)
                    .map(|target| (target, Some(&set.inputs)))
            })
            .collect()
    } else {
        return "The selected preview destination is no longer available.".to_string();
    };
    selected_targets
        .into_iter()
        .map(|(target, set_inputs)| {
            let empty = serde_json::json!({});
            let merged = crono_execution::merge_inputs(&[
                &job_inputs,
                set_inputs.unwrap_or(&empty),
                &target.inputs,
            ])
            .and_then(|inputs| {
                let mut argv = fields.arguments.get();
                argv.extend(target.arguments.clone());
                crono_execution::render_arguments(&argv, &inputs).map(|argv| (inputs, argv))
            });
            match merged {
                Ok((merged_inputs, argv)) => format!(
                    "{}\n$ {}{} {}\ninputs: {}",
                    target.name,
                    executable,
                    if fields.executor.get() == ExecutorKind::Shell {
                        format!(" -c {:?} crono-job", fields.shell_command.get())
                    } else {
                        String::new()
                    },
                    argv.iter()
                        .map(|item| format!("{item:?}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    pretty_json(&merged_inputs)
                ),
                Err(error) => format!("{}\nPreview error: {error}", target.name),
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Format API input JSON for both the editor and preview.
pub(super) fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}

/// Load destinations only after a Namespace is selected.
pub(super) async fn load_targets(
    namespace: Option<Uuid>,
) -> api::ApiResult<Vec<crono_api::TargetResource>> {
    match namespace {
        Some(id) => api::all_targets(id).await,
        None => Ok(Vec::new()),
    }
}

/// Load destination groups only after a Namespace is selected.
pub(super) async fn load_target_sets(
    namespace: Option<Uuid>,
) -> api::ApiResult<Vec<crono_api::TargetSetResource>> {
    match namespace {
        Some(id) => api::all_target_sets(id).await,
        None => Ok(Vec::new()),
    }
}
