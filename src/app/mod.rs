//! The window: sidebar, header, the page sliding in the content card, and the settings dialog.

mod bridge;
#[cfg(test)]
mod demo;
mod engine_link;
pub mod history_query;
pub mod live;
mod navigation;
mod root;
mod sidebar;
mod state;
#[cfg(test)]
mod tests;

pub use bridge::engine_settings;
pub use engine_link::EngineLink;
pub use navigation::{Navigation, Page};
pub use root::root;
pub use state::AppState;
