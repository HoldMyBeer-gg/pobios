//! The build as the UI sees it, plus the passive tree renderer.
//!
//! Everything the iPad host needs from the old desktop app crate, and nothing
//! of the desktop app itself: no eframe, no docks, no tabs.

pub mod build_edit;
pub mod build_state;
pub mod style;
pub mod tree_art;
pub mod tree_view;
