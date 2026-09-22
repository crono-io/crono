//! Target Set collection page.

use crate::{
    components::{Card, EmptyState, PageHeader},
    navigation::MaterialSymbol,
};
use leptos::prelude::*;

/// Present Target Sets as named explicit selections of Targets.
#[component]
pub fn TargetSetsPage() -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader
                title="Target Sets"
                description="Target Sets organize multiple targets for execution."
            />
            <Card>
                <EmptyState
                    icon=MaterialSymbol::Lan
                    title="No target sets yet"
                    description="Target sets will appear here when they are defined."
                />
            </Card>
        </div>
    }
}
