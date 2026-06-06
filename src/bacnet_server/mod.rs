mod apdu;
mod properties;
mod rpm;
mod whois_compat;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bacnet_rs::app::Apdu;
use bacnet_rs::network::Npdu;
use bacnet_rs::object::{Device, Segmentation};
use bacnet_rs::service::{ConfirmedServiceChoice, IAmRequest, UnconfirmedServiceChoice};
use log::{error, info, warn};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use crate::simulation::Simulation;
use crate::simulation::registry::DeviceEntry;

/// BACnet vendor identifier assigned to this simulator.
pub const VENDOR_ID: u32 = 260;

/// Maximum APDU length advertised in I-Am and device property responses.
/// BACnet/IP single-segment ceiling (1476 bytes = 1500 MTU − 14 Ethernet − 20 IP − 8 UDP − BVLC/NPDU overhead).
pub const MAX_APDU_LENGTH: u32 = 1476;

pub struct BacnetServer {
    simulation: Arc<Mutex<Simulation>>,
    devices: Vec<DeviceEntry>,
    port: u16,
}

impl BacnetServer {
    pub fn new(simulation: Arc<Mutex<Simulation>>, devices: Vec<DeviceEntry>, port: u16) -> Self {
        Self {
            simulation,
            devices,
            port,
        }
    }

