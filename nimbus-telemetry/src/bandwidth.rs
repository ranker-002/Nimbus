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
            // The first reading is an absolute counter, not a delta: there is
            // nothing to measure it against yet.
            self.initialized = true;
            (0, 0)
        } else {
            (
                compute_rate(current_rx, self.last_rx, elapsed),
                compute_rate(current_tx, self.last_tx, elapsed),
            )
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

/// Bytes per second between two counter readings.
///
/// Returns 0 when the counter went backwards — that means it was reset (the
/// interface came back up, or the hotspot moved) and the difference would be
/// meaningless.
fn compute_rate(current: u64, last: u64, elapsed_secs: f64) -> u64 {
    if elapsed_secs <= 0.0 || current < last {
        return 0;
    }
    ((current - last) as f64 / elapsed_secs) as u64
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
    fn test_read_sysfs_counter_loopback_matches_the_file() {
        let path = "/sys/class/net/lo/statistics/rx_bytes";
        let Ok(raw) = std::fs::read_to_string(path) else {
            return; // No sysfs (container, non-Linux CI): nothing to compare.
        };
        let expected: u64 = raw.trim().parse().expect("counter should be a number");
        assert_eq!(read_sysfs_counter("lo", "rx_bytes"), expected);
    }

    #[test]
    fn test_compute_rate_divides_delta_by_elapsed() {
        assert_eq!(compute_rate(3000, 1000, 2.0), 1000);
        assert_eq!(compute_rate(1500, 1000, 0.5), 1000);
    }

    #[test]
    fn test_compute_rate_returns_zero_when_counter_resets() {
        // Counters restart at 0 when an interface is torn down and recreated.
        assert_eq!(compute_rate(10, 5_000_000, 1.0), 0);
    }

    #[test]
    fn test_compute_rate_returns_zero_without_elapsed_time() {
        assert_eq!(compute_rate(3000, 1000, 0.0), 0);
        assert_eq!(compute_rate(3000, 1000, -1.0), 0);
    }

    #[test]
    fn test_compute_rate_is_zero_when_idle() {
        assert_eq!(compute_rate(1000, 1000, 5.0), 0);
    }
}
