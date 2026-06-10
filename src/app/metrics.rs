use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Default)]
pub struct AppMetrics {
    pub who_is: AtomicU64,
    pub read_property: AtomicU64,
    pub read_property_multiple: AtomicU64,
    pub listening: AtomicBool,
    last_client: Mutex<Option<(SocketAddr, Instant)>>,
}

impl AppMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_who_is(&self, src: SocketAddr) {
        self.who_is.fetch_add(1, Ordering::Relaxed);
        self.set_last_client(src);
    }

    pub fn record_read_property(&self, src: SocketAddr) {
        self.read_property.fetch_add(1, Ordering::Relaxed);
        self.set_last_client(src);
    }

    pub fn record_read_property_multiple(&self, src: SocketAddr) {
        self.read_property_multiple.fetch_add(1, Ordering::Relaxed);
        self.set_last_client(src);
    }

    pub fn set_listening(&self, listening: bool) {
        self.listening.store(listening, Ordering::Relaxed);
    }

    fn set_last_client(&self, src: SocketAddr) {
        if let Ok(mut guard) = self.last_client.lock() {
            *guard = Some((src, Instant::now()));
        }
    }

    pub fn last_client(&self) -> Option<(SocketAddr, Instant)> {
        self.last_client.lock().ok().and_then(|g| *g)
    }

    pub fn who_is_count(&self) -> u64 {
        self.who_is.load(Ordering::Relaxed)
    }

    pub fn read_property_count(&self) -> u64 {
        self.read_property.load(Ordering::Relaxed)
    }

    pub fn read_property_multiple_count(&self) -> u64 {
        self.read_property_multiple.load(Ordering::Relaxed)
    }

    pub fn is_listening(&self) -> bool {
        self.listening.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn metrics_increment() {
        let m = AppMetrics::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 47808);
        m.record_who_is(addr);
        m.record_read_property(addr);
        m.record_read_property_multiple(addr);
        assert_eq!(m.who_is_count(), 1);
        assert_eq!(m.read_property_count(), 1);
        assert_eq!(m.read_property_multiple_count(), 1);
        assert!(m.last_client().is_some());
    }
}
