//! Percentile summaries for bench reports.

use hdrhistogram::Histogram;
use serde_json::{Value, json};

/// p50, p95, p99 and max of one series, plus its sample count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    pub count: u64,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

impl Summary {
    /// Summarises a GPUI histogram recorded in nanoseconds, in milliseconds. GPUI's histograms
    /// keep three significant figures, so percentiles are exact to 0.1%. `None` when empty.
    pub fn from_nanos(histogram: &Histogram<u64>) -> Option<Self> {
        if histogram.is_empty() {
            return None;
        }
        let ms = |nanos: u64| nanos as f64 / 1_000_000.0;
        Some(Self {
            count: histogram.len(),
            mean: histogram.mean() / 1_000_000.0,
            p50: ms(histogram.value_at_quantile(0.50)),
            p95: ms(histogram.value_at_quantile(0.95)),
            p99: ms(histogram.value_at_quantile(0.99)),
            max: ms(histogram.max()),
        })
    }

    /// Summarises raw samples with nearest-rank percentiles. Sorts `samples` in place.
    /// `None` when empty or when a sample is not finite.
    pub fn from_samples(samples: &mut [f64]) -> Option<Self> {
        if samples.is_empty() || samples.iter().any(|s| !s.is_finite()) {
            return None;
        }
        samples.sort_by(f64::total_cmp);
        let n = samples.len();
        let rank = |q: f64| samples[((q * n as f64).ceil() as usize).clamp(1, n) - 1];
        Some(Self {
            count: n as u64,
            mean: samples.iter().sum::<f64>() / n as f64,
            p50: rank(0.50),
            p95: rank(0.95),
            p99: rank(0.99),
            max: samples[n - 1],
        })
    }

    /// The percentiles as a JSON object. `count` is left out: it is not a cost, and every key
    /// under `metrics` is compared as lower-is-better.
    pub fn to_json(&self) -> Value {
        json!({
            "mean": round(self.mean),
            "p50": round(self.p50),
            "p95": round(self.p95),
            "p99": round(self.p99),
            "max": round(self.max),
        })
    }
}

/// Rounds to 4 decimal places so reports stay readable (0.1 µs when the unit is ms).
pub fn round(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

/// Merges histograms into one. `None` when there is nothing to merge.
pub fn merged<'a>(
    histograms: impl IntoIterator<Item = &'a Histogram<u64>>,
) -> Option<Histogram<u64>> {
    let mut iter = histograms.into_iter();
    let mut merged = iter.next()?.clone();
    for histogram in iter {
        merged.add(histogram).ok()?;
    }
    Some(merged)
}

/// `later - earlier` for histograms recorded by the same GPUI window profiler.
pub fn delta(later: &Histogram<u64>, earlier: &Histogram<u64>) -> Option<Histogram<u64>> {
    let mut delta = later.clone();
    delta.subtract(earlier).ok()?;
    Some(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentiles() {
        let mut samples: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        let s = Summary::from_samples(&mut samples).unwrap();
        assert_eq!(
            (s.count, s.p50, s.p95, s.p99, s.max),
            (100, 50.0, 95.0, 99.0, 100.0)
        );
        assert_eq!(s.mean, 50.5);

        let s = Summary::from_samples(&mut [3.0]).unwrap();
        assert_eq!((s.p50, s.p99, s.max), (3.0, 3.0, 3.0));

        assert!(Summary::from_samples(&mut []).is_none());
        assert!(Summary::from_samples(&mut [1.0, f64::NAN]).is_none());
    }

    #[test]
    fn histogram_summary_in_milliseconds() {
        let mut h = Histogram::<u64>::new(3).unwrap();
        for ms in 1..=100_u64 {
            h.record(ms * 1_000_000).unwrap();
        }
        let s = Summary::from_nanos(&h).unwrap();
        assert_eq!(s.count, 100);
        assert!((s.p50 - 50.0).abs() < 0.1, "{s:?}");
        assert!((s.p99 - 99.0).abs() < 0.1, "{s:?}");
        assert!((s.max - 100.0).abs() < 0.1, "{s:?}");
        assert!(Summary::from_nanos(&Histogram::<u64>::new(3).unwrap()).is_none());
    }

    #[test]
    fn delta_and_merge() {
        let mut early = Histogram::<u64>::new(3).unwrap();
        early.record(10).unwrap();
        let mut late = early.clone();
        late.record(20).unwrap();
        late.record(30).unwrap();
        let d = delta(&late, &early).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d.min(), 20);
        let m = merged([&early, &d]).unwrap();
        assert_eq!(m.len(), 3);
        assert!(merged([]).is_none());
    }
}
