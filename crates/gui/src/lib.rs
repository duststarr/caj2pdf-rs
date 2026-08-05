//! Library facade for the desktop GUI.
//!
//! Exists so unit tests can construct the [`App`] without going through
//! the binary entry point.

#![deny(missing_debug_implementations)]
#![warn(unreachable_pub)]

mod app;
mod font;

pub use app::{App, FileEntry, Status};
pub use font::cjk_font_definitions;
