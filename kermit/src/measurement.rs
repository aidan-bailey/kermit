use criterion::{
    measurement::{Measurement, ValueFormatter},
    Throughput,
};

pub struct BytesFormatter;

impl BytesFormatter {
    /// A binary-prefixed unit for `typical`, with its scale factor
    /// ([`crate::bench_report::byte_unit`]).
    pub fn scale(typical: f64) -> (f64, &'static str) { crate::bench_report::byte_unit(typical) }
}

impl ValueFormatter for BytesFormatter {
    fn scale_values(&self, typical_value: f64, values: &mut [f64]) -> &'static str {
        let (factor, unit) = Self::scale(typical_value);
        for val in values {
            *val *= factor;
        }
        unit
    }

    fn scale_throughputs(
        &self, _typical_value: f64, throughput: &Throughput, values: &mut [f64],
    ) -> &'static str {
        match *throughput {
            | Throughput::Elements(elems) => {
                for val in values {
                    *val /= elems as f64;
                }
                "B/elem"
            },
            | _ => "B",
        }
    }

    fn scale_for_machines(&self, _values: &mut [f64]) -> &'static str { "B" }
}

/// Custom Criterion `Measurement` that reports `heap_size_bytes()` per
/// iteration instead of wall-clock time.
///
/// `start`/`end` are no-ops — the actual byte count is supplied by an
/// `iter_custom` closure in `main.rs::run_benchmark` that calls
/// `heap_size_bytes()` on the pre-built relation. This split exists because
/// Criterion's `Measurement` trait separates "wall instrumentation" from "the
/// thing being measured", and only the latter is meaningful for space.
///
/// **Calibration trap:** Criterion's `iter_custom` calibration uses
/// wall-clock during warmup even for non-time `Measurement`s. A near-instant
/// closure makes the `iters` count ramp toward `u64::MAX` and `iters * bytes`
/// arithmetic saturate `usize`. The closure in `run_benchmark` keeps an O(N)
/// call inside the loop (wrapped in `std::hint::black_box`) for this reason
/// — see CLAUDE.md "Space benchmarks" gotcha.
pub struct SpaceMeasurement;

impl Measurement for SpaceMeasurement {
    type Intermediate = ();
    type Value = usize;

    fn start(&self) -> Self::Intermediate {}

    fn end(&self, _i: Self::Intermediate) -> Self::Value { 0 }

    fn add(&self, v1: &Self::Value, v2: &Self::Value) -> Self::Value { v1 + v2 }

    fn zero(&self) -> Self::Value { 0 }

    fn to_f64(&self, value: &Self::Value) -> f64 { *value as f64 }

    fn formatter(&self) -> &dyn ValueFormatter { &BytesFormatter }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_factor_matches_unit() {
        let (factor, unit) = BytesFormatter::scale(2048.0);
        assert_eq!(unit, "KiB");
        assert!((2048.0 * factor - 2.0).abs() < f64::EPSILON);
    }
}
