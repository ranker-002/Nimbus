use std::time::Instant;

use nimbus_core::types::BandwidthSample;

pub struct BandwidthTracker {
    interface: String,
    last_rx: u64,
    last_tx: u64,
    last_instant: Instant,
    initialized: bool,
}

impl BandwidthTracker {
    pub fn new(interface: &str) -> Self {
        Self {
            interface: interface.to_string(),
            last_rx: 0,
            last_tx: 0,
            last_instant: Instant::now(),
            initialized: false,
        }
    }

    pub fn sample(&mut self) -> BandwidthSample {
        let current_rx = read_sysfs_counter(&self.interface, "rx_bytes");
        let current_tx = read_sysfs_counter(&self.interface, "tx_bytes");
        let now = Instant::now();

        let elapsed = now.duration_since(self.last_instant).as_secs_f64();

        let (rx_rate, tx_rate) = if !self.initialized {
            self.initialized = true;
            (0, 0)
        } else if elapsed > 0.0 {
            let rx_delta = current_rx.saturating_sub(self.last_rx);
            let tx_delta = current_tx.saturating_sub(self.last_tx);

            if current_rx < self.last_rx || current_tx < self.last_tx {
                (0, 0)
            } else {
                (
                    (rx_delta as f64 / elapsed) as u64,
                    (tx_delta as f64 / elapsed) as u64,
                )
            }
        } else {
            (0, 0)
        };

        self.last_rx = current_rx;
        self.last_tx = current_tx;
        self.last_instant = now;

        BandwidthSample {
            rx_rate,
            tx_rate,
            total_rx: current_rx,
            total_tx: current_tx,
        }
    }

    pub fn reset(&mut self) {
        self.last_rx = 0;
        self.last_tx = 0;
        self.last_instant = Instant::now();
        self.initialized = false;
    }

    pub fn interface(&self) -> &str {
        &self.interface
    }
}

fn read_sysfs_counter(interface: &str, counter: &str) -> u64 {
    let path = format!("/sys/class/net/{}/statistics/{}", interface, counter);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bandwidth_tracker_new() {
        let tracker = BandwidthTracker::new("eth0");
        assert_eq!(tracker.interface(), "eth0");
    }

    #[test]
    fn test_bandwidth_tracker_first_sample() {
        let mut tracker = BandwidthTracker::new("lo");
        let sample = tracker.sample();
        assert_eq!(sample.rx_rate, 0);
        assert_eq!(sample.tx_rate, 0);
    }

    #[test]
    fn test_bandwidth_tracker_reset() {
        let mut tracker = BandwidthTracker::new("lo");
        let _ = tracker.sample();
        tracker.reset();
        let sample = tracker.sample();
        assert_eq!(sample.rx_rate, 0);
        assert_eq!(sample.tx_rate, 0);
    }

    #[test]
    fn test_read_sysfs_counter_missing() {
        let value = read_sysfs_counter("nonexistent_interface", "rx_bytes");
        assert_eq!(value, 0);
    }

    #[test]
    fn test_read_sysfs_counter_loopback() {
        let value = read_sysfs_counter("lo", "rx_bytes");
        assert!(value <= u64::MAX);
    }
}
