//! Namespace collection page.

use crate::{
    components::{Card, EmptyState, PageHeader},
    navigation::MaterialSymbol,
};
use leptos::prelude::*;

/// Establish Namespace terminology and its initial empty state.
#[component]
pub fn NamespacesPage() -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader
                title="Namespaces"
                description="Namespaces organize related jobs and execution targets."
            />
            <Card>
                <EmptyState
                    icon=MaterialSymbol::AccountTree
                    title="No namespaces yet"
                    description="Namespaces will appear here when they are created."
                />
            </Card>
        </div>
    }
}
