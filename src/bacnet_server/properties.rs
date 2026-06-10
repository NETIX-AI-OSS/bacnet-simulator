use std::collections::HashMap;

use bacnet_rs::object::{ObjectIdentifier, ObjectType, PropertyIdentifier};
use bacnet_rs::property::{self, PropertyValue};
use bacnet_rs::service::{ReadPropertyRequest, ReadPropertyResponse};

use crate::simulation::Simulation;
use crate::simulation::registry::{DeviceEntry, PointEntry};

use super::{MAX_APDU_LENGTH, VENDOR_ID};

/// A lightweight description of a single property read operation.
///
/// This struct is non-pub: it exists only as a convenience inside the `bacnet_server`
/// module.  `rpm.rs` constructs it directly from `bacnet-types` values; within this
/// file `handle_read_property` builds it from a `ReadPropertyRequest`.
pub(super) struct PropertyRead {
    pub object_identifier: ObjectIdentifier,
    pub property_identifier: PropertyIdentifier,
    pub property_array_index: Option<u32>,
}

impl PropertyRead {
    pub fn from_request(request: &ReadPropertyRequest) -> Self {
        Self {
            object_identifier: request.object_identifier,
            property_identifier: request.property_identifier,
            property_array_index: request.property_array_index,
        }
    }
}

pub fn handle_read_property(
    service_data: &[u8],
    devices: &[DeviceEntry],
    simulation: &Simulation,
) -> Option<Vec<u8>> {
    let request = ReadPropertyRequest::decode(service_data).ok()?;
    let value = resolve_property_read(&PropertyRead::from_request(&request), devices, simulation)?;
    let response = ReadPropertyResponse {
        object_identifier: request.object_identifier,
        property_identifier: request.property_identifier,
        property_array_index: request.property_array_index,
        property_values: vec![value],
    };
    let mut ack = Vec::new();
    response.encode(&mut ack).ok()?;
    Some(ack)
}

/// Build an O(1) device-id index from a flat device slice.
///
/// The index is constructed once per request in the hot path.  For the typical use-case
/// of 200–300 devices the allocation is cheap and keeps the per-property lookup at O(1)
/// rather than O(n).
fn index_devices_by_id(devices: &[DeviceEntry]) -> HashMap<u32, &DeviceEntry> {
    devices.iter().map(|d| (d.device_id, d)).collect()
}

pub fn resolve_property_read(
    read: &PropertyRead,
    devices: &[DeviceEntry],
    simulation: &Simulation,
) -> Option<PropertyValue> {
    let object_type = read.object_identifier.object_type;
    let instance = read.object_identifier.instance;

    if object_type == ObjectType::Device {
        // O(1) device lookup by id.
        let index = index_devices_by_id(devices);
        let device = index.get(&instance)?;
        return read_device_property(device, read.property_identifier, read.property_array_index);
    }

    // Point lookup: iterate devices once to find the point owner.  In a future revision
    // this can be replaced by a (object_type, instance) → (device_id, point_idx) map
    // built at startup.
    for device in devices {
        let Some(point) = device.find_point(object_type, instance) else {
            continue;
        };
        return read_point_property(
            simulation,
            device.device_id,
            &device.name,
            point,
            read.property_identifier,
        );
    }

    None
}

pub fn encode_property_value_bytes(value: &PropertyValue) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    property::encode_property_value(value, &mut bytes).ok()?;
    Some(bytes)
}

