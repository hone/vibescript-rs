use crate::ast::*;
use crate::value::{ClassDef, FunctionDef, InstanceData, Param, Value};
use chrono::Utc;
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
        Self {
            globals: HashMap::new(),
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
            Expr::Variable(name) => self
                .get_var(name)
                .ok_or_else(|| EvalError::Message(format!("Variable '{}' not found", name))),
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
                kwargs: _,
                block,
            } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(arg)?);
                }

                match func.as_str() {
                    "money" => {
                        if let Some(Value::String(s)) = arg_vals.first() {
                            return Ok(Value::String(format!("{} (money)", s)));
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
                block,
            } => {
                let recv_val = self.eval_expr(receiver)?;
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(arg)?);
                }
                self.eval_member(recv_val, method, arg_vals, block.as_deref())
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
        block: Option<&Expr>,
    ) -> Result<Value, EvalError> {
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

            (Value::String(s), "uppercase", _) => Ok(Value::String(s.to_uppercase())),
            (Value::String(s), "lowercase", _) => Ok(Value::String(s.to_lowercase())),
            (Value::String(s), "contains?" | "include?", _) => {
                if let Some(Value::String(sub)) = args.first() {
                    Ok(Value::Bool(s.contains(sub)))
                } else {
                    Ok(Value::Bool(false))
                }
            }
            (Value::String(s), "strip", _) => Ok(Value::String(s.trim().to_string())),

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

            _ => Err("Binary operation not supported".to_string()),
        }
    }
}
