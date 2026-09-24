//! Shared API-backed option collections for resource relationship fields.
//!
//! Each helper owns one collection request and exposes derived display state,
//! keeping pages focused on their form-specific workflow. Collection filtering
//! remains inside the reusable controls.

use crate::{api, components::ResourceOption};
use leptos::prelude::*;
use uuid::Uuid;

/// Reactive state for one searchable resource collection.
#[derive(Clone, Copy)]
pub(super) struct ResourceOptions {
    pub options: Signal<Vec<ResourceOption>>,
    pub loading: Signal<bool>,
    pub load_error: Signal<Option<String>>,
}

/// Load all Namespaces once for a form and expose name/UUID options.
pub(super) fn namespaces() -> ResourceOptions {
    let resources = LocalResource::new(api::all_namespaces);
    ResourceOptions {
        options: Signal::derive(move || {
            resources
                .get()
                .and_then(Result::ok)
                .unwrap_or_default()
                .into_iter()
                .map(|namespace| ResourceOption {
                    id: namespace.id,
                    label: namespace.name,
                })
                .collect()
        }),
        loading: Signal::derive(move || resources.get().is_none()),
        load_error: Signal::derive(move || {
            resources
                .get()
                .and_then(Result::err)
                .map(|error| error.message)
        }),
    }
}

/// Load enabled Queues once for Job creation and expose name/UUID options.
pub(super) fn queues() -> ResourceOptions {
    let resources = LocalResource::new(api::all_queues);
    ResourceOptions {
        options: Signal::derive(move || {
            resources
                .get()
                .and_then(Result::ok)
                .unwrap_or_default()
                .into_iter()
                .filter(|queue| queue.enabled)
                .map(|queue| ResourceOption {
                    id: queue.id,
                    label: queue.name,
                })
                .collect()
        }),
        loading: Signal::derive(move || resources.get().is_none()),
        load_error: Signal::derive(move || {
            resources
                .get()
                .and_then(Result::err)
                .map(|error| error.message)
        }),
    }
}

/// Load Jobs whenever the selected Namespace changes.
pub(super) fn jobs(namespace_id: RwSignal<Option<Uuid>>) -> ResourceOptions {
    let resources = LocalResource::new(move || {
        let selected = namespace_id.get();
        async move {
            match selected {
                Some(id) => api::all_jobs(id).await,
                None => Ok(Vec::new()),
            }
        }
    });
    ResourceOptions {
        options: Signal::derive(move || {
            resources
                .get()
                .and_then(Result::ok)
                .unwrap_or_default()
                .into_iter()
                .map(|job| ResourceOption {
                    id: job.id,
                    label: job.name,
                })
                .collect()
        }),
        loading: Signal::derive(move || namespace_id.get().is_some() && resources.get().is_none()),
        load_error: Signal::derive(move || {
            resources
                .get()
                .and_then(Result::err)
                .map(|error| error.message)
        }),
    }
}

/// Load Targets whenever the selected Namespace changes.
pub(super) fn targets(namespace_id: RwSignal<Option<Uuid>>) -> ResourceOptions {
    let resources = LocalResource::new(move || {
        let selected = namespace_id.get();
        async move {
            match selected {
                Some(id) => api::all_targets(id).await,
                None => Ok(Vec::new()),
            }
        }
    });
    ResourceOptions {
        options: Signal::derive(move || {
            resources
                .get()
                .and_then(Result::ok)
                .unwrap_or_default()
                .into_iter()
                .map(|target| ResourceOption {
                    id: target.id,
                    label: target.name,
                })
                .collect()
        }),
        loading: Signal::derive(move || namespace_id.get().is_some() && resources.get().is_none()),
        load_error: Signal::derive(move || {
            resources
                .get()
                .and_then(Result::err)
                .map(|error| error.message)
        }),
    }
}
