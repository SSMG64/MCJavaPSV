#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use psv_builder::{app, logging, paths::AppPaths, payload::Payload};

fn main() -> eframe::Result<()> {
    let paths = AppPaths::detect();
    let verbose = std::env::args().any(|arg| arg == "--verbose");
    let logging = logging::init(&paths.log_dir(), cfg!(debug_assertions), verbose);
    app::run(paths, Payload::locate(), logging.buffer)
}
