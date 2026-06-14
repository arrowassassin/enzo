//! Enzo client library — egui/eframe workspace UI over the ATP daemon.
//!
//! The heavy engine work lives in `enzo-daemon`; this crate renders daemon state
//! and routes user input back over ATP. The UI is built with egui (wgpu-backed)
//! and styled to Enzo's "modern terminal-brutalism" aesthetic in [`gui::theme`].

pub mod atp;
pub mod gui;
pub mod overlay;
pub mod surface;
pub mod terminal;
pub mod ui;
