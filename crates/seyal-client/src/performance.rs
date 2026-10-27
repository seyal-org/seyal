//! Shared helpers for deterministic performance-gate reporting.

/// Select a percentile using the nearest-rank definition from sorted samples.
pub fn nearest_rank_percentile(sorted: &[u64], percentile: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (usize::from(percentile) * sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::nearest_rank_percentile;

    #[test]
    fn percentile_uses_nearest_rank() {
        let samples: Vec<u64> = (1..=200).collect();
        assert_eq!(nearest_rank_percentile(&samples, 50), 100);
        assert_eq!(nearest_rank_percentile(&samples, 95), 190);
        assert_eq!(nearest_rank_percentile(&samples, 99), 198);
        assert_eq!(nearest_rank_percentile(&[7], 99), 7);
        assert_eq!(nearest_rank_percentile(&[], 99), 0);
    }
}
