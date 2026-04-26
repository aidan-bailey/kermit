use criterion::{
    measurement::{Measurement, ValueFormatter},
    Throughput,
};

pub struct BytesFormatter;

impl BytesFormatter {
    /// Pick a binary-prefixed unit (`B`, `KiB`, `MiB`, `GiB`) for `typical`
    /// and return the multiplicative scale factor plus the unit string.
    /// Use [`format_bytes`] for one-shot formatting of a single byte count.
    pub fn scale(typical: f64) -> (f64, &'static str) {
        if typical < 1024.0 {
            (1.0, "B")
        } else if typical < 1024.0 * 1024.0 {
            (1.0 / 1024.0, "KiB")
        } else if typical < 1024.0 * 1024.0 * 1024.0 {
            (1.0 / (1024.0 * 1024.0), "MiB")
        } else {
            (1.0 / (1024.0 * 1024.0 * 1024.0), "GiB")
        }
    }
}

/// Render `n` bytes as a human-readable string using [`BytesFormatter::scale`].
/// Whole-byte values render without a decimal (e.g. `"768 B"`); larger units
/// render with two decimal places (e.g. `"1.50 MiB"`).
pub fn format_bytes(n: u64) -> String {
    let (factor, unit) = BytesFormatter::scale(n as f64);
    if unit == "B" {
        format!("{n} {unit}")
    } else {
        format!("{:.2} {unit}", n as f64 * factor)
    }
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

    fn scale_for_machines(&self, _values: &mut [f64]) -> &'static str {
        "B"
    }
}

pub struct SpaceMeasurement;

impl Measurement for SpaceMeasurement {
    type Intermediate = ();
    type Value = usize;

    fn start(&self) -> Self::Intermediate {}

    fn end(&self, _i: Self::Intermediate) -> Self::Value {
        0
    }

    fn add(&self, v1: &Self::Value, v2: &Self::Value) -> Self::Value {
        v1 + v2
    }

    fn zero(&self) -> Self::Value {
        0
    }

    fn to_f64(&self, value: &Self::Value) -> f64 {
        *value as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        &BytesFormatter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_picks_unit_per_threshold() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(768), "768 B");
        assert_eq!(format_bytes(1024), "1.00 KiB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.00 MiB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.00 GiB");
    }

    #[test]
    fn scale_factor_matches_unit() {
        let (factor, unit) = BytesFormatter::scale(2048.0);
        assert_eq!(unit, "KiB");
        assert!((2048.0 * factor - 2.0).abs() < f64::EPSILON);
    }
}
