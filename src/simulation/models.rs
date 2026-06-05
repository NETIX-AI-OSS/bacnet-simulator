use std::collections::HashMap;

use bacnet_rs::object::ObjectType;
use bacnet_rs::property::PropertyValue;

use crate::config::{DeviceSpec, PointSpec};
use crate::simulation::profiles::{PointValue, ProfileState, TickCtx};

#[derive(Debug, Clone)]
pub struct SimulatedPoint {
    pub label: String,
    pub object_type: ObjectType,
    pub instance: u32,
    pub units: u32,
    pub value: PointValue,
    pub profile: ProfileState,
}

impl SimulatedPoint {
    pub fn from_spec(spec: &PointSpec) -> Option<Self> {
        let object_type = object_type_from_str(&spec.object_type)?;
        let profile = ProfileState::from_spec(&spec.profile);
        let value = profile.initial_value();
        Some(SimulatedPoint {
            label: spec.label.clone(),
            object_type,
            instance: spec.instance,
            units: units_from_str(spec.units.as_deref()),
            value,
            profile,
        })
    }

    pub fn present_value_property(&self) -> PropertyValue {
        match self.value {
            PointValue::Real(v) => PropertyValue::Real(v),
            PointValue::Boolean(b) => PropertyValue::Boolean(b),
            PointValue::Unsigned(u) => PropertyValue::Unsigned(u as u64),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimulatedDevice {
    pub device_id: u32,
    pub name: String,
    pub points: Vec<SimulatedPoint>,
}

impl SimulatedDevice {
    pub fn from_spec(spec: &DeviceSpec) -> Self {
        let points = spec
            .points
            .iter()
            .filter_map(SimulatedPoint::from_spec)
            .collect();
        SimulatedDevice {
            device_id: spec.device_id,
            name: spec.name.clone(),
            points,
        }
    }

    pub fn tick(&mut self, dt: f32, now_secs: f64, occupancy: f32, outside_temp: f32) {
        let mut siblings: HashMap<String, f32> = HashMap::with_capacity(self.points.len());
        // Pre-seed with current values so DerivedConstant/Integrator referencing yet-unticked
        // points still get a stable starting value.
        for p in &self.points {
            if let Some(v) = p.value.as_f32() {
                siblings.insert(p.label.clone(), v);
            }
        }
        for p in &mut self.points {
            let ctx = TickCtx {
                dt,
                now_secs,
                occupancy,
                outside_temp,
                siblings: &siblings,
            };
            let new_value = p.profile.tick(&ctx);
            p.value = new_value;
            if let Some(v) = new_value.as_f32() {
                siblings.insert(p.label.clone(), v);
            }
        }
    }

    pub fn find_point(&self, object_type: ObjectType, instance: u32) -> Option<&SimulatedPoint> {
        self.points
            .iter()
            .find(|p| p.object_type == object_type && p.instance == instance)
    }
}

pub fn object_type_from_str(value: &str) -> Option<ObjectType> {
    match value.trim().to_ascii_lowercase().as_str() {
        "analog_input" => Some(ObjectType::AnalogInput),
        "analog_output" => Some(ObjectType::AnalogOutput),
        "analog_value" => Some(ObjectType::AnalogValue),
        "binary_input" => Some(ObjectType::BinaryInput),
        "binary_output" => Some(ObjectType::BinaryOutput),
        "binary_value" => Some(ObjectType::BinaryValue),
        "multi_state_input" => Some(ObjectType::MultiStateInput),
        "multi_state_output" => Some(ObjectType::MultiStateOutput),
        "multi_state_value" => Some(ObjectType::MultiStateValue),
        _ => None,
    }
}

pub fn units_from_str(value: Option<&str>) -> u32 {
    // BACnet engineering-units enumeration (ANSI/ASHRAE Std 135).
    match value.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("degrees_celsius") => 62,
        Some("degrees_fahrenheit") => 64,
        Some("degrees_kelvin") => 63,
        Some("percent") => 98,
        Some("percent_relative_humidity") => 29,
        Some("parts_per_million") => 96,
        Some("parts_per_billion") => 97,
        Some("watts") => 47,
        Some("kilowatts") => 48,
        Some("kilovolt_amperes") => 49,
        Some("kilovolt_amperes_reactive") => 52,
        Some("watt_hours") => 18,
        Some("kilowatt_hours") => 19,
        Some("volts") => 5,
        Some("amperes") => 3,
        Some("hertz") => 27,
        Some("pascals") => 53,
        Some("kilopascals") => 54,
        Some("bar") => 55,
        Some("cubic_feet_per_minute") => 84,
        Some("liters_per_second") => 87,
        Some("cubic_meters_per_hour") => 135,
        Some("cubic_meters") => 80,
        Some("liters") => 82,
        Some("meters_per_second") => 74,
        Some("minutes") => 72,
        Some("hours") => 71,
        Some("seconds") => 73,
        Some("no_units") | None => 95,
        Some(_) => 95,
    }
}
