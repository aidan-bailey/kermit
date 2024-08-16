use std::hash::Hash;

#[derive(Debug, Clone, PartialEq)]
pub enum AnyValType {
    Str(String),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
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
            | AnyValType::Str(v) => v.hash(state),
            | AnyValType::I32(v) => v.hash(state),
            | AnyValType::I64(v) => v.hash(state),
            | AnyValType::F32(v) => v.to_bits().hash(state),
            | AnyValType::F64(v) => v.to_bits().hash(state),
        }
    }
}
