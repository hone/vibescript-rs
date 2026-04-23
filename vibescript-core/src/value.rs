use crate::ast::Stmt;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq)]
pub struct EnumMember {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(skip)]
    Class(Arc<ClassDef>),
    #[serde(skip)]
    Instance(Arc<RwLock<InstanceData>>),
}

#[derive(Debug)]
pub struct ClassDef {
    pub name: String,
    pub methods: HashMap<String, FunctionDef>,
    pub class_methods: HashMap<String, FunctionDef>,
    pub class_vars: RwLock<HashMap<String, Value>>,
}

// We implement PartialEq manually for ClassDef since we use Arc
impl PartialEq for ClassDef {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub is_private: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub is_ivar: bool,
}

#[derive(Debug)]
pub struct InstanceData {
    pub class: Arc<ClassDef>,
    pub ivars: HashMap<String, Value>,
}

// Manual PartialEq for Value to handle Arc/RwLock
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(l), Value::Bool(r)) => l == r,
            (Value::Int(l), Value::Int(r)) => l == r,
            (Value::Float(l), Value::Float(r)) => l == r,
            (Value::String(l), Value::String(r)) => l == r,
            (Value::Time(l), Value::Time(r)) => l == r,
            (
                Value::EnumVariant {
                    enum_name: el,
                    variant_name: vl,
                },
                Value::EnumVariant {
                    enum_name: er,
                    variant_name: vr,
                },
            ) => el == er && vl == vr,
            (Value::Array(l), Value::Array(r)) => l == r,
            (Value::Hash(l), Value::Hash(r)) => l == r,
            (
                Value::Block {
                    params: pl,
                    body: bl,
                },
                Value::Block {
                    params: pr,
                    body: br,
                },
            ) => pl == pr && bl == br,
            (Value::Class(l), Value::Class(r)) => Arc::ptr_eq(l, r),
            (Value::Instance(l), Value::Instance(r)) => Arc::ptr_eq(l, r),
            _ => false,
        }
    }
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
            Value::Class(c) => format!("<class {}>", c.name),
            Value::Instance(i) => format!("<instance of {}>", i.read().unwrap().class.name),
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Nil
    }
}
