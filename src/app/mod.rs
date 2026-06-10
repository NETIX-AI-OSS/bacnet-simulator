pub mod log;
pub mod metrics;
pub mod snapshot;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ::log::{error, info};
use tokio::sync::Mutex;

use crate::bacnet_server::BacnetServer;
use crate::config::SimulatorConfig;
use crate::simulation::Simulation;
use crate::simulation::registry::build_device_registry;

pub use log::AppLog;
pub use metrics::AppMetrics;

const DEFAULT_PORT: u16 = 47808;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Tui,
    Headless,
}

#[derive(Debug, Clone)]
pub struct AppMeta {
    pub building_name: String,
    pub config_path: PathBuf,
    pub port: u16,
    pub started_at: Instant,
}

pub struct AppContext {
    pub meta: AppMeta,
    pub simulation: Arc<Mutex<Simulation>>,
    pub metrics: Arc<AppMetrics>,
    pub log: Arc<AppLog>,
}

pub fn detect_run_mode(no_tui_flag: bool) -> RunMode {
    if no_tui_flag {
        return RunMode::Headless;
    }
    if std::env::var("BACNET_SIM_NO_TUI")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return RunMode::Headless;
    }
    if std::io::stdout().is_terminal() {
        RunMode::Tui
    } else {
        RunMode::Headless
    }
}

pub fn parse_args() -> (bool, PathBuf) {
    let mut no_tui = false;
    let mut config_path = std::env::var("CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("config.yaml"));

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-tui" => no_tui = true,
            "--config" | "-c" => {
                if let Some(path) = args.next() {
                    config_path = PathBuf::from(path);
                }
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other if other.starts_with('-') => {
                eprintln!("Unknown flag: {other}");
                print_help();
                std::process::exit(2);
            }
            path => config_path = PathBuf::from(path),
        }
    }

    (no_tui, config_path)
}

fn print_help() {
    eprintln!(
        "Usage: bacnet-simulator [OPTIONS] [CONFIG_PATH]\n\n\
         Options:\n\
           --no-tui          Log-only mode (also BACNET_SIM_NO_TUI=1)\n\
           -c, --config PATH Config file (default: config.yaml)\n\
           -h, --help        Show this help\n"
    );
}

pub fn run(
    mode: RunMode,
    config_path: PathBuf,
    config: SimulatorConfig,
    simulation: Simulation,
) -> Result<(), Box<dyn std::error::Error>> {
    let devices = build_device_registry(&simulation.devices);
    let device_count = simulation.devices.len();
    let point_count = simulation.total_points();

    let meta = AppMeta {
        building_name: config.building.name.clone(),
        config_path: config_path.clone(),
        port: DEFAULT_PORT,
        started_at: Instant::now(),
    };

    let metrics = Arc::new(AppMetrics::new());
    let app_log = Arc::new(AppLog::new());
    let sim_arc = Arc::new(Mutex::new(simulation));
    let server = BacnetServer::new(
        sim_arc.clone(),
        devices,
        DEFAULT_PORT,
        Arc::clone(&metrics),
        if mode == RunMode::Tui {
            Some(Arc::clone(&app_log))
        } else {
            None
        },
    );

    let rt = tokio::runtime::Runtime::new()?;
    let _server_handle = rt.spawn(server.run());

    match mode {
        RunMode::Headless => {
            info!("Using config at {}", config_path.display());
            info!(
                "Loaded configuration for building: {} ({} devices, {} points)",
                meta.building_name, device_count, point_count
            );
            rt.block_on(async {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                }
            });
        }
        RunMode::Tui => {
            app_log.push(format!(
                "Loaded {} devices, {} points from {}",
                device_count,
                point_count,
                config_path.display()
            ));
            let ctx = AppContext {
                meta,
                simulation: sim_arc,
                metrics,
                log: app_log,
            };
            crate::tui::run(rt, ctx)?;
        }
    }

    Ok(())
}

pub fn restart_process() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let cwd = std::env::current_dir()?;
    std::process::Command::new(exe)
        .current_dir(cwd)
        .spawn()?;
    Ok(())
}

pub fn bootstrap_config(config_path: &PathBuf) -> Result<SimulatorConfig, i32> {
    match SimulatorConfig::ensure_config_file(config_path) {
        Ok(true) => {}
        Ok(false) => {}
        Err(e) => {
            error!(
                "Failed to prepare config at {}: {}",
                config_path.display(),
                e
            );
            crate::exit_with_error(1);
        }
    }

    let config_path_str = config_path.to_string_lossy();
    match SimulatorConfig::load_from_file(&config_path_str) {
        Ok(c) => Ok(c),
        Err(e) => {
            error!(
                "Failed to load config from {}: {}",
                config_path.display(),
                e
            );
            crate::exit_with_error(1);
        }
    }
}

pub fn build_simulation(config: &SimulatorConfig) -> Result<Simulation, i32> {
    match Simulation::new(config) {
        Ok(s) => Ok(s),
        Err(e) => {
            error!("Failed to build simulation: {}", e);
            crate::exit_with_error(1);
        }
    }
}
