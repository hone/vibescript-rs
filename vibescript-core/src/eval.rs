use crate::ast::*;
use crate::value::{ClassDef, FunctionDef, InstanceData, Param, Value};
use chrono::{DateTime, TimeZone, Utc};
use rand::{Rng, distributions::Alphanumeric};
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct Engine {
    globals: HashMap<String, Value>,
    stack: Vec<HashMap<String, Value>>,
    pub functions: HashMap<String, FunctionDef>,
    pub classes: HashMap<String, Arc<ClassDef>>,
    class_vars: HashMap<String, Value>,
    recursion_depth: usize,
}

const MAX_RECURSION_DEPTH: usize = 500;

#[derive(Debug)]
pub enum EvalError {
    Message(String),
}

impl From<String> for EvalError {
    fn from(msg: String) -> Self {
        EvalError::Message(msg)
    }
}

#[derive(Debug)]
pub enum ControlFlow {
    Continue(Value),
    Return(Value),
    Break(Value),
    Next(Value),
}

impl ControlFlow {
    pub fn value(&self) -> Value {
        match self {
            ControlFlow::Continue(v) => v.clone(),
            ControlFlow::Return(v) => v.clone(),
            ControlFlow::Break(v) => v.clone(),
            ControlFlow::Next(v) => v.clone(),
        }
    }

    pub fn is_continue(&self) -> bool {
        matches!(self, ControlFlow::Continue(_))
    }
}

impl Engine {
    pub fn new() -> Self {
        let mut globals = HashMap::new();
        globals.insert("JSON".to_string(), Value::Namespace("JSON".to_string()));
        globals.insert("Time".to_string(), Value::Namespace("Time".to_string()));
        globals.insert("Regex".to_string(), Value::Namespace("Regex".to_string()));

        Self {
            globals,
            stack: vec![HashMap::new()],
            functions: HashMap::new(),
            classes: HashMap::new(),
            class_vars: HashMap::new(),
            recursion_depth: 0,
        }
    }

    pub fn eval_stmt(&mut self, stmt: &Stmt) -> Result<ControlFlow, EvalError> {
        match stmt {
            Stmt::Expression(expr) => self.eval_expr(expr).map(ControlFlow::Continue),
            Stmt::Assignment { name, value } => {
                let val = self.eval_expr(value)?;
                self.set_var(name, val.clone());
                Ok(ControlFlow::Continue(val))
            }
            Stmt::IvarAssignment { name, value } => {
                let val = self.eval_expr(value)?;
                self.set_ivar(name, val.clone())?;
                Ok(ControlFlow::Continue(val))
            }
            Stmt::CvarAssignment { name, value } => {
                let val = self.eval_expr(value)?;
                self.set_cvar(name, val.clone())?;
                Ok(ControlFlow::Continue(val))
            }
            Stmt::If {
                condition,
                then_branch,
                elsif_branches,
                else_branch,
            } => {
                let cond = self.eval_expr(condition)?;
                if self.is_truthy(&cond) {
                    self.eval_block(then_branch)
                } else {
                    for (elsif_cond, elsif_body) in elsif_branches {
                        let cond = self.eval_expr(elsif_cond)?;
                        if self.is_truthy(&cond) {
                            return self.eval_block(elsif_body);
                        }
                    }
                    if let Some(else_body) = else_branch {
                        self.eval_block(else_body)
                    } else {
                        Ok(ControlFlow::Continue(Value::Nil))
                    }
                }
            }
            Stmt::While { condition, body } => {
                let mut last_val = Value::Nil;
                loop {
                    let cond = self.eval_expr(condition)?;
                    if !self.is_truthy(&cond) {
                        break;
                    }
                    match self.eval_block(body)? {
                        ControlFlow::Break(v) => return Ok(ControlFlow::Continue(v)),
                        ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                        cf => last_val = cf.value(),
                    }
                }
                Ok(ControlFlow::Continue(last_val))
            }
            Stmt::Until { condition, body } => {
                let mut last_val = Value::Nil;
                loop {
                    let cond = self.eval_expr(condition)?;
                    if self.is_truthy(&cond) {
                        break;
                    }
                    match self.eval_block(body)? {
                        ControlFlow::Break(v) => return Ok(ControlFlow::Continue(v)),
                        ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                        cf => last_val = cf.value(),
                    }
                }
                Ok(ControlFlow::Continue(last_val))
            }
            Stmt::For {
                var,
                iterable,
                body,
            } => {
                let iter_val = self.eval_expr(iterable)?;
                let mut last_val = Value::Nil;
                if let Value::Array(a) = iter_val {
                    let arr = a.read().unwrap().clone();
                    for val in arr {
                        self.set_var(var, val);
                        match self.eval_block(body)? {
                            ControlFlow::Break(v) => {
                                last_val = v;
                                break;
                            }
                            ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                            cf => last_val = cf.value(),
                        }
                    }
                }
                Ok(ControlFlow::Continue(last_val))
            }
            Stmt::Function(f) => {
                let def = FunctionDef {
                    params: f.params.clone(),
                    body: f.body.clone(),
                    is_private: f.is_private,
                };
                if f.is_class_method {
                    if let Some(Value::Class(c)) = self.get_var("self") {
                        c.class_methods.write().unwrap().insert(f.name.clone(), def);
                    }
                } else {
                    if let Some(Value::Class(c)) = self.get_var("self") {
                        c.methods.write().unwrap().insert(f.name.clone(), def);
                    } else {
                        self.functions.insert(f.name.clone(), def);
                    }
                }
                Ok(ControlFlow::Continue(Value::Nil))
            }
            Stmt::EnumDef { name, members } => {
                let mut variants = HashMap::new();
                for m in members {
                    variants.insert(
                        m.name.clone(),
                        Value::EnumVariant {
                            enum_name: name.clone(),
                            variant_name: m.name.clone(),
                        },
                    );
                }
                let enum_hash = Value::new_hash(variants);
                self.set_var(name, enum_hash);
                Ok(ControlFlow::Continue(Value::Nil))
            }
            Stmt::ClassDef { name, body } => {
                let class_def = Arc::new(ClassDef {
                    name: name.clone(),
                    methods: RwLock::new(HashMap::new()),
                    class_methods: RwLock::new(HashMap::new()),
                    class_vars: RwLock::new(HashMap::new()),
                });
                self.classes.insert(name.clone(), class_def.clone());
                self.set_var(name, Value::Class(class_def.clone()));

                self.stack.push(HashMap::new());
                self.set_var("self", Value::Class(class_def.clone()));
                for stmt in body {
                    let cf = self.eval_stmt(stmt)?;
                    if !cf.is_continue() {
                        self.stack.pop();
                        return Ok(cf);
                    }
                }
                self.stack.pop();

                Ok(ControlFlow::Continue(Value::Nil))
            }
            Stmt::PropertyDecl { names, .. } => {
                if let Some(Value::Class(c)) = self.get_var("self") {
                    for name in names {
                        let getter_body = vec![Stmt::Return(Some(Expr::InstanceVar(name.clone())))];
                        c.methods.write().unwrap().insert(
                            name.clone(),
                            FunctionDef {
                                params: vec![],
                                body: getter_body,
                                is_private: false,
                            },
                        );
                        let setter_name = format!("{}=", name);
                        let setter_body = vec![Stmt::IvarAssignment {
                            name: name.clone(),
                            value: Expr::Variable("val".to_string()),
                        }];
                        c.methods.write().unwrap().insert(
                            setter_name,
                            FunctionDef {
                                params: vec![Param {
                                    name: "val".to_string(),
                                    is_ivar: false,
                                }],
                                body: setter_body,
                                is_private: false,
                            },
                        );
                    }
                }
                Ok(ControlFlow::Continue(Value::Nil))
            }
            Stmt::Return(expr) => {
                let val = if let Some(e) = expr {
                    self.eval_expr(e)?
                } else {
                    Value::Nil
                };
                Ok(ControlFlow::Return(val))
            }
            Stmt::Assert { condition, message } => {
                let cond = self.eval_expr(condition)?;
                if !self.is_truthy(&cond) {
                    let msg = if let Some(m) = message {
                        self.eval_expr(m)?.to_string()
                    } else {
                        "assertion failed".to_string()
                    };
                    return Err(EvalError::Message(msg));
                }
                Ok(ControlFlow::Continue(Value::Nil))
            }
            Stmt::Break => Ok(ControlFlow::Break(Value::Nil)),
            Stmt::Next => Ok(ControlFlow::Next(Value::Nil)),
            Stmt::Try {
                body,
                rescue,
                ensure,
            } => {
                let res = self.eval_block(body);
                let final_res = match res {
                    Ok(cf) => Ok(cf),
                    Err(EvalError::Message(_)) => {
                        if let Some(r) = rescue {
                            self.eval_block(&r.body)
                        } else {
                            res
                        }
                    }
                };
                if let Some(e) = ensure {
                    let _ = self.eval_block(e);
                }
                final_res
            }
        }
    }

