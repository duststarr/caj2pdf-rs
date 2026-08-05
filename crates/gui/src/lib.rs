//! Library facade for the desktop GUI.
//!
//! Exists so unit tests can construct the [`App`] without going through
//! the binary entry point.

#![deny(missing_debug_implementations)]
#![warn(unreachable_pub)]

mod app;
pub use app::{App, FileEntry, Status};
