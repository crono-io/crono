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
    Queues,
    Jobs,
    JobsNew,
    Targets,
    TargetSets,
    Schedules,
    Runs,
    Workers,
    Monitor,
    Settings,
}

impl AppRoute {
    /// Return the canonical absolute browser path.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Overview => "/",
            Self::Namespaces => "/namespaces",
            Self::Queues => "/queues",
            Self::Jobs => "/jobs",
            Self::JobsNew => "/jobs/new",
            Self::Targets => "/targets",
            Self::TargetSets => "/target-sets",
            Self::Schedules => "/schedules",
            Self::Runs => "/runs",
            Self::Workers => "/workers",
            Self::Monitor => "/monitor",
            Self::Settings => "/settings",
        }
    }

    /// Return the concise human-facing navigation label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Namespaces => "Namespaces",
            Self::Queues => "Queues",
            Self::Jobs => "Jobs",
            Self::JobsNew => "Create Job",
            Self::Targets => "Targets",
            Self::TargetSets => "Target Sets",
            Self::Schedules => "Schedules",
            Self::Runs => "Runs",
            Self::Workers => "Workers",
            Self::Monitor => "Monitor",
            Self::Settings => "Settings",
        }
    }

    /// Return the explicit Material Symbol assigned to the route.
    #[must_use]
    pub const fn symbol(self) -> MaterialSymbol {
        match self {
            Self::Overview => MaterialSymbol::Dashboard,
            Self::Namespaces => MaterialSymbol::AccountTree,
            Self::Queues => MaterialSymbol::Queue,
            Self::Jobs | Self::JobsNew => MaterialSymbol::Work,
            Self::Targets => MaterialSymbol::Dns,
            Self::TargetSets => MaterialSymbol::Lan,
            Self::Schedules => MaterialSymbol::CalendarMonth,
            Self::Runs => MaterialSymbol::PlayCircle,
            Self::Workers => MaterialSymbol::Memory,
            Self::Monitor => MaterialSymbol::Monitoring,
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

    /// Static sidebar actions beneath a resource type, never resource records.
    #[must_use]
    pub const fn children(self) -> &'static [NavigationChild] {
        match self {
            Self::Jobs => JOB_CHILDREN,
            _ => &[],
        }
    }

    /// Stable accessible ID for a resource submenu; empty when none exists.
    #[must_use]
    pub const fn submenu_id(self) -> &'static str {
        match self {
            Self::Jobs => "jobs-submenu",
            _ => "",
        }
    }
}

/// One navigable child in a resource-type submenu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationChild {
    pub route: AppRoute,
    pub label: &'static str,
}

const JOB_CHILDREN: &[NavigationChild] = &[
    NavigationChild {
        route: AppRoute::Jobs,
        label: "All Jobs",
    },
    NavigationChild {
        route: AppRoute::JobsNew,
        label: "Create Job",
    },
];

/// Known symbols used by the Crono shell and empty states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialSymbol {
    AccountTree,
    CalendarMonth,
    Check,
    Dashboard,
    Dns,
    ExpandMore,
    Lan,
    LightMode,
    Memory,
    Monitoring,
    PlayCircle,
    Queue,
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
            Self::CalendarMonth => "calendar_month",
            Self::Check => "check",
            Self::Dashboard => "dashboard",
            Self::Dns => "dns",
            Self::ExpandMore => "expand_more",
            Self::Lan => "lan",
            Self::LightMode => "light_mode",
            Self::Memory => "memory",
            Self::Monitoring => "monitoring",
            Self::PlayCircle => "play_circle",
            Self::Queue => "queue",
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
    AppRoute::Queues,
    AppRoute::Jobs,
    AppRoute::Targets,
    AppRoute::TargetSets,
];
const EXECUTION_ROUTES: &[AppRoute] = &[AppRoute::Schedules, AppRoute::Runs, AppRoute::Workers];
const SYSTEM_ROUTES: &[AppRoute] = &[AppRoute::Monitor, AppRoute::Settings];

/// Complete route inventory used for exact matching and verification.
pub const ALL_ROUTES: [AppRoute; 12] = [
    AppRoute::Overview,
    AppRoute::Namespaces,
    AppRoute::Queues,
    AppRoute::Jobs,
    AppRoute::JobsNew,
    AppRoute::Targets,
    AppRoute::TargetSets,
    AppRoute::Schedules,
    AppRoute::Runs,
    AppRoute::Workers,
    AppRoute::Monitor,
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

/// Treat deeper resource URLs as part of the parent section for expansion.
#[must_use]
pub fn is_section_active_path(path: &str, route: AppRoute) -> bool {
    is_active_path(path, route)
        || (!route.children().is_empty()
            && path
                .strip_prefix(route.path())
                .is_some_and(|suffix| suffix.starts_with('/')))
}

/// Canonical edit URL for a Job; resource records never become sidebar items.
#[must_use]
pub fn job_edit_path(id: impl std::fmt::Display) -> String {
    format!("/jobs/{id}/edit")
}

#[cfg(test)]
mod tests {
    use super::{
        ALL_ROUTES, AppRoute, MaterialSymbol, NAVIGATION_GROUPS, is_active_path,
        is_section_active_path, job_edit_path,
    };

    #[test]
    fn expected_routes_are_canonical_and_unique() {
        let expected = [
            "/",
            "/namespaces",
            "/queues",
            "/jobs",
            "/jobs/new",
            "/targets",
            "/target-sets",
            "/runs",
            "/workers",
            "/monitor",
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
            let top_level = NAVIGATION_GROUPS
                .iter()
                .flat_map(|group| group.routes.iter())
                .filter(|candidate| **candidate == route)
                .count();
            let child = NAVIGATION_GROUPS
                .iter()
                .flat_map(|group| group.routes.iter())
                .flat_map(|parent| parent.children().iter())
                .filter(|candidate| candidate.route == route)
                .count();
            assert_eq!(
                top_level,
                usize::from(route != AppRoute::JobsNew),
                "{}",
                route.path()
            );
            assert_eq!(
                child,
                usize::from(matches!(route, AppRoute::Jobs | AppRoute::JobsNew)),
                "{}",
                route.path()
            );
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
        assert!(is_active_path("/jobs/new", AppRoute::JobsNew));
        assert!(!is_active_path("/jobs/new", AppRoute::Jobs));
        assert!(is_section_active_path("/jobs/new", AppRoute::Jobs));
        assert!(is_section_active_path("/jobs/123/edit", AppRoute::Jobs));
        assert!(!is_section_active_path("/jobs-other", AppRoute::Jobs));
    }

    #[test]
    fn job_submenu_contains_only_browse_and_create_actions() {
        assert_eq!(AppRoute::Jobs.children().len(), 2);
        assert_eq!(
            AppRoute::Jobs.children().first().map(|child| child.label),
            Some("All Jobs")
        );
        assert_eq!(
            AppRoute::Jobs.children().get(1).map(|child| child.label),
            Some("Create Job")
        );
        assert_eq!(AppRoute::Jobs.submenu_id(), "jobs-submenu");
        let id = "00000000-0000-0000-0000-000000000000";
        assert_eq!(job_edit_path(id), format!("/jobs/{id}/edit"));
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
