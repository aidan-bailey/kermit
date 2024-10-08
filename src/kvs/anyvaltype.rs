use std::{any::TypeId, hash::Hash};

#[derive(Debug, Clone, PartialEq)]
pub enum AnyValType {
    Str(String),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

// Defaults

impl AnyValType {
    pub fn default_str() -> Self { AnyValType::Str(String::new()) }

    pub fn default_i32() -> Self { AnyValType::I32(0) }

    pub fn default_i64() -> Self { AnyValType::I64(0) }

    pub fn default_f32() -> Self { AnyValType::F32(0.0) }

    pub fn default_f64() -> Self { AnyValType::F64(0.0) }
}

// Parsing

impl AnyValType {
    pub fn parse_str(v: &str) -> Self { AnyValType::Str(v.to_string()) }

    pub fn parse_i32(v: &str) -> Self { AnyValType::I32(v.parse().unwrap()) }

    pub fn parse_i64(v: &str) -> Self { AnyValType::I64(v.parse().unwrap()) }

    pub fn parse_f32(v: &str) -> Self { AnyValType::F32(v.parse().unwrap()) }

    pub fn parse_f64(v: &str) -> Self { AnyValType::F64(v.parse().unwrap()) }

    pub fn parse_into_self(&self, v: &str) -> Self {
        match self {
            | AnyValType::Str(_) => AnyValType::parse_str(v),
            | AnyValType::I32(_) => AnyValType::parse_i32(v),
            | AnyValType::I64(_) => AnyValType::parse_i64(v),
            | AnyValType::F32(_) => AnyValType::parse_f32(v),
            | AnyValType::F64(_) => AnyValType::parse_f64(v),
        }
    }
}

// To Methods

impl AnyValType {
    pub fn to_string(&self) -> Option<&String> {
        match self {
            | AnyValType::Str(v) => Some(v),
            | _ => None,
        }
    }

    pub fn to_i32(&self) -> Option<i32> {
        match self {
            | AnyValType::I32(v) => Some(*v),
            | _ => None,
        }
    }

    pub fn to_i64(&self) -> Option<i64> {
        match self {
            | AnyValType::I64(v) => Some(*v),
            | _ => None,
        }
    }

    pub fn to_f32(&self) -> Option<f32> {
        match self {
            | AnyValType::F32(v) => Some(*v),
            | _ => None,
        }
    }

    pub fn to_f64(&self) -> Option<f64> {
        match self {
            | AnyValType::F64(v) => Some(*v),
            | _ => None,
        }
    }
}

// From Methods

impl From<&str> for AnyValType {
    fn from(v: &str) -> Self { AnyValType::Str(v.to_string()) }
}

impl From<String> for AnyValType {
    fn from(v: String) -> Self { AnyValType::Str(v) }
}

impl From<i32> for AnyValType {
    fn from(v: i32) -> Self { AnyValType::I32(v) }
}

impl From<i64> for AnyValType {
    fn from(v: i64) -> Self { AnyValType::I64(v) }
}

impl From<f32> for AnyValType {
    fn from(v: f32) -> Self { AnyValType::F32(v) }
}

impl From<f64> for AnyValType {
    fn from(v: f64) -> Self { AnyValType::F64(v) }
}

impl Hash for AnyValType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            | AnyValType::Str(v) => (TypeId::of::<String>(), v).hash(state),
            | AnyValType::I32(v) => (TypeId::of::<i32>(), v).hash(state),
            | AnyValType::I64(v) => (TypeId::of::<i64>(), v).hash(state),
            | AnyValType::F32(v) => (TypeId::of::<f32>(), v.to_bits()).hash(state),
            | AnyValType::F64(v) => (TypeId::of::<f64>(), v.to_bits()).hash(state),
        }
    }
}
