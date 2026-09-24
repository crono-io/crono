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
pub mod page_header;

pub use card::Card;
pub use empty_state::EmptyState;
pub use forms::{
    FormActions, ResourceMultiSelect, ResourceNameInput, ResourceOption, ResourceSelect,
    name_validation_message, visible_name_validation,
};
pub use icon::Icon;
pub use page_header::PageHeader;