    pub fn eval_expr(&mut self, expr: &Expr) -> Result<Value, EvalError> {
        match expr {
            Expr::Literal(val) => Ok(val.clone()),
            Expr::Variable(name) => {
                if name == "uuid" {
                    return Ok(Value::String(Uuid::new_v4().to_string()));
                }
                self.get_var(name)
                    .ok_or_else(|| EvalError::Message(format!("Variable '{}' not found", name)))
            }
            Expr::InstanceVar(name) => self.get_ivar(name).map_err(EvalError::Message),
            Expr::ClassVar(name) => self.get_cvar(name).map_err(EvalError::Message),
            Expr::Binary { left, op, right } => {
                let lhs = self.eval_expr(left)?;
                if *op == BinaryOp::And {
                    return if self.is_truthy(&lhs) {
                        self.eval_expr(right)
                    } else {
                        Ok(lhs)
                    };
                }
                if *op == BinaryOp::Or {
                    return if self.is_truthy(&lhs) {
                        Ok(lhs)
                    } else {
                        self.eval_expr(right)
                    };
                }
                let rhs = self.eval_expr(right)?;
                self.eval_binary(lhs, *op, rhs).map_err(EvalError::Message)
            }
            Expr::Unary { op, expr } => {
                let val = self.eval_expr(expr)?;
                self.eval_unary(*op, &val).map_err(EvalError::Message)
            }
            Expr::Call {
                func,
                args,
                kwargs,
                block,
            } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(arg)?);
                }

                let mut kwarg_vals = HashMap::new();
                for (name, val_expr) in kwargs {
                    kwarg_vals.insert(name.clone(), self.eval_expr(val_expr)?);
                }

                match func.as_str() {
                    "money" => {
                        if let Some(Value::String(s)) = arg_vals.first() {
                            // "50.00 USD" -> 5000 cents
                            let parts: Vec<&str> = s.split_whitespace().collect();
                            if parts.len() == 2 {
                                let amount: f64 = parts[0].parse().unwrap_or(0.0);
                                let cents = (amount * 100.0).round() as i64;
                                let currency = parts[1].to_string();
                                return Ok(Value::Money { cents, currency });
                            }
                        }
                    }
                    "money_cents" => {
                        if let Some(cents) = arg_vals.get(0).and_then(|v| v.as_int()) {
                            if let Some(Value::String(currency)) = arg_vals.get(1) {
                                return Ok(Value::Money {
                                    cents,
                                    currency: currency.clone(),
                                });
                            }
                        }
                    }
                    "json_parse" => {
                        if let Some(Value::String(s)) = arg_vals.first() {
                            let v: serde_json::Value = serde_json::from_str(s).map_err(|e| {
                                EvalError::Message(format!("JSON parse error: {}", e))
                            })?;
                            return Ok(self.json_to_vibe(v));
                        }
                    }
                    "uuid" => return Ok(Value::String(Uuid::new_v4().to_string())),
                    "now" => return Ok(Value::Time(Utc::now())),
                    "to_int" => {
                        return Ok(arg_vals
                            .first()
                            .and_then(|v| v.as_int())
                            .map(Value::Int)
                            .unwrap_or(Value::Nil));
                    }
                    "to_float" => {
                        return Ok(arg_vals
                            .first()
                            .and_then(|v| v.as_float())
                            .map(Value::Float)
                            .unwrap_or(Value::Nil));
                    }
                    "random_id" => {
                        let len = arg_vals.first().and_then(|v| v.as_int()).unwrap_or(16) as usize;
                        let s: String = rand::thread_rng()
                            .sample_iter(&Alphanumeric)
                            .take(len)
                            .map(char::from)
                            .collect();
                        return Ok(Value::String(s));
                    }
                    _ => {}
                }

                if let Some(f) = self.functions.get(func).cloned() {
                    return self.call_function_def(&f, arg_vals, block.as_deref());
                }

                if let Some(Value::Instance(inst)) = self.get_var("self") {
                    let class = inst.read().unwrap().class.clone();
                    if let Some(f) = class.methods.read().unwrap().get(func).cloned() {
                        return self.call_function_def(&f, arg_vals, block.as_deref());
                    }
                }

