//! Library root: exposes the core, cli, and tui modules so integration tests
//! (and future embedders) can exercise them without going through the binary.

pub mod cli;
pub mod core;
pub mod tui;
