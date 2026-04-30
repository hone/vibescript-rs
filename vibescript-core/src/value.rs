use crate::ast::{Stmt, TypeExpr};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq)]
pub struct EnumMember {
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Symbol(String),
    Time(DateTime<Utc>),
    Duration(i64), // Seconds
    Money {
        cents: i64,
        currency: String,
    },
    EnumVariant {
        enum_name: String,
        variant_name: String,
    },
    Array(Arc<RwLock<Vec<Value>>>),
    Hash(Arc<RwLock<HashMap<String, Value>>>),
    Object(Arc<RwLock<HashMap<String, Value>>>),
    Block {
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Function(Arc<FunctionDef>),
    Class(Arc<ClassDef>),
    Instance(Arc<RwLock<InstanceData>>),
    Builtin(String),
    Namespace(String),
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Value::Nil => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Int(i) => serializer.serialize_i64(*i),
            Value::Float(f) => serializer.serialize_f64(*f),
            Value::String(s) => serializer.serialize_str(s),
            Value::Symbol(s) => serializer.serialize_str(&format!(":{}", s)),
            Value::Time(t) => serializer.serialize_str(&t.to_rfc3339()),
            Value::Duration(s) => serializer.serialize_str(&format!("{}s", s)),
            Value::Money { cents, currency } => serializer.serialize_str(&format!(
                "{}.{:02} {}",
                cents / 100,
                cents % 100,
                currency
            )),
            Value::EnumVariant {
                enum_name,
                variant_name,
            } => serializer.serialize_str(&format!("{}.{}", enum_name, variant_name)),
            Value::Array(a) => {
                let arr = a.read().unwrap();
                serializer.collect_seq(arr.iter())
            }
            Value::Hash(h) => {
                let hash = h.read().unwrap();
                serializer.collect_map(hash.iter())
            }
            Value::Object(o) => {
                let obj = o.read().unwrap();
                serializer.collect_map(obj.iter())
            }
            Value::Block { .. } => serializer.serialize_str("<block>"),
            Value::Function(_) => serializer.serialize_str("<function>"),
            Value::Class(c) => serializer.serialize_str(&format!("<class {}>", c.name)),
            Value::Instance(i) => {
                let inst = i.read().unwrap();
                serializer.serialize_str(&format!("<instance of {}>", inst.class.name))
            }
            Value::Builtin(name) => serializer.serialize_str(&format!("<builtin {}>", name)),
            Value::Namespace(name) => serializer.serialize_str(&format!("<namespace {}>", name)),
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Err(serde::de::Error::custom(
            "Direct deserialization of Value is not supported. Use eval::Engine::json_to_vibe.",
        ))
    }
}

#[derive(Debug)]
pub struct ClassDef {
    pub name: String,
    pub methods: RwLock<HashMap<String, FunctionDef>>,
    pub class_methods: RwLock<HashMap<String, FunctionDef>>,
    pub class_vars: RwLock<HashMap<String, Value>>,
}

impl PartialEq for ClassDef {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Vec<Stmt>,
    pub is_private: bool,
    pub is_exported: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub is_ivar: bool,
    pub param_type: Option<TypeExpr>,
}

