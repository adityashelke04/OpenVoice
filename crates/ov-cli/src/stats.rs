//! The one statistic both reports need.

/// Nearest-rank percentile: always a value that was actually measured.
///
/// Interpolated percentiles invent latencies nobody experienced. With the
/// few dozen sessions a person dictates in a day, nearest rank is the honest
/// choice.
#[must_use]
pub fn percentile(values: &[u64], p: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentiles_are_values_that_actually_occurred() {
        let v: Vec<u64> = (1..=10).collect();
        assert_eq!(percentile(&v, 50.0), Some(5));
        assert_eq!(percentile(&v, 90.0), Some(9));
        assert_eq!(percentile(&v, 100.0), Some(10));
    }

    #[test]
    fn order_of_the_input_does_not_matter() {
        assert_eq!(percentile(&[9, 1, 5], 50.0), Some(5));
    }

    #[test]
    fn nothing_measured_has_no_percentile() {
        assert_eq!(percentile(&[], 50.0), None);
    }
}
