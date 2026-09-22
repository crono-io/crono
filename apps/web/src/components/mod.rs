//! Small visual primitives shared by Crono pages.
//!
//! Components establish consistent structure, spacing, focus treatment, and
//! accessibility without becoming a general-purpose design system. They own no
//! routing, API, authentication, or domain behavior.

pub mod card;
pub mod empty_state;
pub mod icon;
pub mod layout;
pub mod page_header;
pub mod summary_card;

pub use card::Card;
pub use empty_state::EmptyState;
pub use icon::Icon;
pub use page_header::PageHeader;
pub use summary_card::{SummaryCard, SummaryTone};
