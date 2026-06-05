use bacnet_rs::object::{ObjectIdentifier, ObjectType, PropertyIdentifier};
use bacnet_rs::property::{self, PropertyValue};
use bacnet_rs::service::{ReadPropertyRequest, ReadPropertyResponse};

use crate::simulation::registry::{DeviceEntry, PointEntry};
use crate::simulation::Simulation;

#[derive(Debug, Clone)]
pub struct PropertyRead {
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
    let value = resolve_property_read(
        &PropertyRead::from_request(&request),
        devices,
        simulation,
    )?;
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

pub fn resolve_property_read(
    read: &PropertyRead,
    devices: &[DeviceEntry],
    simulation: &Simulation,
) -> Option<PropertyValue> {
    let object_type = read.object_identifier.object_type;
    let instance = read.object_identifier.instance;

    if object_type == ObjectType::Device {
        let device = devices.iter().find(|entry| entry.device_id == instance)?;
        return read_device_property(
            device,
            read.property_identifier,
            read.property_array_index,
        );
    }

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
                    object_type, instance,
                )))
            }
            None => None,
        },
        PropertyIdentifier::ObjectName => Some(PropertyValue::CharacterString(device.name.clone())),
        PropertyIdentifier::ObjectIdentifier => Some(PropertyValue::ObjectIdentifier(
            ObjectIdentifier::new(ObjectType::Device, device.device_id),
        )),
        PropertyIdentifier::VendorIdentifier => Some(PropertyValue::Unsigned(260)),
        PropertyIdentifier::MaxApduLengthAccepted => Some(PropertyValue::Unsigned(1476)),
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
        PropertyIdentifier::Description => Some(PropertyValue::CharacterString(point.label.clone())),
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