    pub async fn run(&self) {
        let addr = format!("0.0.0.0:{}", self.port);
        let socket = match UdpSocket::bind(&addr).await {
            Ok(s) => Arc::new(s),
            Err(e) => {
                error!("Failed to bind to {}: {}", addr, e);
                return;
            }
        };

        if let Err(e) = socket.set_broadcast(true) {
            warn!("Failed to set broadcast on socket: {}", e);
        }

        info!("BACnet Simulator Server listening on {}", addr);

        // Spawn a dedicated simulation tick task that fires every second regardless of
        // BACnet packet load. Using Instant for elapsed time gives accurate dt even when
        // ticks are slightly late.
        let sim_clone = Arc::clone(&self.simulation);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            let mut last_tick = Instant::now();
            loop {
                interval.tick().await;
                let elapsed = last_tick.elapsed().as_secs_f64();
                last_tick = Instant::now();
                let mut sim = sim_clone.lock().await;
                sim.update(elapsed);
            }
        });

        // BACnet/IP max APDU is MAX_APDU_LENGTH, but BVLC/NPDU framing and some clients pad.
        // 4096 is defensive headroom that costs nothing in practice.
        let mut buf = [0u8; 4096];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, src)) => {
                    self.handle_datagram(&socket, &buf[..len], src).await;
                }
                Err(e) => {
                    warn!("Error receiving from socket: {}", e);
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
            self.handle_confirmed_request(socket, src, invoke_id, service_choice, &service_data)
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
            ConfirmedServiceChoice::ReadProperty => {
                properties::handle_read_property(service_data, &self.devices, &sim)
                    .map(|ack| apdu::build_complex_ack(invoke_id, service_choice, ack))
            }
            ConfirmedServiceChoice::ReadPropertyMultiple => {
                rpm::handle_read_property_multiple(service_data, &self.devices, &sim)
                    .map(|ack| apdu::build_complex_ack(invoke_id, service_choice, ack))
            }
            _ => {
                info!(
                    "Unsupported confirmed service {:?} from {}",
                    service_choice, src
                );
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

        if let Ok(packet) = wrap_unicast_npdu(&apdu_bytes)
            && let Err(e) = socket.send_to(&packet, src).await
        {
            error!("Failed to send confirmed ack to {}: {}", src, e);
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

fn create_iam_response(device_id: u32, name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut device = Device::new(device_id, name.to_string());
    device.set_vendor_by_id(VENDOR_ID as u16)?;

    let iam = IAmRequest::new(
        device.identifier,
        MAX_APDU_LENGTH,
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

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers — build minimal BVLC frames for extract_apdu tests.
    // -----------------------------------------------------------------------

    /// Build a well-formed Original-Unicast-NPDU (0x0A) or Broadcast (0x0B) BVLC frame.
    /// `npdu_apdu` is the raw NPDU+APDU bytes (no BVLC header).
    fn bvlc_frame(function: u8, npdu_apdu: &[u8]) -> Vec<u8> {
        let total = 4 + npdu_apdu.len();
        let mut frame = vec![0x81, function, (total >> 8) as u8, (total & 0xFF) as u8];
        frame.extend_from_slice(npdu_apdu);
        frame
    }

    /// Build a minimal NPDU (single byte 0x00 = no special control flags) followed by a
    /// synthetic APDU byte sequence.
    fn minimal_npdu_apdu(apdu: &[u8]) -> Vec<u8> {
        // NPDU version + control (2 bytes minimum decoded by bacnet-rs Npdu::decode).
        let mut v = vec![0x01, 0x00]; // version=1, control=0
        v.extend_from_slice(apdu);
        v
    }

    // -----------------------------------------------------------------------
    // extract_apdu
    // -----------------------------------------------------------------------

    #[test]
    fn extract_apdu_rejects_non_0x81_leading_byte() {
        let mut frame = bvlc_frame(0x0A, &minimal_npdu_apdu(&[0x10, 0x08]));
        frame[0] = 0x82;
        assert!(extract_apdu(&frame).is_none());
    }

    #[test]
    fn extract_apdu_rejects_frame_too_short() {
        assert!(extract_apdu(&[]).is_none());
        assert!(extract_apdu(&[0x81, 0x0A, 0x00]).is_none()); // only 3 bytes
    }

    #[test]
    fn extract_apdu_rejects_length_mismatch() {
        let npdu_apdu = minimal_npdu_apdu(&[0x10, 0x08]);
        let mut frame = bvlc_frame(0x0A, &npdu_apdu);
        // Corrupt declared length to be one byte longer than actual.
        let real_len = frame.len() as u16;
        frame[2] = ((real_len + 1) >> 8) as u8;
        frame[3] = ((real_len + 1) & 0xFF) as u8;
        assert!(extract_apdu(&frame).is_none());
    }

    #[test]
    fn extract_apdu_rejects_unknown_bvlc_function() {
        let npdu_apdu = minimal_npdu_apdu(&[0x10, 0x08]);
        let frame = bvlc_frame(0xFF, &npdu_apdu);
        assert!(extract_apdu(&frame).is_none());
    }

    #[test]
    fn extract_apdu_valid_0x0a_original_unicast() {
        let apdu_bytes = &[0x10, 0x08]; // unconfirmed WhoIs-like marker
        let npdu_apdu = minimal_npdu_apdu(apdu_bytes);
        let frame = bvlc_frame(0x0A, &npdu_apdu);
        let result = extract_apdu(&frame);
        assert!(result.is_some(), "expected Some for valid 0x0A frame");
        assert_eq!(result.unwrap(), apdu_bytes);
    }

    #[test]
    fn extract_apdu_valid_0x0b_original_broadcast() {
        let apdu_bytes = &[0x10, 0x08];
        let npdu_apdu = minimal_npdu_apdu(apdu_bytes);
        let frame = bvlc_frame(0x0B, &npdu_apdu);
        let result = extract_apdu(&frame);
        assert!(result.is_some(), "expected Some for valid 0x0B frame");
        assert_eq!(result.unwrap(), apdu_bytes);
    }

    #[test]
    fn extract_apdu_valid_0x04_forwarded_npdu() {
        // 0x04 frames have an extra 6-byte originating address inserted after the 4-byte
        // BVLC header, so npdu_start = 10 (4 BVLC + 6 address).
        let apdu_bytes = &[0x10, 0x08];
        // Build inner payload: 6 dummy address bytes + NPDU + APDU
        let mut inner = vec![0u8; 6]; // originating address placeholder
        inner.extend_from_slice(&minimal_npdu_apdu(apdu_bytes));
        let frame = bvlc_frame(0x04, &inner);
        let result = extract_apdu(&frame);
        assert!(result.is_some(), "expected Some for valid 0x04 frame");
        assert_eq!(result.unwrap(), apdu_bytes);
    }

    #[test]
    fn extract_apdu_rejects_truncated_npdu() {
        // A frame where BVLC length is consistent but there is no payload after the BVLC header.
        let frame = vec![0x81u8, 0x0A, 0x00, 0x04]; // length=4, nothing after header
        assert!(extract_apdu(&frame).is_none());
    }

    // -----------------------------------------------------------------------
    // Constants sanity checks
    // -----------------------------------------------------------------------

    #[test]
    fn constants_have_expected_values() {
        assert_eq!(VENDOR_ID, 260);
        assert_eq!(MAX_APDU_LENGTH, 1476);
    }

    // -----------------------------------------------------------------------
    // wrap_unicast_npdu
    // -----------------------------------------------------------------------

    #[test]
    fn wrap_unicast_npdu_produces_valid_bvlc_frame() {
        let apdu = vec![0x10, 0x08];
        let frame = wrap_unicast_npdu(&apdu).expect("wrap should succeed");
        // Leading byte must be 0x81
        assert_eq!(frame[0], 0x81);
        // BVLC function 0x0A = Original-Unicast-NPDU
        assert_eq!(frame[1], 0x0A);
        // Declared length matches actual
        let declared_len = ((frame[2] as u16) << 8) | frame[3] as u16;
        assert_eq!(declared_len as usize, frame.len());
    }
}
