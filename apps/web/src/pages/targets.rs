//! Target collection page.

use crate::{
    components::{Card, EmptyState, PageHeader},
    navigation::MaterialSymbol,
};
use leptos::prelude::*;

/// Present Targets as executor-agnostic destinations or resources.
#[component]
pub fn TargetsPage() -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader
                title="Targets"
                description="Targets define where or against what jobs execute."
            />
            <Card>
                <EmptyState
                    icon=MaterialSymbol::Dns
                    title="No targets yet"
                    description="Targets will appear here when they are defined."
                />
            </Card>
        </div>
    }
}
