//! Browser settings placeholder.

use crate::{
    components::{Card, EmptyState, PageHeader},
    navigation::MaterialSymbol,
};
use leptos::prelude::*;

/// Reserve stable layout space for future browser preferences.
#[component]
pub fn SettingsPage() -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader
                title="Settings"
                description="Configure this Crono environment."
            />
            <Card>
                <EmptyState
                    icon=MaterialSymbol::Settings
                    title="No settings available"
                    description="Configuration options will appear here when they become available."
                />
            </Card>
        </div>
    }
}
