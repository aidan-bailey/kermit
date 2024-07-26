#[derive(PartialOrd, PartialEq, Clone)]
pub enum VariableType {
    Int(i32),
    Float(f32),
    String(String),
}