fn read_device_property(
    device: &DeviceEntry,
    property: PropertyIdentifier,
    array_index: Option<u32>,
) -> Option<PropertyValue> {
    match property {
        PropertyIdentifier::ObjectList => match array_index {
            Some(0) => Some(PropertyValue::Unsigned(device.object_list_len() as u64)),
            Some(index) => {
                let (object_type, instance) = device.object_list_entry(index)?;
                Some(PropertyValue::ObjectIdentifier(ObjectIdentifier::new(
                    object_type,
                    instance,
                )))
            }
            None => None,
        },
        PropertyIdentifier::ObjectName => Some(PropertyValue::CharacterString(device.name.clone())),
        PropertyIdentifier::ObjectIdentifier => Some(PropertyValue::ObjectIdentifier(
            ObjectIdentifier::new(ObjectType::Device, device.device_id),
        )),
        PropertyIdentifier::VendorIdentifier => Some(PropertyValue::Unsigned(VENDOR_ID as u64)),
        PropertyIdentifier::MaxApduLengthAccepted => {
            Some(PropertyValue::Unsigned(MAX_APDU_LENGTH as u64))
        }
        // 4 = NoSegmentation. Clients should chunk RPMs; the server does not segment.
        PropertyIdentifier::SegmentationSupported => Some(PropertyValue::Enumerated(4)),
        _ => None,
    }
}

