use bacnet_rs::object::ObjectType;

use crate::simulation::models::SimulatedDevice;

#[derive(Debug, Clone)]
pub struct PointEntry {
    pub object_type: ObjectType,
    pub instance: u32,
    pub label: String,
    pub units: u32,
}

#[derive(Debug, Clone)]
pub struct DeviceEntry {
    pub device_id: u32,
    pub name: String,
    pub points: Vec<PointEntry>,
}

impl DeviceEntry {
    pub fn object_list_len(&self) -> u32 {
        self.points.len() as u32
    }

    pub fn object_list_entry(&self, index: u32) -> Option<(ObjectType, u32)> {
        if index == 0 {
            return None;
        }
        let point = self.points.get((index - 1) as usize)?;
        Some((point.object_type, point.instance))
    }

    pub fn find_point(&self, object_type: ObjectType, instance: u32) -> Option<&PointEntry> {
        self.points
            .iter()
            .find(|point| point.object_type == object_type && point.instance == instance)
    }
}

pub fn build_device_registry(devices: &[SimulatedDevice]) -> Vec<DeviceEntry> {
    devices
        .iter()
        .map(|d| {
            let mut points: Vec<PointEntry> = d
                .points
                .iter()
                .map(|p| PointEntry {
                    object_type: p.object_type,
                    instance: p.instance,
                    label: p.label.clone(),
                    units: p.units,
                })
                .collect();
            points.sort_by_key(|p| {
                let type_key: u32 = p.object_type.into();
                (type_key, p.instance)
            });
            DeviceEntry {
                device_id: d.device_id,
                name: d.name.clone(),
                points,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PointSpec, DeviceSpec};
    use crate::simulation::profiles::ProfileSpec;

    #[test]
    fn registry_preserves_unique_object_ids() {
        let dev = DeviceSpec {
            device_id: 10101,
            name: "AHU-001".into(),
            points: vec![
                PointSpec {
                    label: "sat".into(),
                    object_type: "analog_input".into(),
                    units: None,
                    instance: 1,
                    profile: ProfileSpec::Constant { value: 20.0 },
                },
                PointSpec {
                    label: "rat".into(),
                    object_type: "analog_input".into(),
                    units: None,
                    instance: 2,
                    profile: ProfileSpec::Constant { value: 22.0 },
                },
            ],
        };
        let sim_dev = SimulatedDevice::from_spec(&dev);
        let registry = build_device_registry(std::slice::from_ref(&sim_dev));
        assert_eq!(registry.len(), 1);
        assert_eq!(registry[0].object_list_len(), 2);
        assert_eq!(
            registry[0].object_list_entry(1),
            Some((ObjectType::AnalogInput, 1))
        );
        assert_eq!(
            registry[0].object_list_entry(2),
            Some((ObjectType::AnalogInput, 2))
        );
    }
}
