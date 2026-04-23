use crate::ast::Stmt;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Time(DateTime<Utc>),
    EnumVariant {
        enum_name: String,
        variant_name: String,
    },
    Array(Vec<Value>),
    Hash(HashMap<String, Value>),
    #[serde(skip)]
    Block {
        params: Vec<String>,
        body: Vec<Stmt>,
    },
}

impl Value {
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Value::Nil => "nil".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => s.clone(),
            Value::Time(t) => t.to_rfc3339(),
            Value::EnumVariant {
                enum_name,
                variant_name,
            } => format!("{}.{}", enum_name, variant_name),
            Value::Array(a) => format!("{:?}", a),
            Value::Hash(h) => format!("{:?}", h),
            Value::Block { .. } => "block".to_string(),
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Nil
    }
}
