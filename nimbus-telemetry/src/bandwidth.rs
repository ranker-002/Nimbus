use std::time::Instant;

use nimbus_core::types::BandwidthSample;

pub struct BandwidthTracker {
    interface: String,
    last_rx: u64,
    last_tx: u64,
    last_instant: Instant,
}

impl BandwidthTracker {
    pub fn new(interface: &str) -> Self {
        Self {
            interface: interface.to_string(),
            last_rx: 0,
            last_tx: 0,
            last_instant: Instant::now(),
        }
    }

    pub fn sample(&mut self) -> BandwidthSample {
        let current_rx = read_sysfs_counter(&self.interface, "rx_bytes");
        let current_tx = read_sysfs_counter(&self.interface, "tx_bytes");
        let now = Instant::now();

        let elapsed = now.duration_since(self.last_instant).as_secs_f64();
        let rx_rate = if elapsed > 0.0 {
            ((current_rx.saturating_sub(self.last_rx)) as f64 / elapsed) as u64
        } else {
            0
        };
        let tx_rate = if elapsed > 0.0 {
            ((current_tx.saturating_sub(self.last_tx)) as f64 / elapsed) as u64
        } else {
            0
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
