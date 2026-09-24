//! Canonical route and sidebar metadata for the browser application.
//!
//! One typed model drives route tests, link labels, icon selection, and active
//! navigation. Keeping this module independent from Leptos makes route coverage
//! testable on native targets while the rendered application remains WASM-only.

/// Every first-class page in the initial browser shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRoute {
    Overview,
    Namespaces,
    Jobs,
    Targets,
    TargetSets,
    Runs,
    Workers,
    Settings,
}

impl AppRoute {
    /// Return the canonical absolute browser path.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Overview => "/",
            Self::Namespaces => "/namespaces",
            Self::Jobs => "/jobs",
            Self::Targets => "/targets",
            Self::TargetSets => "/target-sets",
            Self::Runs => "/runs",
            Self::Workers => "/workers",
            Self::Settings => "/settings",
        }
    }

    /// Return the concise human-facing navigation label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Namespaces => "Namespaces",
            Self::Jobs => "Jobs",
            Self::Targets => "Targets",
            Self::TargetSets => "Target Sets",
            Self::Runs => "Runs",
            Self::Workers => "Workers",
            Self::Settings => "Settings",
        }
    }

    /// Return the explicit Material Symbol assigned to the route.
    #[must_use]
    pub const fn symbol(self) -> MaterialSymbol {
        match self {
            Self::Overview => MaterialSymbol::Dashboard,
            Self::Namespaces => MaterialSymbol::AccountTree,
            Self::Jobs => MaterialSymbol::Work,
            Self::Targets => MaterialSymbol::Dns,
            Self::TargetSets => MaterialSymbol::Lan,
            Self::Runs => MaterialSymbol::PlayCircle,
            Self::Workers => MaterialSymbol::Memory,
            Self::Settings => MaterialSymbol::Settings,
        }
    }

    /// Resolve an exact canonical path to its route.
    #[must_use]
    pub fn from_path(path: &str) -> Option<Self> {
        ALL_ROUTES
            .iter()
            .copied()
            .find(|route| route.path() == path)
    }
}

/// Known symbols used by the Crono shell and empty states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialSymbol {
    AccountTree,
    Check,
    Dashboard,
    Dns,
    ExpandMore,
    Lan,
    LightMode,
    Memory,
    PlayCircle,
    SearchOff,
    Settings,
    Work,
}

impl MaterialSymbol {
    /// Return the exact ligature understood by Material Symbols Outlined.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AccountTree => "account_tree",
            Self::Check => "check",
            Self::Dashboard => "dashboard",
            Self::Dns => "dns",
            Self::ExpandMore => "expand_more",
            Self::Lan => "lan",
            Self::LightMode => "light_mode",
            Self::Memory => "memory",
            Self::PlayCircle => "play_circle",
            Self::SearchOff => "search_off",
            Self::Settings => "settings",
            Self::Work => "work",
        }
    }
}

/// Named navigation section rendered in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationGroup {
    pub label: Option<&'static str>,
    pub routes: &'static [AppRoute],
}

const OVERVIEW_ROUTES: &[AppRoute] = &[AppRoute::Overview];
const RESOURCE_ROUTES: &[AppRoute] = &[
    AppRoute::Namespaces,
    AppRoute::Jobs,
    AppRoute::Targets,
    AppRoute::TargetSets,
];
const EXECUTION_ROUTES: &[AppRoute] = &[AppRoute::Runs, AppRoute::Workers];
const SYSTEM_ROUTES: &[AppRoute] = &[AppRoute::Settings];

/// Complete route inventory used for exact matching and verification.
pub const ALL_ROUTES: [AppRoute; 8] = [
    AppRoute::Overview,
    AppRoute::Namespaces,
    AppRoute::Jobs,
    AppRoute::Targets,
    AppRoute::TargetSets,
    AppRoute::Runs,
    AppRoute::Workers,
    AppRoute::Settings,
];

/// Sidebar sections in their canonical display order.
pub const NAVIGATION_GROUPS: [NavigationGroup; 4] = [
    NavigationGroup {
        label: None,
        routes: OVERVIEW_ROUTES,
    },
    NavigationGroup {
        label: Some("Resources"),
        routes: RESOURCE_ROUTES,
    },
    NavigationGroup {
        label: Some("Execution"),
        routes: EXECUTION_ROUTES,
    },
    NavigationGroup {
        label: Some("System"),
        routes: SYSTEM_ROUTES,
    },
];

/// Test whether a location exactly identifies a sidebar route.
#[must_use]
pub fn is_active_path(path: &str, route: AppRoute) -> bool {
    AppRoute::from_path(path) == Some(route)
}

#[cfg(test)]
mod tests {
    use super::{ALL_ROUTES, AppRoute, MaterialSymbol, NAVIGATION_GROUPS, is_active_path};

    #[test]
    fn expected_routes_are_canonical_and_unique() {
        let expected = [
            "/",
            "/namespaces",
            "/jobs",
            "/targets",
            "/target-sets",
            "/runs",
            "/workers",
            "/settings",
        ];

        assert!(
            expected
                .iter()
                .all(|path| AppRoute::from_path(path).is_some())
        );
        for (position, route) in ALL_ROUTES.iter().enumerate() {
            assert!(
                ALL_ROUTES
                    .iter()
                    .skip(position.saturating_add(1))
                    .all(|other| route.path() != other.path())
            );
        }
    }

    #[test]
    fn sidebar_defines_every_route_once() {
        for route in ALL_ROUTES {
            let occurrences = NAVIGATION_GROUPS
                .iter()
                .flat_map(|group| group.routes.iter())
                .filter(|candidate| **candidate == route)
                .count();
            assert_eq!(occurrences, 1, "{}", route.path());
            assert!(!route.label().is_empty());
            assert!(!route.symbol().as_str().is_empty());
        }
    }

    #[test]
    fn active_route_matching_is_exact() {
        assert!(is_active_path("/targets", AppRoute::Targets));
        assert!(!is_active_path("/targets/", AppRoute::Targets));
        assert!(!is_active_path("/target-sets", AppRoute::Targets));
        assert!(!is_active_path("/unknown", AppRoute::Overview));
    }

    #[test]
    fn shell_symbols_are_explicit() {
        let shell_symbols = [
            MaterialSymbol::Check,
            MaterialSymbol::ExpandMore,
            MaterialSymbol::LightMode,
            MaterialSymbol::SearchOff,
        ];

        assert!(
            shell_symbols
                .into_iter()
                .all(|symbol| !symbol.as_str().is_empty())
        );
    }
}