#[derive(Debug)]
pub struct InstanceData {
    pub class: Arc<ClassDef>,
    pub ivars: HashMap<String, Value>,
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(l), Value::Bool(r)) => l == r,
            (Value::Int(l), Value::Int(r)) => l == r,
            (Value::Float(l), Value::Float(r)) => l == r,
            (Value::String(l), Value::String(r)) => l == r,
            (Value::Symbol(l), Value::Symbol(r)) => l == r,
            (Value::Time(l), Value::Time(r)) => l == r,
            (Value::Duration(l), Value::Duration(r)) => l == r,
            (
                Value::Money {
                    cents: cl,
                    currency: curl,
                },
                Value::Money {
                    cents: cr,
                    currency: curr,
                },
            ) => cl == cr && curl == curr,
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
            (Value::Array(l), Value::Array(r)) => {
                if Arc::ptr_eq(l, r) {
                    true
                } else {
                    let lv = l.read().unwrap();
                    let rv = r.read().unwrap();
                    if lv.len() != rv.len() {
                        return false;
                    }
                    lv.iter().zip(rv.iter()).all(|(a, b)| a == b)
                }
            }
            (Value::Hash(l), Value::Hash(r)) => {
                if Arc::ptr_eq(l, r) {
                    true
                } else {
                    let lv = l.read().unwrap();
                    let rv = r.read().unwrap();
                    if lv.len() != rv.len() {
                        return false;
                    }
                    lv.iter().all(|(k, v)| rv.get(k) == Some(v))
                }
            }
            (Value::Object(l), Value::Object(r)) => {
                if Arc::ptr_eq(l, r) {
                    true
                } else {
                    let lv = l.read().unwrap();
                    let rv = r.read().unwrap();
                    if lv.len() != rv.len() {
                        return false;
                    }
                    lv.iter().all(|(k, v)| rv.get(k) == Some(v))
                }
            }
            (Value::Class(l), Value::Class(r)) => Arc::ptr_eq(l, r),
            (Value::Instance(l), Value::Instance(r)) => Arc::ptr_eq(l, r),
            (Value::Builtin(l), Value::Builtin(r)) => l == r,
            (Value::Namespace(l), Value::Namespace(r)) => l == r,
            _ => false,
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Value::Int(l), Value::Int(r)) => l.partial_cmp(r),
            (Value::Float(l), Value::Float(r)) => l.partial_cmp(r),
            (Value::Int(l), Value::Float(r)) => (*l as f64).partial_cmp(r),
            (Value::Float(l), Value::Int(r)) => l.partial_cmp(&(*r as f64)),
            (Value::String(l), Value::String(r)) => l.partial_cmp(r),
            (Value::Symbol(l), Value::Symbol(r)) => l.partial_cmp(r),
            (Value::Bool(l), Value::Bool(r)) => l.partial_cmp(r),
            (Value::Time(l), Value::Time(r)) => l.partial_cmp(r),
            (Value::Duration(l), Value::Duration(r)) => l.partial_cmp(r),
            (Value::Money { cents: l, .. }, Value::Money { cents: r, .. }) => l.partial_cmp(r),
            _ => None,
        }
    }
}

impl Value {
    pub fn new_array(vec: Vec<Value>) -> Self {
        Value::Array(Arc::new(RwLock::new(vec)))
    }

    pub fn new_hash(hash: HashMap<String, Value>) -> Self {
        Value::Hash(Arc::new(RwLock::new(hash)))
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            Value::String(s) => s.parse::<f64>().ok(),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_hash(&self) -> Option<Arc<RwLock<HashMap<String, Value>>>> {
        match self {
            Value::Hash(h) => Some(h.clone()),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<Arc<RwLock<Vec<Value>>>> {
        match self {
            Value::Array(a) => Some(a.clone()),
            _ => None,
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Bool(b) => *b,
            _ => true,
        }
    }

    pub fn kind_name(&self) -> &str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Symbol(_) => "symbol",
            Value::Time(_) => "time",
            Value::Duration(_) => "duration",
            Value::Money { .. } => "money",
            Value::EnumVariant { .. } => "enum",
            Value::Array(_) => "array",
            Value::Hash(_) => "hash",
            Value::Object(_) => "object",
            Value::Block { .. } => "block",
            Value::Function(_) => "function",
            Value::Class(_) => "class",
            Value::Instance(_) => "instance",
            Value::Builtin(_) => "builtin",
            Value::Namespace(_) => "namespace",
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Value::Nil => "nil".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => s.clone(),
            Value::Symbol(s) => format!(":{}", s),
            Value::Time(t) => t.to_rfc3339(),
            Value::Duration(s) => format!("{}s", s),
            Value::Money { cents, currency } => {
                format!("{}.{:02} {}", cents / 100, cents % 100, currency)
            }
            Value::EnumVariant {
                enum_name,
                variant_name,
            } => format!("{}.{}", enum_name, variant_name),
            Value::Array(a) => {
                let arr = a.read().unwrap();
                let parts: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Hash(h) => {
                let hash = h.read().unwrap();
                let mut parts: Vec<String> = hash
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_string()))
                    .collect();
                parts.sort();
                format!("{{ {} }}", parts.join(", "))
            }
            Value::Object(o) => {
                let obj = o.read().unwrap();
                let mut parts: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_string()))
                    .collect();
                parts.sort();
                format!("{{ {} }}", parts.join(", "))
            }
            Value::Block { .. } => "<block>".to_string(),
            Value::Function(_) => "function".to_string(),
            Value::Class(c) => format!("<class {}>", c.name),
            Value::Instance(i) => format!("<instance of {}>", i.read().unwrap().class.name),
            Value::Builtin(name) => format!("<builtin {}>", name),
            Value::Namespace(name) => format!("<namespace {}>", name),
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Nil
    }
}
