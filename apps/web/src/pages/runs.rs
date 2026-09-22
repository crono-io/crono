//! Run collection page.

use crate::{
    components::{Card, EmptyState, PageHeader},
    navigation::MaterialSymbol,
};
use leptos::prelude::*;

/// Reserve the execution-history area without fabricating Run data.
#[component]
pub fn RunsPage() -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader
                title="Runs"
                description="View current and historical Crono executions."
            />
            <Card>
                <EmptyState
                    icon=MaterialSymbol::PlayCircle
                    title="No runs yet"
                    description="Runs will appear here when jobs are executed."
                />
            </Card>
        </div>
    }
}