fn read_point_property(
    simulation: &Simulation,
    device_id: u32,
    device_name: &str,
    point: &PointEntry,
    property: PropertyIdentifier,
) -> Option<PropertyValue> {
    match property {
        PropertyIdentifier::ObjectName => Some(PropertyValue::CharacterString(format!(
            "{} {}",
            device_name,
            point.label.replace('_', " ")
        ))),
        PropertyIdentifier::Description => {
            Some(PropertyValue::CharacterString(point.label.clone()))
        }
        PropertyIdentifier::PresentValue => {
            simulation.present_value(device_id, point.object_type, point.instance)
        }
        PropertyIdentifier::Units => Some(PropertyValue::Enumerated(point.units)),
        PropertyIdentifier::ObjectIdentifier => Some(PropertyValue::ObjectIdentifier(
            ObjectIdentifier::new(point.object_type, point.instance),
        )),
        PropertyIdentifier::ObjectType => {
            let raw: u32 = point.object_type.into();
            Some(PropertyValue::Enumerated(raw))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        BuildingConfig, IdPolicy, PointSpec, SeasonalityConfig, SimulatorConfig, WeeklySchedule,
    };
    use crate::simulation::Simulation;
    use crate::simulation::profiles::ProfileSpec;
    use crate::simulation::registry::build_device_registry;
    use bacnet_rs::object::ObjectType;
    use std::collections::HashMap;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn single_device_config(points: Vec<PointSpec>) -> SimulatorConfig {
        let mut templates = HashMap::new();
        templates.insert(
            "tpl".to_string(),
            crate::config::AssetTemplate {
                description: String::new(),
                points: points
                    .iter()
                    .map(|p| crate::config::TemplatePointSpec {
                        label: p.label.clone(),
                        object_type: p.object_type.clone(),
                        units: p.units.clone(),
                        profile: p.profile.clone(),
                    })
                    .collect(),
            },
        );
        SimulatorConfig {
            building: BuildingConfig {
                name: "Test Building".into(),
                location: None,
                timezone: None,
            },
            seasonality: SeasonalityConfig {
                weekly_schedule: WeeklySchedule {
                    weekday_occupancy: vec![],
                    weekend_occupancy: vec![],
                },
            },
            id_policy: IdPolicy {
                device_id_base: 1000,
                per_template_block: 100,
            },
            templates,
            instances: vec![crate::config::AssetInstanceSpec {
                template: "tpl".into(),
                name_prefix: "DEV".into(),
                zone: None,
                count: 1,
            }],
        }
    }

    fn make_simulation_and_registry(points: Vec<PointSpec>) -> (Simulation, Vec<DeviceEntry>) {
        let cfg = single_device_config(points);
        let sim = Simulation::new(&cfg).expect("simulation");
        let registry = build_device_registry(&sim.devices);
        (sim, registry)
    }

    // -----------------------------------------------------------------------
    // read_device_property
    // -----------------------------------------------------------------------

    #[test]
    fn device_object_name_returns_device_name() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let read = PropertyRead {
            object_identifier: ObjectIdentifier::new(ObjectType::Device, device.device_id),
            property_identifier: PropertyIdentifier::ObjectName,
            property_array_index: None,
        };
        let result = resolve_property_read(&read, &registry, &sim);
        assert!(
            matches!(result, Some(PropertyValue::CharacterString(ref s)) if s.starts_with("DEV"))
        );
    }

    #[test]
    fn device_object_identifier_returns_device_id() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let read = PropertyRead {
            object_identifier: ObjectIdentifier::new(ObjectType::Device, device.device_id),
            property_identifier: PropertyIdentifier::ObjectIdentifier,
            property_array_index: None,
        };
        let result = resolve_property_read(&read, &registry, &sim);
        assert!(
            matches!(result, Some(PropertyValue::ObjectIdentifier(oid)) if oid.instance == device.device_id)
        );
    }

    #[test]
    fn device_vendor_identifier_returns_constant() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let read = PropertyRead {
            object_identifier: ObjectIdentifier::new(ObjectType::Device, device.device_id),
            property_identifier: PropertyIdentifier::VendorIdentifier,
            property_array_index: None,
        };
        let result = resolve_property_read(&read, &registry, &sim);
        assert_eq!(result, Some(PropertyValue::Unsigned(VENDOR_ID as u64)));
    }

    #[test]
    fn device_max_apdu_returns_constant() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let read = PropertyRead {
            object_identifier: ObjectIdentifier::new(ObjectType::Device, device.device_id),
            property_identifier: PropertyIdentifier::MaxApduLengthAccepted,
            property_array_index: None,
        };
        let result = resolve_property_read(&read, &registry, &sim);
        assert_eq!(
            result,
            Some(PropertyValue::Unsigned(MAX_APDU_LENGTH as u64))
        );
    }

    #[test]
    fn device_segmentation_supported_returns_4() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let read = PropertyRead {
            object_identifier: ObjectIdentifier::new(ObjectType::Device, device.device_id),
            property_identifier: PropertyIdentifier::SegmentationSupported,
            property_array_index: None,
        };
        let result = resolve_property_read(&read, &registry, &sim);
        assert_eq!(result, Some(PropertyValue::Enumerated(4)));
    }

    #[test]
    fn device_object_list_index_0_returns_count() {
        let (sim, registry) = make_simulation_and_registry(vec![
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
        ]);

        let device = &registry[0];
        let read = PropertyRead {
            object_identifier: ObjectIdentifier::new(ObjectType::Device, device.device_id),
            property_identifier: PropertyIdentifier::ObjectList,
            property_array_index: Some(0),
        };
        let result = resolve_property_read(&read, &registry, &sim);
        assert_eq!(result, Some(PropertyValue::Unsigned(2)));
    }

    #[test]
    fn device_object_list_index_none_returns_none() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let read = PropertyRead {
            object_identifier: ObjectIdentifier::new(ObjectType::Device, device.device_id),
            property_identifier: PropertyIdentifier::ObjectList,
            property_array_index: None,
        };
        assert!(resolve_property_read(&read, &registry, &sim).is_none());
    }

    #[test]
    fn device_unknown_property_returns_none() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let read = PropertyRead {
            object_identifier: ObjectIdentifier::new(ObjectType::Device, device.device_id),
            // Use a property identifier value that is not handled.
            property_identifier: PropertyIdentifier::from(9999u32),
            property_array_index: None,
        };
        assert!(resolve_property_read(&read, &registry, &sim).is_none());
    }

    // -----------------------------------------------------------------------
    // read_point_property
    // -----------------------------------------------------------------------

    #[test]
    fn point_object_name_combines_device_name_and_label() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "supply_air_temp".into(),
            object_type: "analog_input".into(),
            units: Some("degrees_celsius".into()),
            instance: 1,
            profile: ProfileSpec::Constant { value: 18.0 },
        }]);

        let device = &registry[0];
        let point = device.find_point(ObjectType::AnalogInput, 1).unwrap();
        let result = read_point_property(
            &sim,
            device.device_id,
            &device.name,
            point,
            PropertyIdentifier::ObjectName,
        );
        if let Some(PropertyValue::CharacterString(name)) = result {
            assert!(name.contains("supply air temp"), "got: {name}");
        } else {
            panic!("expected CharacterString");
        }
    }

    #[test]
    fn point_description_returns_label() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "co2_level".into(),
            object_type: "analog_input".into(),
            units: Some("parts_per_million".into()),
            instance: 1,
            profile: ProfileSpec::Constant { value: 400.0 },
        }]);

        let device = &registry[0];
        let point = device.find_point(ObjectType::AnalogInput, 1).unwrap();
        let result = read_point_property(
            &sim,
            device.device_id,
            &device.name,
            point,
            PropertyIdentifier::Description,
        );
        assert_eq!(
            result,
            Some(PropertyValue::CharacterString("co2_level".into()))
        );
    }

    #[test]
    fn point_present_value_returns_real() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 42.0 },
        }]);

        let device = &registry[0];
        let point = device.find_point(ObjectType::AnalogInput, 1).unwrap();
        let result = read_point_property(
            &sim,
            device.device_id,
            &device.name,
            point,
            PropertyIdentifier::PresentValue,
        );
        assert!(matches!(result, Some(PropertyValue::Real(_))));
    }

    #[test]
    fn point_present_value_unsigned_for_multi_state() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "mode".into(),
            object_type: "multi_state_value".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::ConstantState { value: 3 },
        }]);

        let device = &registry[0];
        let point = device.find_point(ObjectType::MultiStateValue, 1).unwrap();
        let result = read_point_property(
            &sim,
            device.device_id,
            &device.name,
            point,
            PropertyIdentifier::PresentValue,
        );
        assert!(matches!(result, Some(PropertyValue::Unsigned(_))));
    }

    #[test]
    fn point_units_returns_enumerated() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "temp".into(),
            object_type: "analog_input".into(),
            units: Some("degrees_celsius".into()),
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let point = device.find_point(ObjectType::AnalogInput, 1).unwrap();
        let result = read_point_property(
            &sim,
            device.device_id,
            &device.name,
            point,
            PropertyIdentifier::Units,
        );
        assert_eq!(result, Some(PropertyValue::Enumerated(62))); // degrees_celsius = 62
    }

    #[test]
    fn point_object_identifier_returns_object_id() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let point = device.find_point(ObjectType::AnalogInput, 1).unwrap();
        let result = read_point_property(
            &sim,
            device.device_id,
            &device.name,
            point,
            PropertyIdentifier::ObjectIdentifier,
        );
        assert!(matches!(result, Some(PropertyValue::ObjectIdentifier(oid)) if oid.instance == 1));
    }

    #[test]
    fn point_object_type_returns_enumerated() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let point = device.find_point(ObjectType::AnalogInput, 1).unwrap();
        let result = read_point_property(
            &sim,
            device.device_id,
            &device.name,
            point,
            PropertyIdentifier::ObjectType,
        );
        // AnalogInput raw value = 0
        assert_eq!(result, Some(PropertyValue::Enumerated(0)));
    }

    #[test]
    fn point_unknown_property_returns_none() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let device = &registry[0];
        let point = device.find_point(ObjectType::AnalogInput, 1).unwrap();
        let result = read_point_property(
            &sim,
            device.device_id,
            &device.name,
            point,
            PropertyIdentifier::from(9999u32),
        );
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // resolve_property_read — unknown device/point returns None
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_returns_none_for_unknown_device_id() {
        let (sim, registry) = make_simulation_and_registry(vec![PointSpec {
            label: "sat".into(),
            object_type: "analog_input".into(),
            units: None,
            instance: 1,
            profile: ProfileSpec::Constant { value: 20.0 },
        }]);

        let read = PropertyRead {
            object_identifier: ObjectIdentifier::new(ObjectType::Device, 99999),
            property_identifier: PropertyIdentifier::ObjectName,
            property_array_index: None,
        };
        assert!(resolve_property_read(&read, &registry, &sim).is_none());
    }
}
