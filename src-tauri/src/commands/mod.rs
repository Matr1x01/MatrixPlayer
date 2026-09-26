//! Tauri commands: the frontend's only way into the Rust core.
//!
//! Commands validate their arguments and delegate to services; they hold no
//! logic of their own.

pub mod library;
pub mod player;
pub mod queue;
pub mod system;
