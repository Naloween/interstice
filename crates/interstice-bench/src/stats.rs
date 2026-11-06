//! Latency samples and per-second throughput.

use std::time::Duration;

/// Latencies in microseconds.
#[derive(Default, Clone)]
pub struct Latencies(Vec<u32>);

impl Latencies {
    pub fn record(&mut self, d: Duration) {
        self.0.push(d.as_micros().min(u32::MAX as u128) as u32);
    }

    pub fn merge(&mut self, other: Latencies) {
        self.0.extend(other.0);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The `q` quantile (0..=1), in microseconds.
    pub fn quantile(&mut self, q: f64) -> u32 {
        if self.0.is_empty() {
            return 0;
        }
        self.0.sort_unstable();
        let i = ((self.0.len() - 1) as f64 * q).round() as usize;
        self.0[i]
    }
}

/// Mean and standard deviation.
pub fn mean_std(xs: &[f64]) -> (f64, f64) {
    if xs.is_empty() {
        return (0.0, 0.0);
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64;
    (mean, var.sqrt())
}
