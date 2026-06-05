pub mod models;
pub mod profiles;
pub mod registry;
pub mod seasonality;

use bacnet_rs::object::ObjectType;
use bacnet_rs::property::PropertyValue;
use chrono::Local;

use crate::config::{ConfigError, SimulatorConfig};
use models::SimulatedDevice;
use seasonality::SeasonalityEngine;

pub struct Simulation {
    pub devices: Vec<SimulatedDevice>,
    pub engine: SeasonalityEngine,
}

impl Simulation {
    pub fn new(config: &SimulatorConfig) -> Result<Self, ConfigError> {
        let engine = SeasonalityEngine::new(config.seasonality.weekly_schedule.clone());
        let expanded = config.expand()?;
        let devices = expanded.iter().map(SimulatedDevice::from_spec).collect();
        Ok(Self { devices, engine })
    }

    pub fn total_points(&self) -> usize {
        self.devices.iter().map(|d| d.points.len()).sum()
    }

    pub fn update(&mut self, dt_seconds: f64) {
        let now = Local::now();
        let occupancy = self.engine.get_occupancy(now) as f32;
        let outside_temp = self.engine.get_outside_temp(now) as f32;
        let now_secs = now.timestamp() as f64 + now.timestamp_subsec_micros() as f64 / 1_000_000.0;

        for device in &mut self.devices {
            device.tick(dt_seconds as f32, now_secs, occupancy, outside_temp);
        }
    }

    pub fn present_value(
        &self,
        device_id: u32,
        object_type: ObjectType,
        instance: u32,
    ) -> Option<PropertyValue> {
        let device = self.devices.iter().find(|d| d.device_id == device_id)?;
        let point = device.find_point(object_type, instance)?;
        Some(point.present_value_property())
    }
}
