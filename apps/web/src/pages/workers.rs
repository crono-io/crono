//! Worker collection page.

use crate::{
    components::{Card, EmptyState, PageHeader},
    navigation::MaterialSymbol,
};
use leptos::prelude::*;

/// Reserve worker visibility without adding management actions.
#[component]
pub fn WorkersPage() -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader
                title="Workers"
                description="Workers execute jobs on behalf of Crono."
            />
            <Card>
                <EmptyState
                    icon=MaterialSymbol::Memory
                    title="No workers yet"
                    description="Workers will appear here when they register with Crono."
                />
            </Card>
        </div>
    }
}
