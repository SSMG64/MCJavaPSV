//! The MCJavaPSV builder: shared logic behind the `mcjavapsv` window and the
//! `mcjavapsv-cli` command line tool.

pub mod app;
pub mod commands;
pub mod diagnostics;
pub mod jobs;
pub mod logging;
pub mod paths;
pub mod payload;
pub mod settings;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
