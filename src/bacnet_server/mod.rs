mod apdu;
mod properties;
mod rpm;
mod whois_compat;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bacnet_rs::app::Apdu;
use bacnet_rs::network::Npdu;
use bacnet_rs::object::{Device, Segmentation};
use bacnet_rs::service::{ConfirmedServiceChoice, IAmRequest, UnconfirmedServiceChoice};
use log::{error, info, warn};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use crate::simulation::registry::DeviceEntry;
use crate::simulation::Simulation;

pub struct BacnetServer {
    simulation: Arc<Mutex<Simulation>>,
    devices: Vec<DeviceEntry>,
    port: u16,
}

impl BacnetServer {
    pub fn new(
        simulation: Arc<Mutex<Simulation>>,
        devices: Vec<DeviceEntry>,
        port: u16,
    ) -> Self {
        Self {
            simulation,
            devices,
            port,
        }
    }

    pub async fn run(&self) {
        let addr = format!("0.0.0.0:{}", self.port);
        let socket = match UdpSocket::bind(&addr).await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to bind to {}: {}", addr, e);
                return;
            }
        };

        if let Err(e) = socket.set_broadcast(true) {
            warn!("Failed to set broadcast on socket: {}", e);
        }

        info!("BACnet Simulator Server listening on {}", addr);

        // BACnet/IP max APDU is 1476, but BVLC/NPDU framing and some clients pad. 4096 is
        // defensive headroom that costs nothing in practice.
        let mut buf = [0u8; 4096];
        loop {
            tokio::select! {
                result = socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, src)) => {
                            self.handle_datagram(&socket, &buf[..len], src).await;
                        }
                        Err(e) => {
                            warn!("Error receiving from socket: {}", e);
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    let mut sim = self.simulation.lock().await;
                    sim.update(1.0);
                }
            }
        }
    }

    async fn handle_datagram(&self, socket: &UdpSocket, data: &[u8], src: SocketAddr) {
        let Some(apdu_bytes) = extract_apdu(data) else {
            return;
        };

        let apdu = match Apdu::decode(&apdu_bytes) {
            Ok(apdu) => apdu,
            Err(_) => return,
        };

        if apdu::is_unconfirmed_whois(&apdu) {
            if let Apdu::UnconfirmedRequest { service_data, .. } = apdu {
                self.handle_whois(service_data, socket, src).await;
            }
            return;
        }

        if let Apdu::ConfirmedRequest {
            invoke_id,
            service_choice,
            service_data,
            ..
        } = apdu
        {
            self.handle_confirmed_request(
                socket,
                src,
                invoke_id,
                service_choice,
                &service_data,
            )
            .await;
        }
    }

    async fn handle_confirmed_request(
        &self,
        socket: &UdpSocket,
        src: SocketAddr,
        invoke_id: u8,
        service_choice: ConfirmedServiceChoice,
        service_data: &[u8],
    ) {
        let sim = self.simulation.lock().await;
        let response = match service_choice {
            ConfirmedServiceChoice::ReadProperty => properties::handle_read_property(
                service_data,
                &self.devices,
                &sim,
            )
            .map(|ack| apdu::build_complex_ack(invoke_id, service_choice, ack)),
            ConfirmedServiceChoice::ReadPropertyMultiple => rpm::handle_read_property_multiple(
                service_data,
                &self.devices,
                &sim,
            )
            .map(|ack| apdu::build_complex_ack(invoke_id, service_choice, ack)),
            _ => {
                info!("Unsupported confirmed service {:?} from {}", service_choice, src);
                None
            }
        };
        drop(sim);

        let Some(apdu_bytes) = response else {
            let error = apdu::build_error_pdu(invoke_id, service_choice, 0, 31);
            if let Ok(packet) = wrap_unicast_npdu(&error) {
                let _ = socket.send_to(&packet, src).await;
            }
            return;
        };

        if let Ok(packet) = wrap_unicast_npdu(&apdu_bytes) {
            if let Err(e) = socket.send_to(&packet, src).await {
                error!("Failed to send confirmed ack to {}: {}", src, e);
            }
        }
    }

    async fn handle_whois(&self, service_data: Vec<u8>, socket: &UdpSocket, src: SocketAddr) {
        let whois = whois_compat::decode_whois(&service_data);

        // Collect responses while holding the simulation lock, then release it before
        // the staggered send phase so simulation ticks aren't blocked.
        let responses: Vec<(u32, Vec<u8>)> = {
            let sim = self.simulation.lock().await;
            sim.devices
                .iter()
                .filter(|device| whois.matches(device.device_id))
                .filter_map(|device| {
                    create_iam_response(device.device_id, &device.name)
                        .ok()
                        .map(|bytes| (device.device_id, bytes))
                })
                .collect()
        };

        info!(
            "Responding to Who-Is from {}: {} device(s)",
            src,
            responses.len()
        );

        // Stagger I-Am sends so a 200+ device burst doesn't overflow the requester's
        // kernel UDP recv buffer. ~500us per packet keeps the full sweep under 200ms for
        // 400 devices, well inside any sensible discovery window.
        for (device_id, response) in responses {
            if let Err(e) = socket.send_to(&response, src).await {
                error!("Failed to send I-Am for device {}: {}", device_id, e);
            }
            tokio::time::sleep(Duration::from_micros(500)).await;
        }
    }
}

fn extract_apdu(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 4 || data[0] != 0x81 {
        return None;
    }

    let bvlc_function = data[1];
    let bvlc_length = ((data[2] as u16) << 8) | (data[3] as u16);
    if data.len() != bvlc_length as usize {
        return None;
    }

    let npdu_start = match bvlc_function {
        0x0A | 0x0B => 4,
        0x04 => 10,
        _ => return None,
    };

    if data.len() <= npdu_start {
        return None;
    }

    let (_npdu, npdu_len) = Npdu::decode(&data[npdu_start..]).ok()?;
    let apdu_start = npdu_start + npdu_len;
    if data.len() <= apdu_start {
        return None;
    }

    Some(data[apdu_start..].to_vec())
}

fn create_iam_response(
    device_id: u32,
    name: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut device = Device::new(device_id, name.to_string());
    device.set_vendor_by_id(260)?;

    let iam = IAmRequest::new(
        device.identifier,
        1476,
        Segmentation::Both,
        device.vendor_identifier,
    );

    let mut iam_buffer = Vec::new();
    iam.encode(&mut iam_buffer)?;

    let mut apdu = vec![0x10];
    apdu.push(UnconfirmedServiceChoice::IAm as u8);
    apdu.extend_from_slice(&iam_buffer);

    wrap_unicast_npdu(&apdu)
}

fn wrap_unicast_npdu(apdu: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut npdu = Npdu::new();
    npdu.control.priority = 0;
    let mut message = npdu.encode();
    message.extend_from_slice(apdu);

    let mut bvlc_message = vec![0x81, 0x0A, 0x00, 0x00];
    bvlc_message.extend_from_slice(&message);
    let total_len = bvlc_message.len() as u16;
    bvlc_message[2] = (total_len >> 8) as u8;
    bvlc_message[3] = (total_len & 0xFF) as u8;
    Ok(bvlc_message)
}
