mod bacnet_server;
mod config;
mod simulation;

use std::sync::Arc;

use bacnet_server::BacnetServer;
use config::SimulatorConfig;
use log::{error, info};
use simulation::Simulation;
use simulation::registry::build_device_registry;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    info!("Starting BACnet Building Simulator...");

    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.yaml".to_string());

    let config = match SimulatorConfig::load_from_file(&config_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config from {}: {}", config_path, e);
            std::process::exit(1);
        }
    };

    info!(
        "Loaded configuration for building: {} ({} templates, {} instance blocks)",
        config.building.name,
        config.templates.len(),
        config.instances.len()
    );

    let simulation = match Simulation::new(&config) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to build simulation: {}", e);
            std::process::exit(1);
        }
    };

    let devices = build_device_registry(&simulation.devices);
    info!(
        "Simulating {} devices with {} total points.",
        simulation.devices.len(),
        simulation.total_points()
    );

    let sim_arc = Arc::new(Mutex::new(simulation));
    let server = BacnetServer::new(sim_arc.clone(), devices, 47808);

    server.run().await;

    Ok(())
}
