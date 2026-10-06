#[derive(Clone, Debug, serde::Serialize)]
pub struct Dist {
    pub n: u32,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub max_ns: u64,
}

impl Dist {
    pub fn from_samples(samples: &mut [u64]) -> Self {
        if samples.is_empty() {
            return Self {
                n: 0,
                p50_ns: 0,
                p95_ns: 0,
                p99_ns: 0,
                max_ns: 0,
            };
        }
        samples.sort_unstable();
        Self {
            n: samples.len() as u32,
            p50_ns: rank(samples, 50.0),
            p95_ns: rank(samples, 95.0),
            p99_ns: rank(samples, 99.0),
            max_ns: samples[samples.len() - 1],
        }
    }
}

/// Nearest-rank percentile: index = ceil(p/100 * n) - 1.
fn rank(sorted: &[u64], percentile: f64) -> u64 {
    let n = sorted.len();
    let idx = ((percentile / 100.0) * n as f64).ceil() as usize;
    sorted[idx.saturating_sub(1).min(n - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_matches_sorted_ends() {
        let mut samples = [10, 30, 20, 40];
        let dist = Dist::from_samples(&mut samples);
        assert_eq!(dist.n, 4);
        assert_eq!(dist.max_ns, 40);
        assert_eq!(dist.p50_ns, 20);
        assert_eq!(dist.p99_ns, 40);
    }
}
