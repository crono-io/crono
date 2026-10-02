//! Small visual primitives shared by Crono pages.
//!
//! Components establish consistent structure, spacing, focus treatment, and
//! accessibility without becoming a general-purpose design system. They own no
//! routing, API, authentication, or domain behavior.

pub mod card;
pub mod empty_state;
pub mod forms;
pub mod icon;
pub mod layout;
pub mod modal;
pub mod page_header;
pub mod resource_dialogs;
pub mod timezone_select;

pub use card::Card;
pub use empty_state::EmptyState;
pub use forms::{
    ArgumentListInput, FormActions, JsonObjectInput, ResourceMultiSelect, ResourceNameInput,
    ResourceOption, ResourceSelect, name_validation_message, parse_input_object,
    visible_name_validation,
};
pub use icon::Icon;
pub use modal::Modal;
pub use page_header::PageHeader;
pub use resource_dialogs::{DeleteControl, ResourceFeedback, ResourceFeedbackModal, focus_heading};
pub use timezone_select::TimezoneSelect;

/// Style secondary links and controls as quiet navigation with clear hover and focus states.
pub(crate) const QUIET_ACTION_CLASS: &str = "inline-flex min-h-10 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md px-2 py-2 text-sm font-medium text-crono-muted transition-colors hover:bg-zinc-100 hover:text-crono-primary focus-visible:bg-crono-primary-soft focus-visible:text-crono-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent disabled:hover:text-crono-muted sm:gap-2 sm:px-3";
