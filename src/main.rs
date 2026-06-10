mod app;
mod bacnet_server;
mod config;
mod simulation;
mod tui;

use app::{bootstrap_config, build_simulation, detect_run_mode, parse_args, run, RunMode};
use log::info;

#[cfg(windows)]
fn pause_on_fatal_error() {
    use std::io::{self, Write};

    let _ = writeln!(io::stderr(), "\nPress Enter to close this window...");
    let _ = io::stderr().flush();
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
}

#[cfg(not(windows))]
fn pause_on_fatal_error() {}

pub fn exit_with_error(code: i32) -> ! {
    pause_on_fatal_error();
    std::process::exit(code);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (no_tui_flag, config_path) = parse_args();
    let mode = detect_run_mode(no_tui_flag);

    if mode == RunMode::Headless {
        env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
        info!("Starting BACnet Building Simulator...");
    }

    let config = match bootstrap_config(&config_path) {
        Ok(c) => c,
        Err(code) => exit_with_error(code),
    };
    let simulation = match build_simulation(&config) {
        Ok(s) => s,
        Err(code) => exit_with_error(code),
    };

    run(mode, config_path, config, simulation)
}
