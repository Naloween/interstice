//! Zipf-distributed account picks for the transfer load: `alpha = 0` is
//! uniform; SpacetimeDB's keynote uses 0 and 1.5 ("~80% contention").

/// A sampler over `0..n` with `P(k) ∝ 1 / (k + 1)^alpha`.
pub struct Zipf {
    cdf: Vec<f64>,
}

impl Zipf {
    pub fn new(n: u32, alpha: f64) -> Self {
        let mut cdf = Vec::with_capacity(n as usize);
        let mut sum = 0.0;
        for k in 0..n {
            sum += 1.0 / ((k + 1) as f64).powf(alpha);
            cdf.push(sum);
        }
        for p in &mut cdf {
            *p /= sum;
        }
        Self { cdf }
    }

    /// Map a uniform `u` in `[0, 1)` to an index.
    pub fn sample(&self, u: f64) -> u32 {
        self.cdf.partition_point(|&p| p < u).min(self.cdf.len() - 1) as u32
    }
}

/// A small fast generator for the load loop (xorshift64*).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_and_skewed() {
        let n = 1000;
        let mut rng = Rng::new(7);
        let flat = Zipf::new(n, 0.0);
        let skew = Zipf::new(n, 1.5);
        let (mut flat_top, mut skew_top) = (0, 0);
        for _ in 0..100_000 {
            if flat.sample(rng.next_f64()) < 10 {
                flat_top += 1;
            }
            if skew.sample(rng.next_f64()) < 10 {
                skew_top += 1;
            }
        }
        // 1% of keys: ~1% of draws when uniform, most of them when skewed.
        assert!((500..1500).contains(&flat_top), "{flat_top}");
        assert!(skew_top > 70_000, "{skew_top}");
    }
}
