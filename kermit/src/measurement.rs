use criterion::{
    measurement::{Measurement, ValueFormatter},
    Throughput,
};

pub struct BytesFormatter;

impl BytesFormatter {
    fn scale(typical: f64) -> (f64, &'static str) {
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
