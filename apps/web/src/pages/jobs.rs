//! Job collection page.

use crate::{
    components::{Card, EmptyState, PageHeader},
    navigation::MaterialSymbol,
};
use leptos::prelude::*;

/// Present Jobs as behavior definitions rather than execution destinations.
#[component]
pub fn JobsPage() -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader
                title="Jobs"
                description="Jobs define what Crono executes."
            />
            <Card>
                <EmptyState
                    icon=MaterialSymbol::Work
                    title="No jobs yet"
                    description="Jobs will appear here when they are defined."
                />
            </Card>
        </div>
    }
}