                if let Some(class) = self.classes.get(func).cloned() {
                    if let Some(f) = class.class_methods.read().unwrap().get(func).cloned() {
                        return self.call_function_def(&f, arg_vals, block.as_deref());
                    }
                }

                Err(EvalError::Message(format!("Function '{}' not found", func)))
            }
            Expr::Member {
                receiver,
                method,
                args,
                kwargs,
                block,
            } => {
                let recv_val = self.eval_expr(receiver)?;
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(arg)?);
                }
                let mut kwarg_vals = HashMap::new();
                for (name, val_expr) in kwargs {
                    kwarg_vals.insert(name.clone(), self.eval_expr(val_expr)?);
                }
                self.eval_member(recv_val, method, arg_vals, kwarg_vals, block.as_deref())
            }
            Expr::Array(elements) => {
                let mut vals = Vec::new();
                for e in elements {
                    vals.push(self.eval_expr(e)?);
                }
                Ok(Value::new_array(vals))
            }
            Expr::Hash(entries) => {
                let mut hash = HashMap::new();
                for (key, val_expr) in entries {
                    hash.insert(key.clone(), self.eval_expr(val_expr)?);
                }
                Ok(Value::new_hash(hash))
            }
            Expr::Case {
                target,
                clauses,
                else_expr,
            } => {
                let t = self.eval_expr(target)?;
                for clause in clauses {
                    for val_expr in &clause.values {
                        let val = self.eval_expr(val_expr)?;
                        if val == t {
                            return self.eval_block(&clause.body).map(|cf| cf.value());
                        }
                    }
                }
                if let Some(e) = else_expr {
                    self.eval_block(e).map(|cf| cf.value())
                } else {
                    Ok(Value::Nil)
                }
            }
            Expr::Block { params, body } => Ok(Value::Block {
                params: params.clone(),
                body: body.clone(),
            }),
            Expr::InterpolatedString(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        StringPart::Text(s) => result.push_str(s),
                        StringPart::Expr(e) => {
                            let val = self.eval_expr(e)?;
                            result.push_str(&val.to_string());
                        }
                    }
                }
                Ok(Value::String(result))
            }
        }
    }

    fn eval_block(&mut self, stmts: &[Stmt]) -> Result<ControlFlow, EvalError> {
        let mut last_val = Value::Nil;
        for stmt in stmts {
            let cf = self.eval_stmt(stmt)?;
            if !cf.is_continue() {
                return Ok(cf);
            }
            last_val = cf.value();
        }
        Ok(ControlFlow::Continue(last_val))
    }

    fn get_var(&self, name: &str) -> Option<Value> {
        for scope in self.stack.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Some(val.clone());
            }
        }
        self.globals.get(name).cloned()
    }

    fn set_var(&mut self, name: &str, val: Value) {
        if let Some(scope) = self.stack.last_mut() {
            scope.insert(name.to_string(), val);
        } else {
            self.globals.insert(name.to_string(), val);
        }
    }

    fn get_ivar(&self, name: &str) -> Result<Value, String> {
        if let Some(Value::Instance(inst)) = self.get_var("self") {
            Ok(inst
                .read()
                .unwrap()
                .ivars
                .get(name)
                .cloned()
                .unwrap_or(Value::Nil))
        } else {
            Err(format!(
                "Cannot access instance variable '{}' outside instance",
                name
            ))
        }
    }

    fn set_ivar(&mut self, name: &str, val: Value) -> Result<(), EvalError> {
        if let Some(Value::Instance(inst)) = self.get_var("self") {
            inst.write().unwrap().ivars.insert(name.to_string(), val);
            Ok(())
        } else {
            Err(EvalError::Message(format!(
                "Cannot set instance variable '{}' outside instance",
                name
            )))
        }
    }

    fn get_cvar(&self, name: &str) -> Result<Value, String> {
        self.class_vars
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Class variable '{}' not found", name))
    }

    fn set_cvar(&mut self, name: &str, val: Value) -> Result<(), EvalError> {
        self.class_vars.insert(name.to_string(), val);
        Ok(())
    }

    pub fn json_to_vibe(&self, json: serde_json::Value) -> Value {
        match json {
            serde_json::Value::Null => Value::Nil,
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Int(i)
                } else {
                    Value::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::String(s) => Value::String(s),
            serde_json::Value::Array(a) => {
                Value::new_array(a.into_iter().map(|v| self.json_to_vibe(v)).collect())
            }
            serde_json::Value::Object(m) => {
                let mut hash = HashMap::new();
                for (k, v) in m {
                    hash.insert(k, self.json_to_vibe(v));
                }
                Value::new_hash(hash)
            }
        }
    }

    pub fn vibe_to_json(&self, vibe: Value) -> serde_json::Value {
        match vibe {
            Value::Nil => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(b),
            Value::Int(i) => serde_json::Value::Number(serde_json::Number::from(i)),
            Value::Float(f) => {
                if let Some(n) = serde_json::Number::from_f64(f) {
                    serde_json::Value::Number(n)
                } else {
                    serde_json::Value::Null
                }
            }
            Value::String(s) => serde_json::Value::String(s),
            Value::Symbol(s) => serde_json::Value::String(format!(":{}", s)),
            Value::Time(t) => serde_json::Value::String(t.to_rfc3339()),
            Value::Duration(s) => serde_json::Value::String(format!("{}s", s)),
            Value::Money { cents, currency } => serde_json::Value::String(format!(
                "{}.{:02} {}",
                cents / 100,
                cents % 100,
                currency
            )),
            Value::EnumVariant {
                enum_name,
                variant_name,
            } => serde_json::Value::String(format!("{}.{}", enum_name, variant_name)),
            Value::Array(a) => {
                let arr = a.read().unwrap();
                serde_json::Value::Array(arr.iter().map(|v| self.vibe_to_json(v.clone())).collect())
            }
            Value::Hash(h) => {
                let hash = h.read().unwrap();
                let mut map = serde_json::Map::new();
                for (k, v) in hash.iter() {
                    map.insert(k.clone(), self.vibe_to_json(v.clone()));
                }
                serde_json::Value::Object(map)
            }
            _ => serde_json::Value::Null,
        }
    }

    fn is_truthy(&self, val: &Value) -> bool {
        match val {
            Value::Nil => false,
            Value::Bool(b) => *b,
            _ => true,
        }
    }

    fn eval_member(
        &mut self,
        receiver: Value,
        method: &str,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
        block: Option<&Expr>,
    ) -> Result<Value, EvalError> {
        if let Value::Namespace(ref ns) = receiver {
            match (ns.as_str(), method) {
                ("JSON", "parse") => {
                    if let Some(Value::String(s)) = args.first() {
                        let v: serde_json::Value = serde_json::from_str(s)
                            .map_err(|e| EvalError::Message(format!("JSON parse error: {}", e)))?;
                        return Ok(self.json_to_vibe(v));
                    }
                }
                ("JSON", "stringify") => {
                    if let Some(v) = args.first() {
                        let json = self.vibe_to_json(v.clone());
                        let s = serde_json::to_string(&json).map_err(|e| {
                            EvalError::Message(format!("JSON stringify error: {}", e))
                        })?;
                        return Ok(Value::String(s));
                    }
                }
                ("Time", "now") => {
                    return Ok(Value::Time(Utc::now()));
                }
                ("Time", "parse") => {
                    if let Some(Value::String(s)) = args.first() {
                        // Try various formats
                        let t = DateTime::parse_from_rfc3339(s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .or_else(|_| {
                                DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S %z")
                                    .map(|dt| dt.with_timezone(&Utc))
                            })
                            .or_else(|_| Utc.datetime_from_str(s, "%Y-%m-%d %H:%M:%S"))
                            .or_else(|_| Utc.datetime_from_str(s, "%Y-%m-%d"))
                            .map_err(|e| EvalError::Message(format!("Time parse error: {}", e)))?;
                        return Ok(Value::Time(t));
                    }
                }
                ("Regex", "match") => {
                    let pattern = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                    let s = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                    let re = Regex::new(pattern).map_err(|e| EvalError::Message(e.to_string()))?;
                    if let Some(caps) = re.captures(s) {
                        let groups: Vec<Value> = caps
                            .iter()
                            .map(|m| {
                                m.map_or(Value::Nil, |mat| Value::String(mat.as_str().to_string()))
                            })
                            .collect();
                        return Ok(Value::new_array(groups));
                    } else {
                        return Ok(Value::Nil);
                    }
                }
                ("Regex", "replace") => {
                    let s = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                    let pattern = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                    let replacement = args.get(2).and_then(|v| v.as_str()).unwrap_or("");
                    let re = Regex::new(pattern).map_err(|e| EvalError::Message(e.to_string()))?;
                    return Ok(Value::String(re.replace(s, replacement).to_string()));
                }
                ("Regex", "replace_all") => {
                    let s = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                    let pattern = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                    let replacement = args.get(2).and_then(|v| v.as_str()).unwrap_or("");
                    let re = Regex::new(pattern).map_err(|e| EvalError::Message(e.to_string()))?;
                    return Ok(Value::String(re.replace_all(s, replacement).to_string()));
                }
                _ => {
                    return Err(EvalError::Message(format!(
                        "Method '{}' not found for namespace '{}'",
                        method, ns
                    )));
                }
            }
        }

        // High priority: Standard methods
        match (receiver.clone(), method, block) {
            (Value::Array(a), "length" | "size", _) => {
                return Ok(Value::Int(a.read().unwrap().len() as i64));
            }
            (Value::Array(a), "first", _) => {
                let arr = a.read().unwrap();
                if let Some(Value::Int(n)) = args.first() {
                    let count = (*n).max(0) as usize;
                    let taken = arr.iter().take(count).cloned().collect();
                    return Ok(Value::new_array(taken));
                } else {
                    return Ok(arr.first().cloned().unwrap_or(Value::Nil));
                }
            }
            (Value::Array(a), "sum", _) => {
                let arr = a.read().unwrap();
                let mut total = 0;
                for val in arr.iter() {
                    if let Value::Int(i) = val {
                        total += i;
                    }
                }
                return Ok(Value::Int(total));
            }
            (Value::String(s), "length" | "size", _) => return Ok(Value::Int(s.len() as i64)),
            (Value::Hash(h), "length" | "size", _) => {
                return Ok(Value::Int(h.read().unwrap().len() as i64));
            }
            (Value::Hash(h), "keys", _) => {
                let hash = h.read().unwrap();
                let keys = hash.keys().map(|k| Value::String(k.clone())).collect();
                return Ok(Value::new_array(keys));
            }
            (Value::Hash(h), "values", _) => {
                let hash = h.read().unwrap();
                let values = hash.values().cloned().collect();
                return Ok(Value::new_array(values));
            }
            (Value::Hash(h), "fetch", _) => {
                let hash = h.read().unwrap();
                if let Some(Value::String(key)) = args.first() {
                    if let Some(val) = hash.get(key).cloned() {
                        return Ok(val);
                    }
                    if let Some(default) = args.get(1) {
                        return Ok(default.clone());
                    }
                    return Err(EvalError::Message(format!("Key '{}' not found", key)));
                }
                return Err(EvalError::Message("fetch expects a key".to_string()));
            }
            (Value::Hash(h), "merge", _) => {
                if let Some(Value::Hash(other_h)) = args.first() {
                    let hash = h.read().unwrap();
                    let other_hash = other_h.read().unwrap();
                    let mut new_hash = hash.clone();
                    new_hash.extend(other_hash.iter().map(|(k, v)| (k.clone(), v.clone())));
                    return Ok(Value::new_hash(new_hash));
                }
            }
            _ => {}
        }

        match (receiver.clone(), method, block) {
            (Value::Instance(inst), method, _) => {
                let class = inst.read().unwrap().class.clone();
                if let Some(f) = class.methods.read().unwrap().get(method).cloned() {
                    if f.is_private {
                        if let Some(Value::Instance(curr_self)) = self.get_var("self") {
                            if !Arc::ptr_eq(&curr_self, &inst) {
                                return Err(EvalError::Message(format!(
                                    "Method '{}' is private",
                                    method
                                )));
                            }
                        } else {
                            return Err(EvalError::Message(format!(
                                "Method '{}' is private",
                                method
                            )));
                        }
                    }
                    self.stack.push(HashMap::new());
                    self.set_var("self", Value::Instance(inst.clone()));
                    let cf = self.call_function_def_cf(&f, args, block);
                    self.stack.pop();
                    cf.map(|c| c.value())
                } else {
                    Err(EvalError::Message(format!(
                        "Method '{}' not found for class '{}'",
                        method, class.name
                    )))
                }
            }

            (Value::Class(class), "new", _) => {
                let inst = Arc::new(RwLock::new(InstanceData {
                    class: class.clone(),
                    ivars: HashMap::new(),
                }));
                if let Some(f) = class.methods.read().unwrap().get("initialize").cloned() {
                    self.stack.push(HashMap::new());
                    self.set_var("self", Value::Instance(inst.clone()));
                    let _ = self.call_function_def_cf(&f, args, None)?;
                    self.stack.pop();
                }
                Ok(Value::Instance(inst))
            }

            (Value::EnumVariant { variant_name, .. }, "name", _) => {
                Ok(Value::String(variant_name.clone()))
            }
            (Value::EnumVariant { variant_name, .. }, "symbol", _) => {
                Ok(Value::Symbol(variant_name.to_lowercase()))
            }

            (Value::Time(t), "format", _) => {
                if let Some(Value::String(fmt)) = args.first() {
                    let rust_fmt = match fmt.as_str() {
                        "2006-01-02T15:04:05Z07:00" => "%Y-%m-%dT%H:%M:%S%Z",
                        _ => fmt,
                    };
                    return Ok(Value::String(t.format(rust_fmt).to_string()));
                }
                Ok(Value::String(t.to_rfc3339()))
            }

            (Value::Duration(s), "seconds", _) => Ok(Value::Int(s)),

            (Value::Int(i), "minutes", _) => Ok(Value::Duration(i * 60)),
            (Value::Int(i), "hours", _) => Ok(Value::Duration(i * 3600)),
            (Value::Int(i), "days", _) => Ok(Value::Duration(i * 86400)),
            (Value::Int(i), "seconds", _) => Ok(Value::Duration(i)),

            (Value::Float(f), "minutes", _) => Ok(Value::Duration((f * 60.0) as i64)),
            (Value::Float(f), "hours", _) => Ok(Value::Duration((f * 3600.0) as i64)),
            (Value::Float(f), "days", _) => Ok(Value::Duration((f * 86400.0) as i64)),
            (Value::Float(f), "seconds", _) => Ok(Value::Duration(f as i64)),

            (Value::Hash(h), "[]=", _) => {
                if args.len() < 2 {
                    return Err(EvalError::Message(
                        "[]= expects a key and a value".to_string(),
                    ));
                }
                let key_val = &args[0];
                let new_val = &args[1];
                if let Value::String(s) = key_val {
                    let mut hash = h.write().unwrap();
                    hash.insert(s.clone(), new_val.clone());
                    Ok(new_val.clone())
                } else {
                    Err(EvalError::Message("Hash key must be a string".to_string()))
                }
            }

            (Value::Hash(h), method, _) => {
                let hash = h.read().unwrap();
                if let Some(val) = hash.get(method).cloned() {
                    Ok(val)
                } else {
                    Err(EvalError::Message(format!(
                        "Method '{}' not found for Hash",
                        method
                    )))
                }
            }

            (_, "to_string", _) => Ok(Value::String(receiver.to_string())),

            (Value::String(s), "uppercase" | "upcase", _) => Ok(Value::String(s.to_uppercase())),
            (Value::String(s), "lowercase" | "downcase", _) => Ok(Value::String(s.to_lowercase())),
            (Value::String(s), "capitalize", _) => {
                let mut c = s.chars();
                match c.next() {
                    None => Ok(Value::String(String::new())),
                    Some(f) => Ok(Value::String(
                        f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                    )),
                }
            }
            (Value::String(s), "swapcase", _) => {
                let swapped = s
                    .chars()
                    .map(|c| {
                        if c.is_uppercase() {
                            c.to_lowercase().to_string()
                        } else {
                            c.to_uppercase().to_string()
                        }
                    })
                    .collect::<String>();
                Ok(Value::String(swapped))
            }
            (Value::String(s), "reverse", _) => {
                Ok(Value::String(s.chars().rev().collect::<String>()))
            }
            (Value::String(s), "empty?", _) => Ok(Value::Bool(s.is_empty())),
            (Value::String(s), "start_with?" | "starts_with?", _) => {
                if let Some(Value::String(prefix)) = args.first() {
                    Ok(Value::Bool(s.starts_with(prefix)))
                } else {
                    Ok(Value::Bool(false))
                }
            }
            (Value::String(s), "end_with?" | "ends_with?", _) => {
                if let Some(Value::String(suffix)) = args.first() {
                    Ok(Value::Bool(s.ends_with(suffix)))
                } else {
                    Ok(Value::Bool(false))
                }
            }
            (Value::String(s), "contains?" | "include?", _) => {
                if let Some(Value::String(sub)) = args.first() {
                    Ok(Value::Bool(s.contains(sub)))
                } else {
                    Ok(Value::Bool(false))
                }
            }
            (Value::String(s), "lstrip", _) => Ok(Value::String(s.trim_start().to_string())),
            (Value::String(s), "rstrip", _) => Ok(Value::String(s.trim_end().to_string())),
            (Value::String(s), "strip!", _) => {
                let trimmed = s.trim();
                if trimmed == s {
                    Ok(Value::Nil)
                } else {
                    Ok(Value::String(trimmed.to_string()))
                }
            }
            (Value::String(s), "strip", _) => Ok(Value::String(s.trim().to_string())),
            (Value::String(s), "replace", _) => {
                if let Some(new_val) = args.first() {
                    Ok(Value::String(new_val.to_string()))
                } else {
                    Ok(Value::String(s.clone()))
                }
            }
            (Value::String(s), "delete_prefix", _) => {
                if let Some(Value::String(prefix)) = args.first() {
                    let result = s.strip_prefix(prefix).unwrap_or(&s).to_string();
                    if result == s {
                        Ok(Value::String(s.clone()))
                    } else {
                        Ok(Value::String(result))
                    }
                } else {
                    Ok(Value::String(s.clone()))
                }
            }
            (Value::String(s), "delete_suffix", _) => {
                if let Some(Value::String(suffix)) = args.first() {
                    let result = s.strip_suffix(suffix).unwrap_or(&s).to_string();
                    if result == s {
                        Ok(Value::String(s.clone()))
                    } else {
                        Ok(Value::String(result))
                    }
                } else {
                    Ok(Value::String(s.clone()))
                }
            }
            (Value::String(_), "clear", _) => Ok(Value::String(String::new())),
            (Value::String(s), "concat", _) => {
                let mut result = s.clone();
                for arg in args {
                    result.push_str(&arg.to_string());
                }
                Ok(Value::String(result))
            }
            (Value::String(s), "bytesize", _) => Ok(Value::Int(s.len() as i64)),
            (Value::String(s), "ord", _) => {
                if let Some(c) = s.chars().next() {
                    Ok(Value::Int(c as i64))
                } else {
                    Err(EvalError::Message("empty string".to_string()))
                }
            }
            (Value::String(s), "chr", _) => {
                if let Some(c) = s.chars().next() {
                    Ok(Value::String(c.to_string()))
                } else {
                    Ok(Value::String(String::new()))
                }
            }
            (Value::String(s), "split", _) => {
                let sep = if let Some(Value::String(sep)) = args.first() {
                    Some(sep.as_str())
                } else {
                    None
                };

                let parts = if let Some(sep) = sep {
                    if sep.is_empty() {
                        s.chars().map(|c| Value::String(c.to_string())).collect()
                    } else {
                        s.split(sep)
                            .map(|part| Value::String(part.to_string()))
                            .collect()
                    }
                } else {
                    s.split_whitespace()
                        .map(|part| Value::String(part.to_string()))
                        .collect()
                };
                Ok(Value::new_array(parts))
            }
            (Value::String(s), "index", _) => {
                if let Some(Value::String(sub)) = args.first() {
                    if let Some(idx) = s.find(sub) {
                        // Use character index for UTF-8 compatibility
                        let char_idx = s[..idx].chars().count();
                        Ok(Value::Int(char_idx as i64))
                    } else {
                        Ok(Value::Nil)
                    }
                } else {
                    Ok(Value::Nil)
                }
            }
            (Value::String(s), "rindex", _) => {
                if let Some(Value::String(sub)) = args.first() {
                    if let Some(idx) = s.rfind(sub) {
                        // Use character index for UTF-8 compatibility
                        let char_idx = s[..idx].chars().count();
                        Ok(Value::Int(char_idx as i64))
                    } else {
                        Ok(Value::Nil)
                    }
                } else {
                    Ok(Value::Nil)
                }
            }
            (Value::String(s), "sub" | "sub!" | "gsub" | "gsub!", _) => {
                let pattern = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let replacement = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                let is_regex = kwargs
                    .get("regex")
                    .map(|v| self.is_truthy(v))
                    .unwrap_or(false);
                let all = method.contains("gsub");

                let result = if is_regex {
                    let re = Regex::new(pattern).map_err(|e| EvalError::Message(e.to_string()))?;
                    if all {
                        re.replace_all(&s, replacement).to_string()
                    } else {
                        re.replace(&s, replacement).to_string()
                    }
                } else {
                    if all {
                        s.replace(pattern, replacement)
                    } else {
                        s.replacen(pattern, replacement, 1)
                    }
                };

                if method.ends_with('!') {
                    if result == s {
                        Ok(Value::Nil)
                    } else {
                        Ok(Value::String(result))
                    }
                } else {
                    Ok(Value::String(result))
                }
            }
            (Value::String(s), "match", _) => {
                let pattern = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let re = Regex::new(pattern).map_err(|e| EvalError::Message(e.to_string()))?;
                if let Some(caps) = re.captures(&s) {
                    let groups: Vec<Value> = caps
                        .iter()
                        .map(|m| {
                            m.map_or(Value::Nil, |mat| Value::String(mat.as_str().to_string()))
                        })
                        .collect();
                    Ok(Value::new_array(groups))
                } else {
                    Ok(Value::Nil)
                }
            }
            (Value::String(s), "scan", _) => {
                let pattern = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let re = Regex::new(pattern).map_err(|e| EvalError::Message(e.to_string()))?;
                let matches: Vec<Value> = re
                    .find_iter(&s)
                    .map(|mat| Value::String(mat.as_str().to_string()))
                    .collect();
                Ok(Value::new_array(matches))
            }
            (Value::String(s), "slice", _) => {
                let chars: Vec<char> = s.chars().collect();
                if args.len() == 1 {
                    if let Value::Int(i) = args[0] {
                        let idx = if i < 0 { chars.len() as i64 + i } else { i } as usize;
                        Ok(chars
                            .get(idx)
                            .map_or(Value::Nil, |c| Value::String(c.to_string())))
                    } else {
                        Ok(Value::Nil)
                    }
                } else if args.len() >= 2 {
                    if let (Value::Int(start), Value::Int(len)) = (&args[0], &args[1]) {
                        let start_idx = if *start < 0 {
                            chars.len() as i64 + *start
                        } else {
                            *start
                        } as usize;
                        let end_idx = (start_idx + (*len as usize)).min(chars.len());
                        if start_idx >= chars.len() {
                            Ok(Value::Nil)
                        } else {
                            let sub: String = chars[start_idx..end_idx].iter().collect();
                            Ok(Value::String(sub))
                        }
                    } else {
                        Ok(Value::Nil)
                    }
                } else {
                    Ok(Value::Nil)
                }
            }
            (Value::String(s), "squish!", _) => {
                let re = Regex::new(r"\s+").unwrap();
                let squished = re.replace_all(s.trim(), " ").to_string();
                if squished == s {
                    Ok(Value::Nil)
                } else {
                    Ok(Value::String(squished))
                }
            }
            (Value::String(s), "squish", _) => {
                let re = Regex::new(r"\s+").unwrap();
                let squished = re.replace_all(s.trim(), " ").to_string();
                Ok(Value::String(squished))
            }
            (Value::String(s), "chomp", _) => {
                let suffix = args.get(0).and_then(|v| v.as_str()).unwrap_or("\n");
                Ok(Value::String(
                    s.strip_suffix(suffix).unwrap_or(&s).to_string(),
                ))
            }
            (Value::String(s), "template", _) => {
                let mut result = s.clone();
                if let Some(Value::Hash(h)) = args.first() {
                    let hash = h.read().unwrap();
                    // Basic implementation for {{key}} and {{nested.key}}
                    let re = Regex::new(r"\{\{([a-zA-Z0-9_\.]+)\}\}").unwrap();

                    let mut replaced = result.clone();
                    for cap in re.captures_iter(&result) {
                        let full_match = &cap[0];
                        let key_path = &cap[1];

                        let mut current_val = Value::Nil;
                        let mut found = false;

                        // Handle nested keys like user.name
                        let parts: Vec<&str> = key_path.split('.').collect();
                        if let Some(first_key) = parts.first() {
                            if let Some(val) = hash.get(*first_key) {
                                current_val = val.clone();
                                found = true;
                                for part in parts.iter().skip(1) {
                                    let next_val = if let Value::Hash(ref inner_h) = current_val {
                                        inner_h.read().unwrap().get(*part).cloned()
                                    } else {
                                        None
                                    };

                                    if let Some(v) = next_val {
                                        current_val = v;
                                    } else {
                                        found = false;
                                        break;
                                    }
                                }
                            }
                        }

                        if found && !current_val.is_nil() {
                            replaced = replaced.replace(full_match, &current_val.to_string());
                        }
                    }
                    result = replaced;
                }
                Ok(Value::String(result))
            }

            (Value::Array(a), "push", _) => {
                let mut arr = a.write().unwrap();
                for arg in args {
                    arr.push(arg);
                }
                drop(arr);
                Ok(Value::Array(a.clone()))
            }
            (Value::Array(a), "pop", _) => {
                let mut arr = a.write().unwrap();
                let val = arr.pop().unwrap_or(Value::Nil);
                drop(arr);
                Ok(val)
            }
            (Value::Array(a), "include?" | "contains?", _) => {
                if let Some(target) = args.first() {
                    let arr = a.read().unwrap();
                    Ok(Value::Bool(arr.contains(target)))
                } else {
                    Ok(Value::Bool(false))
                }
            }
            (Value::Array(a), "join", _) => {
                let sep = if let Some(Value::String(s)) = args.first() {
                    s.clone()
                } else {
                    "".to_string()
                };
                let arr = a.read().unwrap();
                let joined = arr
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(&sep);
                Ok(Value::String(joined))
            }
            (Value::Array(a), "map", Some(Expr::Block { params, body })) => {
                let arr = a.read().unwrap().clone();
                let mut results = Vec::new();
                for val in arr {
                    let mut scope = HashMap::new();
                    if let Some(p) = params.first() {
                        scope.insert(p.clone(), val);
                    }
                    self.stack.push(scope);
                    let cf = self.eval_block(body)?;
                    self.stack.pop();
                    results.push(cf.value());
                }
                Ok(Value::new_array(results))
            }
            (Value::Array(a), "select", Some(Expr::Block { params, body })) => {
                let arr = a.read().unwrap().clone();
                let mut results = Vec::new();
                for val in arr {
                    let mut scope = HashMap::new();
                    if let Some(p) = params.first() {
                        scope.insert(p.clone(), val.clone());
                    }
                    self.stack.push(scope);
                    let cf = self.eval_block(body)?;
                    self.stack.pop();
                    if self.is_truthy(&cf.value()) {
                        results.push(val);
                    }
                }
                Ok(Value::new_array(results))
            }
            (Value::Array(a), "reduce", Some(Expr::Block { params, body })) => {
                let mut acc = args.first().cloned().unwrap_or(Value::Nil);
                let arr = a.read().unwrap().clone();
                for val in arr {
                    let mut scope = HashMap::new();
                    if let Some(p) = params.first() {
                        scope.insert(p.clone(), acc);
                    }
                    if let Some(p) = params.get(1) {
                        scope.insert(p.clone(), val);
                    }
                    self.stack.push(scope);
                    let cf = self.eval_block(body)?;
                    self.stack.pop();
                    acc = cf.value();
                }
                Ok(acc)
            }

            (Value::Array(a), "[]=", _) => {
                if args.len() < 2 {
                    return Err(EvalError::Message(
                        "[]= expects an index and a value".to_string(),
                    ));
                }
                let idx_val = &args[0];
                let new_val = &args[1];
                if let Value::Int(i) = idx_val {
                    let mut arr = a.write().unwrap();
                    let idx = if *i < 0 { arr.len() as i64 + i } else { *i };
                    if idx < 0 || idx >= arr.len() as i64 {
                        return Err(EvalError::Message(format!(
                            "Array index {} out of bounds (length {})",
                            i,
                            arr.len()
                        )));
                    }
                    arr[idx as usize] = new_val.clone();
                    Ok(new_val.clone())
                } else {
                    Err(EvalError::Message(
                        "Array index must be an integer".to_string(),
                    ))
                }
            }

            (Value::Array(a), "each", Some(Expr::Block { params, body })) => {
                let arr = a.read().unwrap().clone();
                for val in arr {
                    let mut scope = HashMap::new();
                    if let Some(p) = params.first() {
                        scope.insert(p.clone(), val);
                    }
                    self.stack.push(scope);
                    let cf = self.eval_block(body)?;
                    self.stack.pop();
                    if !cf.is_continue() {
                        if matches!(cf, ControlFlow::Break(_)) {
                            break;
                        }
                        return Ok(cf.value());
                    }
                }
                Ok(Value::Array(a))
            }

            _ => Err(EvalError::Message(format!(
                "Method '{}' not supported for this type",
                method
            ))),
        }
    }

    pub fn call_function(&mut self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let fn_def = self
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Function '{}' not found", name))?;

        self.call_function_def(&fn_def, args, None)
            .map_err(|e| match e {
                EvalError::Message(m) => m,
            })
    }

    fn call_function_def(
        &mut self,
        func: &FunctionDef,
        args: Vec<Value>,
        _block: Option<&Expr>,
    ) -> Result<Value, EvalError> {
        self.call_function_def_cf(func, args, _block)
            .map(|cf| cf.value())
    }

    fn call_function_def_cf(
        &mut self,
        func: &FunctionDef,
        args: Vec<Value>,
        _block: Option<&Expr>,
    ) -> Result<ControlFlow, EvalError> {
        self.recursion_depth += 1;
        if self.recursion_depth > MAX_RECURSION_DEPTH {
            self.recursion_depth -= 1;
            return Err(EvalError::Message(
                "Stack overflow: maximum recursion depth exceeded".to_string(),
            ));
        }

        let result = self.do_call_function_def(func, args, _block);

        self.recursion_depth -= 1;
        result
    }

    fn do_call_function_def(
        &mut self,
        func: &FunctionDef,
        args: Vec<Value>,
        _block: Option<&Expr>,
    ) -> Result<ControlFlow, EvalError> {
        if func.params.len() != args.len() {
            return Err(EvalError::Message(format!(
                "Expected {} arguments, but got {}",
                func.params.len(),
                args.len()
            )));
        }

        let mut new_scope = HashMap::new();
        for (param, val) in func.params.iter().zip(args) {
            new_scope.insert(param.name.clone(), val.clone());
            if param.is_ivar {
                self.stack.push(new_scope.clone());
                self.set_ivar(&param.name, val.clone())?;
                new_scope = self.stack.pop().unwrap();
            }
        }

        self.stack.push(new_scope);
        let cf = self.eval_block(&func.body)?;
        self.stack.pop();

        Ok(cf)
    }

    fn eval_unary(&self, op: UnaryOp, val: &Value) -> Result<Value, String> {
        match (op, val) {
            (UnaryOp::Neg, Value::Int(i)) => Ok(Value::Int(-i)),
            (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
            (UnaryOp::Not, val) => Ok(Value::Bool(!self.is_truthy(val))),
            _ => Err("Unary operation not supported for this type".to_string()),
        }
    }

    fn eval_binary(&self, lhs: Value, op: BinaryOp, rhs: Value) -> Result<Value, String> {
        match (lhs, op, rhs) {
            (Value::Array(a), BinaryOp::Index, Value::Int(i)) => {
                let arr = a.read().unwrap();
                let idx = if i < 0 { arr.len() as i64 + i } else { i };
                if idx < 0 || idx >= arr.len() as i64 {
                    Err(format!(
                        "Array index {} out of bounds (length {})",
                        i,
                        arr.len()
                    ))
                } else {
                    Ok(arr[idx as usize].clone())
                }
            }
            (Value::Hash(h), BinaryOp::Index, Value::String(s)) => {
                let hash = h.read().unwrap();
                Ok(hash.get(&s).cloned().unwrap_or(Value::Nil))
            }
            (l, BinaryOp::Eq, r) => Ok(Value::Bool(l == r)),
            (l, BinaryOp::NotEq, r) => Ok(Value::Bool(l != r)),
            (Value::Int(l), BinaryOp::Lt, Value::Int(r)) => Ok(Value::Bool(l < r)),
            (Value::Int(l), BinaryOp::LtEq, Value::Int(r)) => Ok(Value::Bool(l <= r)),
            (Value::Int(l), BinaryOp::Gt, Value::Int(r)) => Ok(Value::Bool(l > r)),
            (Value::Int(l), BinaryOp::GtEq, Value::Int(r)) => Ok(Value::Bool(l >= r)),
            (Value::Int(l), BinaryOp::Add, Value::Int(r)) => Ok(Value::Int(l + r)),
            (Value::Int(l), BinaryOp::Sub, Value::Int(r)) => Ok(Value::Int(l - r)),
            (Value::Int(l), BinaryOp::Mul, Value::Int(r)) => Ok(Value::Int(l * r)),
            (Value::Int(l), BinaryOp::Div, Value::Int(r)) => {
                if r == 0 {
                    return Err("Division by zero".to_string());
                }
                let res = if (l < 0) != (r < 0) && l % r != 0 {
                    (l / r) - 1
                } else {
                    l / r
                };
                Ok(Value::Int(res))
            }
            (Value::Int(l), BinaryOp::Modulo, Value::Int(r)) => {
                if r == 0 {
                    return Err("Modulo by zero".to_string());
                }
                Ok(Value::Int(l % r))
            }
            (Value::Float(l), BinaryOp::Add, Value::Float(r)) => Ok(Value::Float(l + r)),
            (Value::Float(l), BinaryOp::Add, Value::Int(r)) => Ok(Value::Float(l + r as f64)),
            (Value::Int(l), BinaryOp::Add, Value::Float(r)) => Ok(Value::Float(l as f64 + r)),
            (Value::Float(l), BinaryOp::Sub, Value::Float(r)) => Ok(Value::Float(l - r)),
            (Value::Float(l), BinaryOp::Sub, Value::Int(r)) => Ok(Value::Float(l - r as f64)),
            (Value::Int(l), BinaryOp::Sub, Value::Float(r)) => Ok(Value::Float(l as f64 - r)),
            (Value::Float(l), BinaryOp::Mul, Value::Float(r)) => Ok(Value::Float(l * r)),
            (Value::Float(l), BinaryOp::Mul, Value::Int(r)) => Ok(Value::Float(l * r as f64)),
            (Value::Int(l), BinaryOp::Mul, Value::Float(r)) => Ok(Value::Float(l as f64 * r)),
            (Value::Float(l), BinaryOp::Div, Value::Int(r)) => Ok(Value::Float(l / r as f64)),
            (Value::Int(l), BinaryOp::Div, Value::Float(r)) => Ok(Value::Float(l as f64 / r)),
            (Value::Float(l), BinaryOp::Div, Value::Float(r)) => Ok(Value::Float(l / r)),

            (Value::String(l), BinaryOp::Add, r) => {
                Ok(Value::String(format!("{}{}", l, r.to_string())))
            }
            (l, BinaryOp::Add, Value::String(r)) => {
                Ok(Value::String(format!("{}{}", l.to_string(), r)))
            }

            (Value::Array(al), BinaryOp::Add, Value::Array(ar)) => {
                let l = al.read().unwrap();
                let r = ar.read().unwrap();
                let mut new_arr = l.clone();
                new_arr.extend(r.iter().cloned());
                Ok(Value::new_array(new_arr))
            }

            (Value::Int(start), BinaryOp::Range, Value::Int(end)) => {
                let arr: Vec<Value> = (start..=end).map(Value::Int).collect();
                Ok(Value::new_array(arr))
            }

            (
                Value::Money {
                    cents: l,
                    currency: cl,
                },
                BinaryOp::Add,
                Value::Money {
                    cents: r,
                    currency: cr,
                },
            ) => {
                if cl != cr {
                    return Err(format!("Currency mismatch: {} vs {}", cl, cr));
                }
                Ok(Value::Money {
                    cents: l + r,
                    currency: cl,
                })
            }
            (
                Value::Money {
                    cents: l,
                    currency: cl,
                },
                BinaryOp::Sub,
                Value::Money {
                    cents: r,
                    currency: cr,
                },
            ) => {
                if cl != cr {
                    return Err(format!("Currency mismatch: {} vs {}", cl, cr));
                }
                Ok(Value::Money {
                    cents: l - r,
                    currency: cl,
                })
            }
            (
                Value::Money {
                    cents: l,
                    currency: cl,
                },
                BinaryOp::Lt,
                Value::Money {
                    cents: r,
                    currency: cr,
                },
            ) => {
                if cl != cr {
                    return Err(format!("Currency mismatch: {} vs {}", cl, cr));
                }
                Ok(Value::Bool(l < r))
            }
            (
                Value::Money {
                    cents: l,
                    currency: cl,
                },
                BinaryOp::LtEq,
                Value::Money {
                    cents: r,
                    currency: cr,
                },
            ) => {
                if cl != cr {
                    return Err(format!("Currency mismatch: {} vs {}", cl, cr));
                }
                Ok(Value::Bool(l <= r))
            }
            (
                Value::Money {
                    cents: l,
                    currency: cl,
                },
                BinaryOp::Gt,
                Value::Money {
                    cents: r,
                    currency: cr,
                },
            ) => {
                if cl != cr {
                    return Err(format!("Currency mismatch: {} vs {}", cl, cr));
                }
                Ok(Value::Bool(l > r))
            }
            (
                Value::Money {
                    cents: l,
                    currency: cl,
                },
                BinaryOp::GtEq,
                Value::Money {
                    cents: r,
                    currency: cr,
                },
            ) => {
                if cl != cr {
                    return Err(format!("Currency mismatch: {} vs {}", cl, cr));
                }
                Ok(Value::Bool(l >= r))
            }

            (Value::Duration(l), BinaryOp::Add, Value::Duration(r)) => Ok(Value::Duration(l + r)),
            (Value::Duration(l), BinaryOp::Sub, Value::Duration(r)) => Ok(Value::Duration(l - r)),

            _ => Err("Binary operation not supported".to_string()),
        }
    }
}
