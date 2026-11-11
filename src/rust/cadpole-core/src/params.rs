//! Script parameters: declared by `param(...)`, overridable from the document.

use cadpole_proto::v1 as pb;

#[derive(Clone, Debug, PartialEq)]
pub enum ParamValue {
    Number(f64),
    Bool(bool),
    Text(String),
    Vector(crate::Vec3),
}

impl ParamValue {
    pub fn to_proto(&self) -> pb::ParamValue {
        use pb::param_value::Value;
        let value = match self {
            ParamValue::Number(n) => Value::Number(*n),
            ParamValue::Bool(b) => Value::Boolean(*b),
            ParamValue::Text(s) => Value::Text(s.clone()),
            ParamValue::Vector(v) => Value::Vector(v.to_proto()),
        };
        pb::ParamValue { value: Some(value) }
    }

    pub fn from_proto(p: &pb::ParamValue) -> Option<ParamValue> {
        use pb::param_value::Value;
        Some(match p.value.as_ref()? {
            Value::Number(n) => ParamValue::Number(*n),
            Value::Boolean(b) => ParamValue::Bool(*b),
            Value::Text(s) => ParamValue::Text(s.clone()),
            Value::Vector(v) => ParamValue::Vector(crate::Vec3::new(v.x, v.y, v.z)),
        })
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            ParamValue::Number(_) => "number",
            ParamValue::Bool(_) => "bool",
            ParamValue::Text(_) => "string",
            ParamValue::Vector(_) => "vector",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ParamDecl {
    pub name: String,
    pub default: ParamValue,
    pub description: String,
    pub unit: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    /// Allowed values for text params (an enum-like choice), if any.
    pub choices: Vec<String>,
    pub span: Option<crate::Span>,
}
