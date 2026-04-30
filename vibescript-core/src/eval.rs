use crate::ast::*;
use crate::value::{ClassDef, FunctionDef, InstanceData, Param, Value};
use chrono::{DateTime, Datelike, NaiveDateTime, Timelike, Utc};
use rand::{Rng, distributions::Alphanumeric};
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub trait ModuleResolver: Send + Sync {
    fn load_module(
        &self,
        name: &str,
        caller_path: Option<&str>,
    ) -> Result<(String, String), String>;
}

pub trait Compiler: Send + Sync {
    fn compile(&self, source: &str) -> Result<Vec<Stmt>, String>;
}

pub struct Engine {
    globals: HashMap<String, Value>,
    stack: Vec<HashMap<String, Value>>,
    pub functions: HashMap<String, FunctionDef>,
    pub classes: HashMap<String, Arc<ClassDef>>,
    pub enums: HashMap<String, Stmt>, // Store Stmt::EnumDef variants
    class_vars: HashMap<String, Value>,
    recursion_depth: usize,
    pub modules: HashMap<String, Value>,
    pub loading_modules: Vec<String>,
    pub module_resolver: Option<Arc<dyn ModuleResolver>>,
    pub compiler: Option<Arc<dyn Compiler>>,
    pub current_module_path: Option<String>,
    pub current_block: Option<Value>,
}

const MAX_RECURSION_DEPTH: usize = 500;

#[derive(Debug, Clone)]
pub struct EvalError {
    pub kind: String,
    pub message: String,
}

impl EvalError {
    pub fn new(kind: &str, message: &str) -> Self {
        Self {
            kind: kind.to_string(),
            message: message.to_string(),
        }
    }
}

impl From<String> for EvalError {
    fn from(msg: String) -> Self {
        EvalError::new("RuntimeError", &msg)
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
            enums: HashMap::new(),
            class_vars: HashMap::new(),
            recursion_depth: 0,
            modules: HashMap::new(),
            loading_modules: Vec::new(),
            module_resolver: None,
            compiler: None,
            current_module_path: None,
            current_block: None,
        }
    }

    pub fn eval_script(&mut self, stmts: &[Stmt]) -> Result<Value, EvalError> {
        let mut last_val = Value::Nil;
        for stmt in stmts {
            match self.eval_stmt(stmt)? {
                ControlFlow::Continue(v) => last_val = v,
                ControlFlow::Return(v) => return Ok(v),
                ControlFlow::Break(v) => return Ok(v),
                ControlFlow::Next(v) => last_val = v,
            }
        }
        Ok(last_val)
    }

    fn flatten_array(&self, arr: Vec<Value>, depth: i64) -> Vec<Value> {
        let mut result = Vec::new();
        for val in arr {
            if let Value::Array(inner_lock) = val {
                if depth != 0 {
                    let inner = inner_lock.read().unwrap();
                    let flattened =
                        self.flatten_array(inner.clone(), if depth > 0 { depth - 1 } else { -1 });
                    result.extend(flattened);
                } else {
                    result.push(Value::Array(inner_lock));
                }
            } else {
                result.push(val);
            }
        }
        result
    }

    fn deep_transform_hash_keys(
        &mut self,
        hash: &HashMap<String, Value>,
        params: &[String],
        body: &[Stmt],
    ) -> Result<HashMap<String, Value>, EvalError> {
        let mut new_map = HashMap::new();
        let keys: Vec<String> = hash.keys().cloned().collect();
        for k in keys {
            let mut val = hash.get(&k).unwrap().clone();
            if let Value::Hash(inner_lock) = val {
                let inner = inner_lock.read().unwrap();
                let transformed = self.deep_transform_hash_keys(&inner, params, body)?;
                val = Value::new_hash(transformed);
            }

            let mut scope = HashMap::new();
            if let Some(p) = params.first() {
                scope.insert(p.clone(), Value::Symbol(k));
            }
            self.stack.push(scope);
            let res = self.eval_block(body)?;
            self.stack.pop();

            let new_key = match res.value() {
                Value::String(s) | Value::Symbol(s) => s,
                v => v.to_string(),
            };
            new_map.insert(new_key, val);
        }
        Ok(new_map)
    }

    fn builtin_require(
        &mut self,
        name: &str,
        kwargs: &Option<HashMap<String, Value>>,
    ) -> Result<Value, EvalError> {
        let resolver = self
            .module_resolver
            .clone()
            .ok_or_else(|| EvalError::new("RuntimeError", "Module resolver not configured"))?;
        let compiler = self
            .compiler
            .clone()
            .ok_or_else(|| EvalError::new("RuntimeError", "Compiler not configured"))?;

        let (source, resolved_path) = resolver
            .load_module(name, self.current_module_path.as_deref())
            .map_err(|e| EvalError::new("RuntimeError", &e))?;

        if let Some(cached) = self.modules.get(&resolved_path) {
            return Ok(cached.clone());
        }

        if self.loading_modules.contains(&resolved_path) {
            return Err(EvalError::new(
                "RuntimeError",
                &format!("Circular dependency detected: {}", resolved_path),
            ));
        }

        self.loading_modules.push(resolved_path.clone());

        let stmts = compiler
            .compile(&source)
            .map_err(|e| EvalError::new("RuntimeError", &e))?;

        let mut mod_engine = Engine::new();
        mod_engine.module_resolver = self.module_resolver.clone();
        mod_engine.compiler = self.compiler.clone();
        mod_engine.current_module_path = Some(resolved_path.clone());
        mod_engine.modules = self.modules.clone();
        mod_engine.loading_modules = self.loading_modules.clone();

        mod_engine.eval_script(&stmts)?;

        // Sync module cache back
        for (k, v) in &mod_engine.modules {
            self.modules.insert(k.clone(), v.clone());
        }

        // Extract exports: Enums and non-private functions
        let exports = Arc::new(RwLock::new(HashMap::new()));
        {
            let mut exports_map = exports.write().unwrap();

            // 1. Enums
            for name in mod_engine.enums.keys() {
                let enum_val = Value::Namespace(name.clone()); // Simplified for now
                exports_map.insert(name.clone(), enum_val.clone());
                // Inject globally if not exists
                if !self.globals.contains_key(name) {
                    self.globals.insert(name.clone(), enum_val);
                }
            }

            // 2. Functions (non-private or explicitly exported)
            for (name, func) in &mod_engine.functions {
                if !func.is_private || func.is_exported {
                    let fn_val = Value::Function(Arc::new(func.clone()));
                    exports_map.insert(name.clone(), fn_val.clone());

                    // Inject globally if not exists
                    if !self.globals.contains_key(name) {
                        self.globals.insert(name.clone(), fn_val);
                    }
                }
            }
        }

        let exports_val = Value::Object(exports);
        self.modules
            .insert(resolved_path.clone(), exports_val.clone());
        self.loading_modules.pop();

        // Handle alias if 'as' kwarg is provided
        if let Some(kwargs) = kwargs {
            if let Some(Value::String(alias)) = kwargs.get("as") {
                self.globals.insert(alias.clone(), exports_val.clone());
            } else if let Some(Value::Symbol(alias)) = kwargs.get("as") {
                self.globals.insert(alias.clone(), exports_val.clone());
            }
        }

        Ok(exports_val)
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
                        ControlFlow::Next(_) => continue,
                        ControlFlow::Continue(v) => last_val = v,
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
                        ControlFlow::Next(_) => continue,
                        ControlFlow::Continue(v) => last_val = v,
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
                            ControlFlow::Next(_) => continue,
                            ControlFlow::Continue(v) => last_val = v,
                        }
                    }
                }
                Ok(ControlFlow::Continue(last_val))
            }
            Stmt::Function(f) => {
                let def = FunctionDef {
                    params: f.params.clone(),
                    return_type: f.return_type.clone(),
                    body: f.body.clone(),
                    is_private: f.is_private,
                    is_exported: f.is_exported,
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
            Stmt::Raise(expr) => {
                let val = self.eval_expr(expr)?;
                Err(EvalError::new("RuntimeError", &val.to_string()))
            }
            Stmt::EnumDef { name, members } => {
                self.enums.insert(name.clone(), stmt.clone());
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
            Stmt::PropertyDecl { names, kind: _ } => {
                if let Some(Value::Class(c)) = self.get_var("self") {
                    for name in names {
                        let getter_body = vec![Stmt::Return(Some(Expr::InstanceVar(name.clone())))];
                        c.methods.write().unwrap().insert(
                            name.clone(),
                            FunctionDef {
                                params: vec![],
                                return_type: None,
                                body: getter_body,
                                is_private: false,
                                is_exported: false,
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
                                    param_type: None,
                                }],
                                return_type: None,
                                body: setter_body,
                                is_private: false,
                                is_exported: false,
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
                    return Err(EvalError::new("AssertionError", &msg));
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
                    Err(e) => {
                        if let Some(r) = rescue {
                            let mut matches = r.types.is_empty();
                            if !matches {
                                for ty in &r.types {
                                    if ty.name.to_lowercase() == e.kind.to_lowercase()
                                        || (ty.name == "Error" && e.kind == "RuntimeError")
                                    {
                                        matches = true;
                                        break;
                                    }
                                }
                            }

                            if matches {
                                self.eval_block(&r.body)
                            } else {
                                Err(e)
                            }
                        } else {
                            Err(e)
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
                self.get_var(name).ok_or_else(|| {
                    EvalError::new("RuntimeError", &format!("Variable '{}' not found", name))
                })
            }
            Expr::InstanceVar(name) => self
                .get_ivar(name)
                .map_err(|e| EvalError::new("RuntimeError", &e)),
            Expr::ClassVar(name) => self
                .get_cvar(name)
                .map_err(|e| EvalError::new("RuntimeError", &e)),
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
                self.eval_binary(lhs, *op, rhs)
                    .map_err(|e| EvalError::new("RuntimeError", &e))
            }
            Expr::Unary { op, expr } => {
                let val = self.eval_expr(expr)?;
                self.eval_unary(*op, &val)
                    .map_err(|e| EvalError::new("RuntimeError", &e))
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
                                EvalError::new("RuntimeError", &format!("JSON parse error: {}", e))
                            })?;
                            return Ok(self.json_to_vibe(v));
                        }
                    }
                    "uuid" => return Ok(Value::String(Uuid::new_v4().to_string())),
                    "now" => return Ok(Value::Time(Utc::now())),
                    "to_int" => {
                        if let Some(val) = arg_vals.first() {
                            match val {
                                Value::Int(i) => return Ok(Value::Int(*i)),
                                Value::Float(f) => return Ok(Value::Int(*f as i64)),
                                Value::String(s) => {
                                    if let Ok(i) = s.parse::<i64>() {
                                        return Ok(Value::Int(i));
                                    }
                                }
                                _ => {}
                            }
                        }
                        return Ok(Value::Nil);
                    }
                    "to_float" => {
                        if let Some(val) = arg_vals.first() {
                            match val {
                                Value::Int(i) => return Ok(Value::Float(*i as f64)),
                                Value::Float(f) => return Ok(Value::Float(*f)),
                                Value::String(s) => {
                                    if let Ok(f) = s.parse::<f64>() {
                                        return Ok(Value::Float(f));
                                    }
                                }
                                _ => {}
                            }
                        }
                        return Ok(Value::Nil);
                    }
                    "random_id" => {
                        let len = arg_vals.first().and_then(|v| v.as_int()).unwrap_or(16);
                        if len <= 0 {
                            return Err(EvalError::new(
                                "RuntimeError",
                                "random_id length must be positive",
                            ));
                        }
                        if len > 1024 {
                            return Err(EvalError::new(
                                "RuntimeError",
                                "random_id length exceeds maximum 1024",
                            ));
                        }
                        let s: String = rand::thread_rng()
                            .sample_iter(&Alphanumeric)
                            .take(len as usize)
                            .map(char::from)
                            .collect();
                        return Ok(Value::String(s));
                    }
                    "require" => {
                        if let Some(Value::String(name)) = arg_vals.first() {
                            return self.builtin_require(name, &Some(kwarg_vals));
                        } else {
                            return Err(EvalError::new(
                                "RuntimeError",
                                "require expects a string argument",
                            ));
                        }
                    }
                    _ => {}
                }

                if let Some(val) = self.get_var(func) {
                    if let Value::Function(f) = val {
                        return self.call_function_def(&f, arg_vals, block.as_deref());
                    }
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

                Err(EvalError::new(
                    "RuntimeError",
                    &format!("Function '{}' not found", func),
                ))
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
            Expr::BlockExpr(stmt) => self.eval_stmt(stmt).map(|cf| cf.value()),
            Expr::Yield { args } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(arg)?);
                }
                let block_val = self.current_block.clone().ok_or_else(|| {
                    EvalError::new(
                        "RuntimeError",
                        "yield used outside of block-receiving function",
                    )
                })?;
                if let Value::Block { params, body } = block_val {
                    let mut new_scope = HashMap::new();
                    for (param, val) in params.iter().zip(arg_vals) {
                        new_scope.insert(param.clone(), val);
                    }
                    self.stack.push(new_scope);
                    let cf = self.eval_block(&body)?;
                    self.stack.pop();
                    Ok(cf.value())
                } else {
                    Err(EvalError::new(
                        "RuntimeError",
                        "yield target is not a block",
                    ))
                }
            }
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
            Expr::If {
                condition,
                then_branch,
                elsif_branches,
                else_branch,
            } => {
                let cond = self.eval_expr(condition)?;
                if self.is_truthy(&cond) {
                    self.eval_block(then_branch).map(|cf| cf.value())
                } else {
                    for (elsif_cond, elsif_body) in elsif_branches {
                        let cond = self.eval_expr(elsif_cond)?;
                        if self.is_truthy(&cond) {
                            return self.eval_block(elsif_body).map(|cf| cf.value());
                        }
                    }
                    if let Some(else_body) = else_branch {
                        self.eval_block(else_body).map(|cf| cf.value())
                    } else {
                        Ok(Value::Nil)
                    }
                }
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
        for scope in self.stack.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), val);
                return;
            }
        }
        if self.globals.contains_key(name) {
            self.globals.insert(name.to_string(), val);
            return;
        }
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
            Err(EvalError::new(
                "RuntimeError",
                &format!("Cannot set instance variable '{}' outside instance", name),
            ))
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
                        let v: serde_json::Value = serde_json::from_str(s).map_err(|e| {
                            EvalError::new("RuntimeError", &format!("JSON parse error: {}", e))
                        })?;
                        return Ok(self.json_to_vibe(v));
                    }
                }
                ("JSON", "stringify") => {
                    if let Some(v) = args.first() {
                        let json = self.vibe_to_json(v.clone());
                        let s = serde_json::to_string(&json).map_err(|e| {
                            EvalError::new("RuntimeError", &format!("JSON stringify error: {}", e))
                        })?;
                        return Ok(Value::String(s));
                    }
                }
                ("Time", "now") => {
                    return Ok(Value::Time(Utc::now()));
                }
                ("Time", "parse") => {
                    if let Some(Value::String(s)) = args.first() {
                        let t = DateTime::parse_from_rfc3339(s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .or_else(|_| {
                                DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S %z")
                                    .map(|dt| dt.with_timezone(&Utc))
                            })
                            .or_else(|_| {
                                NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                                    .map(|ndt| ndt.and_utc())
                            })
                            .or_else(|_| {
                                NaiveDateTime::parse_from_str(s, "%Y-%m-%d").map(|ndt| {
                                    ndt.and_local_timezone(Utc)
                                        .latest()
                                        .unwrap_or_else(|| ndt.and_utc())
                                })
                            })
                            .map_err(|e| {
                                EvalError::new("RuntimeError", &format!("Time parse error: {}", e))
                            })?;
                        return Ok(Value::Time(t));
                    }
                }
                ("Regex", "match") => {
                    let pattern = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                    let s = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                    let re = Regex::new(pattern)
                        .map_err(|e| EvalError::new("RuntimeError", &e.to_string()))?;
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
                    let re = Regex::new(pattern)
                        .map_err(|e| EvalError::new("RuntimeError", &e.to_string()))?;
                    return Ok(Value::String(re.replace(s, replacement).to_string()));
                }
                ("Regex", "replace_all") => {
                    let s = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                    let pattern = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                    let replacement = args.get(2).and_then(|v| v.as_str()).unwrap_or("");
                    let re = Regex::new(pattern)
                        .map_err(|e| EvalError::new("RuntimeError", &e.to_string()))?;
                    return Ok(Value::String(re.replace_all(s, replacement).to_string()));
                }
                _ => {
                    return Err(EvalError::new(
                        "RuntimeError",
                        &format!("Method '{}' not found for namespace '{}'", method, ns),
                    ));
                }
            }
        }

        if method == "to_string" {
            return Ok(Value::String(receiver.to_string()));
        }

        match (receiver.clone(), method, block) {
            (Value::Object(o), method, _) => {
                let obj = o.read().unwrap();
                if let Some(val) = obj.get(method).cloned() {
                    if let Value::Function(f) = val {
                        return self
                            .call_function_def_cf(&f, args, block)
                            .map(|cf| cf.value());
                    }
                    return Ok(val);
                } else {
                    return Err(EvalError::new(
                        "RuntimeError",
                        &format!("Member '{}' not found in Object", method),
                    ));
                }
            }
            (Value::Hash(h), method, block) => {
                let hash = h.read().unwrap().clone();
                if let Some(val) = hash.get(method).cloned() {
                    if let Value::Function(f) = val {
                        return self
                            .call_function_def_cf(&f, args, block)
                            .map(|cf| cf.value());
                    }
                    return Ok(val);
                }
                match method {
                    "length" | "size" => Ok(Value::Int(hash.len() as i64)),
                    "empty?" => Ok(Value::Bool(hash.is_empty())),
                    "key?" | "has_key?" | "include?" => {
                        if let Some(arg) = args.first() {
                            let key = match arg {
                                Value::String(s) | Value::Symbol(s) => s.clone(),
                                _ => return Ok(Value::Bool(false)),
                            };
                            Ok(Value::Bool(hash.contains_key(&key)))
                        } else {
                            Ok(Value::Bool(false))
                        }
                    }
                    "fetch" => {
                        if let Some(arg) = args.first() {
                            let key = match arg {
                                Value::String(s) | Value::Symbol(s) => s.clone(),
                                _ => {
                                    return Err(EvalError::new(
                                        "TypeError",
                                        "Hash.fetch key must be string or symbol",
                                    ));
                                }
                            };
                            if let Some(val) = hash.get(&key) {
                                return Ok(val.clone());
                            }
                            if args.len() == 2 {
                                return Ok(args[1].clone());
                            }
                            Ok(Value::Nil)
                        } else {
                            Err(EvalError::new("ArgumentError", "Hash.fetch expects a key"))
                        }
                    }
                    "keys" => {
                        let mut keys: Vec<String> = hash.keys().cloned().collect();
                        keys.sort();
                        let vals = keys.into_iter().map(Value::Symbol).collect();
                        Ok(Value::new_array(vals))
                    }
                    "values" => {
                        let mut keys: Vec<String> = hash.keys().cloned().collect();
                        keys.sort();
                        let vals = keys
                            .into_iter()
                            .map(|k| hash.get(&k).unwrap().clone())
                            .collect();
                        Ok(Value::new_array(vals))
                    }
                    "compact" => {
                        let mut new_map = HashMap::new();
                        for (k, v) in hash.iter() {
                            if !v.is_nil() {
                                new_map.insert(k.clone(), v.clone());
                            }
                        }
                        Ok(Value::new_hash(new_map))
                    }
                    "each_key" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let keys: Vec<String> = hash.keys().cloned().collect();
                            for k in keys {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), Value::Symbol(k));
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
                            Ok(Value::Hash(h.clone()))
                        } else {
                            Err(EvalError::new("ArgumentError", "each_key requires a block"))
                        }
                    }
                    "each_value" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let values: Vec<Value> = hash.values().cloned().collect();
                            for v in values {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), v);
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
                            Ok(Value::Hash(h.clone()))
                        } else {
                            Err(EvalError::new(
                                "ArgumentError",
                                "each_value requires a block",
                            ))
                        }
                    }
                    "deep_transform_keys" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let res = self.deep_transform_hash_keys(&hash, params, body)?;
                            Ok(Value::new_hash(res))
                        } else {
                            Err(EvalError::new(
                                "ArgumentError",
                                "deep_transform_keys requires a block",
                            ))
                        }
                    }
                    "remap_keys" => {
                        if let Some(Value::Hash(mapping_lock)) = args.first() {
                            let mapping = mapping_lock.read().unwrap();
                            let mut new_map = HashMap::new();
                            for (k, v) in hash.iter() {
                                if let Some(new_key_val) = mapping.get(k) {
                                    let new_key = match new_key_val {
                                        Value::String(s) | Value::Symbol(s) => s.clone(),
                                        _ => {
                                            return Err(EvalError::new(
                                                "TypeError",
                                                "remap_keys mapping values must be symbol or string",
                                            ));
                                        }
                                    };
                                    new_map.insert(new_key, v.clone());
                                } else {
                                    new_map.insert(k.clone(), v.clone());
                                }
                            }
                            Ok(Value::new_hash(new_map))
                        } else {
                            Err(EvalError::new(
                                "ArgumentError",
                                "remap_keys expects a mapping hash",
                            ))
                        }
                    }
                    "merge" => {
                        if let Some(other_val) = args.first() {
                            if let Some(other_hash_lock) = other_val.as_hash() {
                                let other_hash = other_hash_lock.read().unwrap();
                                let mut new_map = hash.clone();
                                for (k, v) in other_hash.iter() {
                                    new_map.insert(k.clone(), v.clone());
                                }
                                return Ok(Value::new_hash(new_map));
                            }
                        }
                        Err(EvalError::new(
                            "TypeError",
                            "Hash.merge expects another Hash",
                        ))
                    }
                    "dig" => {
                        let mut current_dig = receiver.clone();
                        for arg in args {
                            let key = match arg {
                                Value::String(s) | Value::Symbol(s) => s.clone(),
                                _ => {
                                    return Err(EvalError::new(
                                        "TypeError",
                                        "dig keys must be symbols or strings",
                                    ));
                                }
                            };
                            match current_dig {
                                Value::Hash(h_lock) | Value::Object(h_lock) => {
                                    let next = h_lock.read().unwrap().get(&key).cloned();
                                    if let Some(val) = next {
                                        current_dig = val;
                                    } else {
                                        return Ok(Value::Nil);
                                    }
                                }
                                _ => return Ok(Value::Nil),
                            }
                        }
                        Ok(current_dig)
                    }
                    "slice" => {
                        let mut new_map = HashMap::new();
                        for arg in args {
                            let key = match arg {
                                Value::String(s) | Value::Symbol(s) => s.clone(),
                                _ => continue,
                            };
                            if let Some(val) = hash.get(&key).cloned() {
                                new_map.insert(key, val);
                            }
                        }
                        Ok(Value::new_hash(new_map))
                    }
                    "except" => {
                        let mut new_map = hash.clone();
                        for arg in args {
                            if let Some(s) = arg.as_str() {
                                new_map.remove(s);
                            } else if let Value::Symbol(s) = arg {
                                new_map.remove(&s);
                            }
                        }
                        Ok(Value::new_hash(new_map))
                    }
                    "select" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let mut new_map = HashMap::new();
                            let mut keys: Vec<String> = hash.keys().cloned().collect();
                            keys.sort();
                            for k in keys {
                                let v = hash.get(&k).unwrap().clone();
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), Value::Symbol(k.clone()));
                                }
                                if let Some(p) = params.get(1) {
                                    scope.insert(p.clone(), v.clone());
                                }
                                self.stack.push(scope);
                                let res = self.eval_block(body)?;
                                self.stack.pop();
                                if self.is_truthy(&res.value()) {
                                    new_map.insert(k, v);
                                }
                            }
                            Ok(Value::new_hash(new_map))
                        } else {
                            Err(EvalError::new("ArgumentError", "select requires a block"))
                        }
                    }
                    "reject" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let mut new_map = HashMap::new();
                            let mut keys: Vec<String> = hash.keys().cloned().collect();
                            keys.sort();
                            for k in keys {
                                let v = hash.get(&k).unwrap().clone();
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), Value::Symbol(k.clone()));
                                }
                                if let Some(p) = params.get(1) {
                                    scope.insert(p.clone(), v.clone());
                                }
                                self.stack.push(scope);
                                let res = self.eval_block(body)?;
                                self.stack.pop();
                                if !self.is_truthy(&res.value()) {
                                    new_map.insert(k, v);
                                }
                            }
                            Ok(Value::new_hash(new_map))
                        } else {
                            Err(EvalError::new("ArgumentError", "reject requires a block"))
                        }
                    }
                    "transform_keys" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let mut new_map = HashMap::new();
                            let mut keys: Vec<String> = hash.keys().cloned().collect();
                            keys.sort();
                            for k in keys {
                                let v = hash.get(&k).unwrap().clone();
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), Value::Symbol(k.clone()));
                                }
                                self.stack.push(scope);
                                let res = self.eval_block(body)?;
                                self.stack.pop();
                                let new_key = match res.value() {
                                    Value::String(s) | Value::Symbol(s) => s,
                                    v => v.to_string(),
                                };
                                new_map.insert(new_key, v);
                            }
                            Ok(Value::new_hash(new_map))
                        } else {
                            Err(EvalError::new(
                                "ArgumentError",
                                "transform_keys requires a block",
                            ))
                        }
                    }
                    "transform_values" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let mut new_map = HashMap::new();
                            let mut keys: Vec<String> = hash.keys().cloned().collect();
                            keys.sort();
                            for k in keys {
                                let v = hash.get(&k).unwrap().clone();
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), v);
                                }
                                self.stack.push(scope);
                                let res = self.eval_block(body)?;
                                self.stack.pop();
                                new_map.insert(k, res.value());
                            }
                            Ok(Value::new_hash(new_map))
                        } else {
                            Err(EvalError::new(
                                "ArgumentError",
                                "transform_values requires a block",
                            ))
                        }
                    }
                    "[]=" => {
                        if args.len() < 2 {
                            return Err(EvalError::new(
                                "ArgumentError",
                                "[]= expects key and value",
                            ));
                        }
                        if let Value::String(s) = &args[0] {
                            h.write().unwrap().insert(s.clone(), args[1].clone());
                            Ok(args[1].clone())
                        } else {
                            Err(EvalError::new("TypeError", "Hash key must be string"))
                        }
                    }
                    _ => Err(EvalError::new(
                        "RuntimeError",
                        &format!(
                            "Method '{}' not supported for Hash and key not found",
                            method
                        ),
                    )),
                }
            }
            (Value::Array(a), method, block) => {
                let arr_snapshot = a.read().unwrap().clone();
                match method {
                    "length" | "size" => Ok(Value::Int(arr_snapshot.len() as i64)),
                    "empty?" => Ok(Value::Bool(arr_snapshot.is_empty())),
                    "include?" | "contains?" => {
                        if let Some(target) = args.first() {
                            Ok(Value::Bool(arr_snapshot.contains(target)))
                        } else {
                            Ok(Value::Bool(false))
                        }
                    }
                    "fetch" => {
                        if let Some(idx) = args.first().and_then(|v| v.as_int()) {
                            let i = idx as usize;
                            if i < arr_snapshot.len() {
                                return Ok(arr_snapshot[i].clone());
                            }
                            if args.len() == 2 {
                                return Ok(args[1].clone());
                            }
                            Ok(Value::Nil)
                        } else {
                            Err(EvalError::new(
                                "ArgumentError",
                                "Array.fetch expects an integer index",
                            ))
                        }
                    }
                    "index" => {
                        if let Some(target) = args.first() {
                            let offset = args.get(1).and_then(|v| v.as_int()).unwrap_or(0) as usize;
                            if offset >= arr_snapshot.len() {
                                return Ok(Value::Nil);
                            }
                            if let Some(pos) =
                                arr_snapshot.iter().skip(offset).position(|v| v == target)
                            {
                                return Ok(Value::Int((offset + pos) as i64));
                            }
                        }
                        Ok(Value::Nil)
                    }
                    "rindex" => {
                        if let Some(target) = args.first() {
                            let offset = args
                                .get(1)
                                .and_then(|v| v.as_int())
                                .unwrap_or(arr_snapshot.len() as i64 - 1)
                                as isize;
                            if offset < 0 {
                                return Ok(Value::Nil);
                            }
                            let limit = (offset as usize).min(arr_snapshot.len().saturating_sub(1));
                            if let Some(pos) =
                                arr_snapshot[..=limit].iter().rposition(|v| v == target)
                            {
                                return Ok(Value::Int(pos as i64));
                            }
                        }
                        Ok(Value::Nil)
                    }
                    "find" => {
                        if let Some(Expr::Block { params, body }) = block {
                            for val in arr_snapshot.iter() {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), val.clone());
                                }
                                self.stack.push(scope);
                                let cf = self.eval_block(body)?;
                                self.stack.pop();
                                if self.is_truthy(&cf.value()) {
                                    return Ok(val.clone());
                                }
                            }
                            Ok(Value::Nil)
                        } else {
                            Err(EvalError::new("ArgumentError", "find requires a block"))
                        }
                    }
                    "find_index" => {
                        if let Some(Expr::Block { params, body }) = block {
                            for (idx, val) in arr_snapshot.iter().enumerate() {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), val.clone());
                                }
                                self.stack.push(scope);
                                let cf = self.eval_block(body)?;
                                self.stack.pop();
                                if self.is_truthy(&cf.value()) {
                                    return Ok(Value::Int(idx as i64));
                                }
                            }
                            Ok(Value::Nil)
                        } else {
                            Err(EvalError::new(
                                "ArgumentError",
                                "find_index requires a block",
                            ))
                        }
                    }
                    "push" => {
                        let mut new_vec = arr_snapshot.clone();
                        for arg in args {
                            new_vec.push(arg);
                        }
                        Ok(Value::new_array(new_vec))
                    }
                    "pop" => {
                        let mut new_vec = arr_snapshot.clone();
                        let count_val = args.first().and_then(|v| v.as_int()).unwrap_or(1);
                        let count = if count_val < 0 { 0 } else { count_val as usize };
                        let mut removed = Vec::new();
                        for _ in 0..count {
                            if let Some(val) = new_vec.pop() {
                                removed.push(val);
                            } else {
                                break;
                            }
                        }
                        let mut result = HashMap::new();
                        result.insert("array".to_string(), Value::new_array(new_vec));
                        if args.is_empty() {
                            result.insert(
                                "popped".to_string(),
                                removed.first().cloned().unwrap_or(Value::Nil),
                            );
                        } else {
                            removed.reverse();
                            result.insert("popped".to_string(), Value::new_array(removed));
                        }
                        Ok(Value::new_hash(result))
                    }
                    "first" => {
                        if let Some(count_val) = args.first().and_then(|v| v.as_int()) {
                            let n = count_val as usize;
                            let sub: Vec<Value> = arr_snapshot.iter().take(n).cloned().collect();
                            Ok(Value::new_array(sub))
                        } else {
                            Ok(arr_snapshot.first().cloned().unwrap_or(Value::Nil))
                        }
                    }
                    "last" => {
                        if let Some(count_val) = args.first().and_then(|v| v.as_int()) {
                            let n = count_val as usize;
                            let start = if arr_snapshot.len() > n {
                                arr_snapshot.len() - n
                            } else {
                                0
                            };
                            let sub: Vec<Value> =
                                arr_snapshot.iter().skip(start).cloned().collect();
                            Ok(Value::new_array(sub))
                        } else {
                            Ok(arr_snapshot.last().cloned().unwrap_or(Value::Nil))
                        }
                    }
                    "reverse" => {
                        let rev: Vec<Value> = arr_snapshot.iter().rev().cloned().collect();
                        Ok(Value::new_array(rev))
                    }
                    "join" => {
                        let sep = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                        let joined = arr_snapshot
                            .iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(sep);
                        Ok(Value::String(joined))
                    }
                    "sum" => {
                        let mut sum_int = 0;
                        let mut sum_float = 0.0;
                        let mut has_float = false;
                        for v in arr_snapshot.iter() {
                            if let Some(i) = v.as_int() {
                                sum_int += i;
                                sum_float += i as f64;
                            } else if let Some(f) = v.as_float() {
                                sum_float += f;
                                has_float = true;
                            }
                        }
                        if has_float {
                            Ok(Value::Float(sum_float))
                        } else {
                            Ok(Value::Int(sum_int))
                        }
                    }
                    "compact" => {
                        let compacted: Vec<Value> = arr_snapshot
                            .iter()
                            .filter(|v| !v.is_nil())
                            .cloned()
                            .collect();
                        Ok(Value::new_array(compacted))
                    }
                    "uniq" => {
                        let mut unique: Vec<Value> = Vec::new();
                        for v in arr_snapshot.iter() {
                            if !unique.contains(v) {
                                unique.push(v.clone());
                            }
                        }
                        Ok(Value::new_array(unique))
                    }
                    "flatten" => {
                        let depth = args.first().and_then(|v| v.as_int()).unwrap_or(-1);
                        let flattened = self.flatten_array(arr_snapshot.clone(), depth);
                        Ok(Value::new_array(flattened))
                    }
                    "count" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let mut total = 0;
                            for val in arr_snapshot.iter() {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), val.clone());
                                }
                                self.stack.push(scope);
                                let cf = self.eval_block(body)?;
                                self.stack.pop();
                                if self.is_truthy(&cf.value()) {
                                    total += 1;
                                }
                            }
                            Ok(Value::Int(total))
                        } else if let Some(target) = args.first() {
                            let total = arr_snapshot.iter().filter(|v| *v == target).count();
                            Ok(Value::Int(total as i64))
                        } else {
                            Ok(Value::Int(arr_snapshot.len() as i64))
                        }
                    }
                    "chunk" => {
                        let size = args.first().and_then(|v| v.as_int()).unwrap_or(0);
                        if size <= 0 {
                            return Err(EvalError::new(
                                "RuntimeError",
                                "chunk size must be positive",
                            ));
                        }
                        let chunks: Vec<Value> = arr_snapshot
                            .chunks(size as usize)
                            .map(|c| Value::new_array(c.to_vec()))
                            .collect();
                        Ok(Value::new_array(chunks))
                    }
                    "window" => {
                        let size = args.first().and_then(|v| v.as_int()).unwrap_or(0);
                        if size <= 0 {
                            return Err(EvalError::new(
                                "RuntimeError",
                                "window size must be positive",
                            ));
                        }
                        if size as usize > arr_snapshot.len() {
                            return Ok(Value::new_array(vec![]));
                        }
                        let windows: Vec<Value> = arr_snapshot
                            .windows(size as usize)
                            .map(|w| Value::new_array(w.to_vec()))
                            .collect();
                        Ok(Value::new_array(windows))
                    }
                    "sample" => {
                        if arr_snapshot.is_empty() {
                            return Ok(Value::Nil);
                        }
                        let mut rng = rand::thread_rng();
                        if let Some(count_val) = args.first().and_then(|v| v.as_int()) {
                            let count = count_val.max(0) as usize;
                            if count == 0 {
                                return Ok(Value::new_array(vec![]));
                            }
                            let mut indices: Vec<usize> = (0..arr_snapshot.len()).collect();
                            use rand::seq::SliceRandom;
                            indices.shuffle(&mut rng);
                            let sampled: Vec<Value> = indices
                                .into_iter()
                                .take(count)
                                .map(|i| arr_snapshot[i].clone())
                                .collect();
                            Ok(Value::new_array(sampled))
                        } else {
                            use rand::Rng;
                            let idx = rng.gen_range(0..arr_snapshot.len());
                            Ok(arr_snapshot[idx].clone())
                        }
                    }
                    "each" => {
                        if let Some(Expr::Block { params, body }) = block {
                            for val in arr_snapshot.iter() {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), val.clone());
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
                            Ok(Value::Array(a.clone()))
                        } else {
                            Err(EvalError::new("ArgumentError", "each requires a block"))
                        }
                    }
                    "map" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let mut results = Vec::new();
                            for val in arr_snapshot.iter() {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), val.clone());
                                }
                                self.stack.push(scope);
                                let cf = self.eval_block(body)?;
                                self.stack.pop();
                                results.push(cf.value());
                            }
                            Ok(Value::new_array(results))
                        } else {
                            Err(EvalError::new("ArgumentError", "map requires a block"))
                        }
                    }
                    "select" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let mut results = Vec::new();
                            for val in arr_snapshot.iter() {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), val.clone());
                                }
                                self.stack.push(scope);
                                let cf = self.eval_block(body)?;
                                self.stack.pop();
                                if self.is_truthy(&cf.value()) {
                                    results.push(val.clone());
                                }
                            }
                            Ok(Value::new_array(results))
                        } else {
                            Err(EvalError::new("ArgumentError", "select requires a block"))
                        }
                    }
                    "reduce" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let mut acc = args.first().cloned().unwrap_or(Value::Nil);
                            for val in arr_snapshot.iter() {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), acc);
                                }
                                if let Some(p) = params.get(1) {
                                    scope.insert(p.clone(), val.clone());
                                }
                                self.stack.push(scope);
                                let cf = self.eval_block(body)?;
                                self.stack.pop();
                                acc = cf.value();
                            }
                            Ok(acc)
                        } else {
                            Err(EvalError::new("ArgumentError", "reduce requires a block"))
                        }
                    }
                    "sort" => {
                        let mut sorted = arr_snapshot.clone();
                        if let Some(Expr::Block { params, body }) = block {
                            let mut err = None;
                            sorted.sort_by(|a, b| {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), a.clone());
                                }
                                if let Some(p) = params.get(1) {
                                    scope.insert(p.clone(), b.clone());
                                }
                                self.stack.push(scope);
                                match self.eval_block(body) {
                                    Ok(cf) => {
                                        self.stack.pop();
                                        if let Some(i) = cf.value().as_int() {
                                            if i < 0 {
                                                std::cmp::Ordering::Less
                                            } else if i > 0 {
                                                std::cmp::Ordering::Greater
                                            } else {
                                                std::cmp::Ordering::Equal
                                            }
                                        } else {
                                            std::cmp::Ordering::Equal
                                        }
                                    }
                                    Err(e) => {
                                        self.stack.pop();
                                        err = Some(e);
                                        std::cmp::Ordering::Equal
                                    }
                                }
                            });
                            if let Some(e) = err {
                                return Err(e);
                            }
                        } else {
                            sorted.sort_by(|a, b| {
                                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                            });
                        }
                        Ok(Value::new_array(sorted))
                    }
                    "sort_by" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let sorted = arr_snapshot.clone();
                            let mut sort_keys = Vec::new();
                            for val in &sorted {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), val.clone());
                                }
                                self.stack.push(scope);
                                let res = self.eval_block(body)?;
                                self.stack.pop();
                                sort_keys.push(res.value());
                            }
                            let mut paired: Vec<(Value, Value)> =
                                sorted.into_iter().zip(sort_keys).collect();
                            paired.sort_by(|(_, ka), (_, kb)| {
                                ka.partial_cmp(kb).unwrap_or(std::cmp::Ordering::Equal)
                            });
                            Ok(Value::new_array(
                                paired.into_iter().map(|(v, _)| v).collect(),
                            ))
                        } else {
                            Err(EvalError::new("ArgumentError", "sort_by requires a block"))
                        }
                    }
                    "partition" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let mut left = Vec::new();
                            let mut right = Vec::new();
                            for val in arr_snapshot.iter() {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), val.clone());
                                }
                                self.stack.push(scope);
                                let res = self.eval_block(body)?;
                                self.stack.pop();
                                if self.is_truthy(&res.value()) {
                                    left.push(val.clone());
                                } else {
                                    right.push(val.clone());
                                }
                            }
                            Ok(Value::new_array(vec![
                                Value::new_array(left),
                                Value::new_array(right),
                            ]))
                        } else {
                            Err(EvalError::new(
                                "ArgumentError",
                                "partition requires a block",
                            ))
                        }
                    }
                    "group_by" | "group_by_stable" => {
                        if let Some(Expr::Block { params, body }) = block {
                            let mut groups = HashMap::new();
                            let mut order = Vec::new();
                            for val in arr_snapshot.iter() {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), val.clone());
                                }
                                self.stack.push(scope);
                                let res = self.eval_block(body)?;
                                self.stack.pop();
                                let key = match res.value() {
                                    Value::String(s) | Value::Symbol(s) => s,
                                    v => v.to_string(),
                                };
                                if !groups.contains_key(&key) {
                                    groups.insert(key.clone(), Vec::new());
                                    order.push(key.clone());
                                }
                                groups.get_mut(&key).unwrap().push(val.clone());
                            }
                            let mut res_map = HashMap::new();
                            for k in order {
                                res_map.insert(
                                    k.clone(),
                                    Value::new_array(groups.remove(&k).unwrap()),
                                );
                            }
                            Ok(Value::new_hash(res_map))
                        } else {
                            Err(EvalError::new("ArgumentError", "group_by requires a block"))
                        }
                    }
                    "tally" => {
                        let mut tallies = HashMap::new();
                        for val in arr_snapshot.iter() {
                            let key = if let Some(Expr::Block { params, body }) = block {
                                let mut scope = HashMap::new();
                                if let Some(p) = params.first() {
                                    scope.insert(p.clone(), val.clone());
                                }
                                self.stack.push(scope);
                                let res = self.eval_block(body)?;
                                self.stack.pop();
                                match res.value() {
                                    Value::String(s) | Value::Symbol(s) => s,
                                    v => v.to_string(),
                                }
                            } else {
                                match val {
                                    Value::String(s) | Value::Symbol(s) => s.clone(),
                                    v => v.to_string(),
                                }
                            };
                            *tallies.entry(key).or_insert(0) += 1;
                        }
                        let mut res_map = HashMap::new();
                        for (k, v) in tallies {
                            res_map.insert(k, Value::Int(v));
                        }
                        Ok(Value::new_hash(res_map))
                    }
                    "[]=" => {
                        if args.len() < 2 {
                            return Err(EvalError::new(
                                "ArgumentError",
                                "[]= expects index and value",
                            ));
                        }
                        if let Some(i) = args[0].as_int() {
                            let mut mut_arr = a.write().unwrap();
                            let idx = if i < 0 { mut_arr.len() as i64 + i } else { i } as usize;
                            if idx < mut_arr.len() {
                                mut_arr[idx] = args[1].clone();
                                Ok(args[1].clone())
                            } else {
                                return Err(EvalError::new(
                                    "RuntimeError",
                                    &format!("index {} out of bounds", idx),
                                ));
                            }
                        } else {
                            return Err(EvalError::new("TypeError", "Array index must be int"));
                        }
                    }
                    _ => Err(EvalError::new(
                        "RuntimeError",
                        &format!("Method '{}' not supported for Array", method),
                    )),
                }
            }

            (Value::String(s), method, _) | (Value::Symbol(s), method, _) => match method {
                "length" | "size" => Ok(Value::Int(s.chars().count() as i64)),
                "bytesize" => Ok(Value::Int(s.len() as i64)),
                "empty?" => Ok(Value::Bool(s.is_empty())),
                "ord" => {
                    if let Some(c) = s.chars().next() {
                        Ok(Value::Int(c as i64))
                    } else {
                        Err(EvalError::new("TypeError", "ord requires non-empty string"))
                    }
                }
                "chomp" => {
                    let sep = args.get(0).and_then(|v| v.as_str()).unwrap_or("\n");
                    if s.ends_with(sep) {
                        Ok(Value::String(s[..s.len() - sep.len()].to_string()))
                    } else if sep == "\n" && s.ends_with("\r\n") {
                        Ok(Value::String(s[..s.len() - 2].to_string()))
                    } else {
                        Ok(Value::String(s.clone()))
                    }
                }
                "upcase" | "uppercase" => {
                    let res = s.to_uppercase();
                    if matches!(receiver, Value::Symbol(_)) {
                        Ok(Value::Symbol(res))
                    } else {
                        Ok(Value::String(res))
                    }
                }
                "downcase" | "lowercase" => {
                    let res = s.to_lowercase();
                    if matches!(receiver, Value::Symbol(_)) {
                        Ok(Value::Symbol(res))
                    } else {
                        Ok(Value::String(res))
                    }
                }
                "capitalize" => {
                    let mut c = s.chars();
                    let cap = match c.next() {
                        None => String::new(),
                        Some(f) => {
                            f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()
                        }
                    };
                    if matches!(receiver, Value::Symbol(_)) {
                        Ok(Value::Symbol(cap))
                    } else {
                        Ok(Value::String(cap))
                    }
                }
                "swapcase" => {
                    let res: String = s
                        .chars()
                        .map(|c| {
                            if c.is_uppercase() {
                                c.to_lowercase().next().unwrap()
                            } else {
                                c.to_uppercase().next().unwrap()
                            }
                        })
                        .collect();
                    if matches!(receiver, Value::Symbol(_)) {
                        Ok(Value::Symbol(res))
                    } else {
                        Ok(Value::String(res))
                    }
                }
                "reverse" => {
                    let res: String = s.chars().rev().collect();
                    if matches!(receiver, Value::Symbol(_)) {
                        Ok(Value::Symbol(res))
                    } else {
                        Ok(Value::String(res))
                    }
                }
                "strip!" => {
                    if let Value::Symbol(_) = receiver {
                        return Err(EvalError::new("TypeError", "Cannot mutate symbol"));
                    }
                    let res = s.trim();
                    if res == s {
                        Ok(Value::Nil)
                    } else {
                        Ok(Value::String(res.to_string()))
                    }
                }
                "upcase!" | "uppercase!" => {
                    let res = s.to_uppercase();
                    if res == s {
                        Ok(Value::Nil)
                    } else {
                        Ok(Value::String(res))
                    }
                }
                "downcase!" | "lowercase!" => {
                    let res = s.to_lowercase();
                    if res == s {
                        Ok(Value::Nil)
                    } else {
                        Ok(Value::String(res))
                    }
                }
                "capitalize!" => {
                    let mut c = s.chars();
                    let cap = match c.next() {
                        None => String::new(),
                        Some(f) => {
                            f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()
                        }
                    };
                    if cap == s {
                        Ok(Value::Nil)
                    } else {
                        Ok(Value::String(cap))
                    }
                }
                "swapcase!" => {
                    let res: String = s
                        .chars()
                        .map(|c| {
                            if c.is_uppercase() {
                                c.to_lowercase().next().unwrap()
                            } else {
                                c.to_uppercase().next().unwrap()
                            }
                        })
                        .collect();
                    if res == s {
                        Ok(Value::Nil)
                    } else {
                        Ok(Value::String(res))
                    }
                }
                "reverse!" => {
                    let res: String = s.chars().rev().collect();
                    if res == s {
                        Ok(Value::Nil)
                    } else {
                        Ok(Value::String(res))
                    }
                }
                "match" => {
                    if let Some(Value::String(pattern)) = args.first() {
                        let re = Regex::new(pattern).map_err(|e| {
                            EvalError::new("RuntimeError", &format!("invalid regex: {}", e))
                        })?;
                        if let Some(caps) = re.captures(&s) {
                            let groups: Vec<Value> = caps
                                .iter()
                                .map(|m| {
                                    m.map_or(Value::Nil, |mat| {
                                        Value::String(mat.as_str().to_string())
                                    })
                                })
                                .collect();
                            Ok(Value::new_array(groups))
                        } else {
                            Ok(Value::Nil)
                        }
                    } else {
                        Err(EvalError::new(
                            "ArgumentError",
                            "match expects a regex pattern",
                        ))
                    }
                }
                "scan" => {
                    if let Some(Value::String(pattern)) = args.first() {
                        let re = Regex::new(pattern).map_err(|e| {
                            EvalError::new("RuntimeError", &format!("invalid regex: {}", e))
                        })?;
                        let matches: Vec<Value> = re
                            .find_iter(&s)
                            .map(|m| Value::String(m.as_str().to_string()))
                            .collect();
                        Ok(Value::new_array(matches))
                    } else {
                        Err(EvalError::new(
                            "ArgumentError",
                            "scan expects a regex pattern",
                        ))
                    }
                }
                "index" => {
                    if let Some(Value::String(sub)) = args.first() {
                        let offset = args.get(1).and_then(|v| v.as_int()).unwrap_or(0) as usize;
                        let runes: Vec<char> = s.chars().collect();
                        if offset >= runes.len() {
                            return Ok(Value::Nil);
                        }
                        let search_str: String = runes[offset..].iter().collect();
                        if let Some(idx) = search_str.find(sub) {
                            let char_idx = search_str[..idx].chars().count();
                            return Ok(Value::Int((offset + char_idx) as i64));
                        }
                    }
                    Ok(Value::Nil)
                }
                "rindex" => {
                    if let Some(Value::String(sub)) = args.first() {
                        let runes: Vec<char> = s.chars().collect();
                        let offset =
                            args.get(1)
                                .and_then(|v| v.as_int())
                                .unwrap_or(runes.len() as i64) as usize;
                        let limit = if offset > runes.len() {
                            runes.len()
                        } else {
                            offset
                        };
                        let search_str: String = runes[..limit].iter().collect();
                        if let Some(idx) = search_str.rfind(sub) {
                            let char_idx = search_str[..idx].chars().count();
                            return Ok(Value::Int(char_idx as i64));
                        }
                    }
                    Ok(Value::Nil)
                }
                "start_with?" | "starts_with?" => {
                    if let Some(Value::String(pre)) = args.first() {
                        Ok(Value::Bool(s.starts_with(pre)))
                    } else {
                        Ok(Value::Bool(false))
                    }
                }
                "end_with?" | "ends_with?" => {
                    if let Some(Value::String(suf)) = args.first() {
                        Ok(Value::Bool(s.ends_with(suf)))
                    } else {
                        Ok(Value::Bool(false))
                    }
                }
                "contains?" | "include?" => {
                    if let Some(Value::String(sub)) = args.first() {
                        Ok(Value::Bool(s.contains(sub)))
                    } else {
                        Ok(Value::Bool(false))
                    }
                }
                "lstrip" => Ok(Value::String(s.trim_start().to_string())),
                "rstrip" => Ok(Value::String(s.trim_end().to_string())),
                "strip" => Ok(Value::String(s.trim().to_string())),
                "replace" => {
                    if let (Some(Value::String(old)), Some(Value::String(new))) =
                        (args.get(0), args.get(1))
                    {
                        Ok(Value::String(s.replace(old, new)))
                    } else {
                        Ok(Value::String(s.clone()))
                    }
                }
                "delete_prefix" => {
                    if let Some(Value::String(pre)) = args.first() {
                        Ok(Value::String(s.strip_prefix(pre).unwrap_or(&s).to_string()))
                    } else {
                        Ok(Value::String(s.clone()))
                    }
                }
                "delete_suffix" => {
                    if let Some(Value::String(suf)) = args.first() {
                        Ok(Value::String(s.strip_suffix(suf).unwrap_or(&s).to_string()))
                    } else {
                        Ok(Value::String(s.clone()))
                    }
                }
                "clear" => Ok(Value::String(String::new())),
                "concat" => {
                    let mut res = s.clone();
                    for arg in args {
                        res.push_str(&arg.to_string());
                    }
                    Ok(Value::String(res))
                }
                "chr" => {
                    if let Some(c) = s.chars().next() {
                        Ok(Value::String(c.to_string()))
                    } else {
                        Ok(Value::String(String::new()))
                    }
                }
                "split" => {
                    let sep = args.get(0).and_then(|v| v.as_str());
                    let parts = if let Some(s_sep) = sep {
                        if s_sep.is_empty() {
                            s.chars().map(|c| Value::String(c.to_string())).collect()
                        } else {
                            s.split(s_sep)
                                .map(|p| Value::String(p.to_string()))
                                .collect()
                        }
                    } else {
                        s.split_whitespace()
                            .map(|p| Value::String(p.to_string()))
                            .collect()
                    };
                    Ok(Value::new_array(parts))
                }
                "sub" | "sub!" | "gsub" | "gsub!" => {
                    let pattern = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
                    let replacement = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
                    let is_regex = kwargs
                        .get("regex")
                        .map(|v| self.is_truthy(v))
                        .unwrap_or(false);
                    let all = method.contains("gsub");
                    let result = if is_regex {
                        let re = Regex::new(pattern)
                            .map_err(|e| EvalError::new("RuntimeError", &e.to_string()))?;
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
                "slice" => {
                    let chars: Vec<char> = s.chars().collect();
                    if args.len() == 1 {
                        if let Some(i) = args[0].as_int() {
                            let idx = if i < 0 { chars.len() as i64 + i } else { i } as usize;
                            Ok(chars
                                .get(idx)
                                .map_or(Value::Nil, |c| Value::String(c.to_string())))
                        } else {
                            Ok(Value::Nil)
                        }
                    } else if args.len() >= 2 {
                        if let (Some(start), Some(len)) = (args[0].as_int(), args[1].as_int()) {
                            let start_idx = if start < 0 {
                                chars.len() as i64 + start
                            } else {
                                start
                            } as usize;
                            let end_idx = (start_idx + len as usize).min(chars.len());
                            if start_idx >= chars.len() {
                                Ok(Value::Nil)
                            } else {
                                Ok(Value::String(chars[start_idx..end_idx].iter().collect()))
                            }
                        } else {
                            Ok(Value::Nil)
                        }
                    } else {
                        Ok(Value::Nil)
                    }
                }
                "squish" | "squish!" => {
                    let res = Regex::new(r"\s+")
                        .unwrap()
                        .replace_all(s.trim(), " ")
                        .to_string();
                    if method.ends_with('!') {
                        if res == s {
                            Ok(Value::Nil)
                        } else {
                            Ok(Value::String(res))
                        }
                    } else {
                        Ok(Value::String(res))
                    }
                }
                "template" => {
                    let mut result = s.clone();
                    if let Some(Value::Hash(h)) = args.first() {
                        let hash = h.read().unwrap();
                        let re = Regex::new(r"\{\{([a-zA-Z0-9_\.]+)\}\}").unwrap();
                        let mut replaced = result.clone();
                        for cap in re.captures_iter(&result) {
                            let full_match = &cap[0];
                            let key_path = &cap[1];
                            let mut current_val = Value::Nil;
                            let mut found = false;
                            let parts: Vec<&str> = key_path.split('.').collect();
                            if let Some(first_key) = parts.first() {
                                if let Some(val) = hash.get(*first_key) {
                                    current_val = val.clone();
                                    found = true;
                                    for part in parts.iter().skip(1) {
                                        let next_val = if let Value::Hash(ref inner_h) = current_val
                                        {
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
                _ => Err(EvalError::new(
                    "RuntimeError",
                    &format!("Method '{}' not supported for String", method),
                )),
            },

            (Value::Int(i), method, block) => match method {
                "seconds" | "second" => Ok(Value::Duration(i)),
                "minutes" | "minute" => Ok(Value::Duration(i * 60)),
                "hours" | "hour" => Ok(Value::Duration(i * 3600)),
                "days" | "day" => Ok(Value::Duration(i * 86400)),
                "weeks" | "week" => Ok(Value::Duration(i * 86400 * 7)),
                "abs" => Ok(Value::Int(i.abs())),
                "even?" => Ok(Value::Bool(i % 2 == 0)),
                "odd?" => Ok(Value::Bool(i % 2 != 0)),
                "clamp" => {
                    if args.len() != 2 {
                        return Err(EvalError::new("ArgumentError", "clamp expects min and max"));
                    }
                    let min = args[0]
                        .as_int()
                        .ok_or_else(|| EvalError::new("TypeError", "min must be int"))?;
                    let max = args[1]
                        .as_int()
                        .ok_or_else(|| EvalError::new("TypeError", "max must be int"))?;
                    Ok(Value::Int(i.clamp(min, max)))
                }
                "times" => {
                    if let Some(Expr::Block { params, body }) = block {
                        for idx in 0..i {
                            let mut scope = HashMap::new();
                            if let Some(p) = params.first() {
                                scope.insert(p.clone(), Value::Int(idx));
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
                        Ok(Value::Int(i))
                    } else {
                        Err(EvalError::new("ArgumentError", "times requires a block"))
                    }
                }
                _ => Err(EvalError::new(
                    "RuntimeError",
                    &format!("unknown int member {}", method),
                )),
            },

            (Value::Float(f), method, _) => match method {
                "abs" => Ok(Value::Float(f.abs())),
                "round" => Ok(Value::Int(f.round() as i64)),
                "floor" => Ok(Value::Int(f.floor() as i64)),
                "ceil" => Ok(Value::Int(f.ceil() as i64)),
                "clamp" => {
                    if args.len() != 2 {
                        return Err(EvalError::new("ArgumentError", "clamp expects min and max"));
                    }
                    let min = args[0]
                        .as_float()
                        .or_else(|| args[0].as_int().map(|i| i as f64))
                        .ok_or_else(|| EvalError::new("TypeError", "min must be numeric"))?;
                    let max = args[1]
                        .as_float()
                        .or_else(|| args[1].as_int().map(|i| i as f64))
                        .ok_or_else(|| EvalError::new("TypeError", "max must be numeric"))?;
                    Ok(Value::Float(f.clamp(min, max)))
                }
                _ => Err(EvalError::new(
                    "RuntimeError",
                    &format!("unknown float member {}", method),
                )),
            },

            (Value::Money { cents, currency }, method, _) => match method {
                "cents" => Ok(Value::Int(cents)),
                "currency" => Ok(Value::String(currency)),
                "format" => Ok(Value::String(format!(
                    "{:.2} {}",
                    cents as f64 / 100.0,
                    currency
                ))),
                _ => Err(EvalError::new(
                    "RuntimeError",
                    &format!("unknown money member {}", method),
                )),
            },

            (Value::Duration(s), method, _) => match method {
                "seconds" | "to_i" => Ok(Value::Int(s)),
                "iso8601" => {
                    let h = s / 3600;
                    let m = (s % 3600) / 60;
                    let sec = s % 60;
                    let mut res = "PT".to_string();
                    if h > 0 {
                        res.push_str(&format!("{}H", h));
                    }
                    if m > 0 {
                        res.push_str(&format!("{}M", m));
                    }
                    if sec > 0 || (h == 0 && m == 0) {
                        res.push_str(&format!("{}S", sec));
                    }
                    Ok(Value::String(res))
                }
                "in_hours" => Ok(Value::Float(s as f64 / 3600.0)),
                "parts" => {
                    let mut parts = HashMap::new();
                    parts.insert("hours".to_string(), Value::Int(s / 3600));
                    parts.insert("minutes".to_string(), Value::Int((s % 3600) / 60));
                    parts.insert("seconds".to_string(), Value::Int(s % 60));
                    Ok(Value::new_hash(parts))
                }
                "ago" | "before" | "until" => {
                    let anchor = args
                        .first()
                        .and_then(|v| match v {
                            Value::Time(t) => Some(*t),
                            _ => None,
                        })
                        .unwrap_or(Utc::now());
                    let d = chrono::Duration::seconds(s);
                    Ok(Value::Time(anchor - d))
                }
                "after" | "since" | "from_now" => {
                    let anchor = args
                        .first()
                        .and_then(|v| match v {
                            Value::Time(t) => Some(*t),
                            _ => None,
                        })
                        .unwrap_or(Utc::now());
                    let d = chrono::Duration::seconds(s);
                    Ok(Value::Time(anchor + d))
                }
                _ => Err(EvalError::new(
                    "RuntimeError",
                    &format!("unknown duration member {}", method),
                )),
            },

            (Value::Time(t), method, _) => match method {
                "format" => {
                    let fmt = args
                        .get(0)
                        .and_then(|v| v.as_str())
                        .unwrap_or("%Y-%m-%dT%H:%M:%S%Z");
                    let rust_fmt = match fmt {
                        "2006-01-02T15:04:05Z07:00" => "%Y-%m-%dT%H:%M:%S%Z",
                        "15:04:05" => "%H:%M:%S",
                        "2006-01-02" => "%Y-%m-%d",
                        _ => fmt,
                    };
                    let mut res = t.format(rust_fmt).to_string();
                    if fmt == "15:04:05" {
                        res.push_str("UTC");
                    }
                    Ok(Value::String(res))
                }
                "year" => Ok(Value::Int(t.year() as i64)),
                "month" => Ok(Value::Int(t.month() as i64)),
                "day" => Ok(Value::Int(t.day() as i64)),
                "hour" => Ok(Value::Int(t.hour() as i64)),
                "minute" => Ok(Value::Int(t.minute() as i64)),
                "second" => Ok(Value::Int(t.second() as i64)),
                _ => Err(EvalError::new(
                    "RuntimeError",
                    &format!("unknown time member {}", method),
                )),
            },

            (Value::Instance(inst), method, _) => {
                let class = inst.read().unwrap().class.clone();
                if let Some(f) = class.methods.read().unwrap().get(method).cloned() {
                    if f.is_private {
                        if let Some(Value::Instance(curr_self)) = self.get_var("self") {
                            if !Arc::ptr_eq(&curr_self, &inst) {
                                return Err(EvalError::new(
                                    "RuntimeError",
                                    &format!("Method '{}' is private", method),
                                ));
                            }
                        } else {
                            return Err(EvalError::new(
                                "RuntimeError",
                                &format!("Method '{}' is private", method),
                            ));
                        }
                    }
                    self.stack.push(HashMap::new());
                    self.set_var("self", Value::Instance(inst.clone()));
                    let cf = self.call_function_def_cf(&f, args, block);
                    self.stack.pop();
                    cf.map(|c| c.value())
                } else {
                    Err(EvalError::new(
                        "RuntimeError",
                        &format!("Method '{}' not found for class '{}'", method, class.name),
                    ))
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

            (Value::Function(f), "call", _) => self.call_function_def(&f, args, block),

            (_, "to_string", _) => Ok(Value::String(receiver.to_string())),

            _ => Err(EvalError::new(
                "RuntimeError",
                &format!("Method '{}' not supported for this type", method),
            )),
        }
    }

    pub fn call_function(&mut self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let fn_def = self
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Function '{}' not found", name))?;

        self.call_function_def(&fn_def, args, None)
            .map_err(|e| e.message)
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
        block: Option<&Expr>,
    ) -> Result<ControlFlow, EvalError> {
        self.recursion_depth += 1;
        if self.recursion_depth > MAX_RECURSION_DEPTH {
            self.recursion_depth -= 1;
            return Err(EvalError::new(
                "RuntimeError",
                "Stack overflow: maximum recursion depth exceeded",
            ));
        }

        let old_block = self.current_block.take();
        if let Some(b) = block {
            self.current_block = Some(self.eval_expr(b)?);
        }

        let result = self.do_call_function_def(func, args, block)?;

        if let Some(expected_ret_ty) = &func.return_type {
            if let Err(e) = self.validate_value_type(&result.value(), expected_ret_ty) {
                self.current_block = old_block;
                self.recursion_depth -= 1;
                return Err(EvalError::new(
                    "TypeError",
                    &format!(
                        "return value expected {}, got {}: {}",
                        expected_ret_ty.name,
                        result.value().kind_name(),
                        e.message
                    ),
                ));
            }
        }

        self.current_block = old_block;
        self.recursion_depth -= 1;
        Ok(result)
    }

    fn do_call_function_def(
        &mut self,
        func: &FunctionDef,
        args: Vec<Value>,
        _block: Option<&Expr>,
    ) -> Result<ControlFlow, EvalError> {
        if func.params.len() != args.len() {
            return Err(EvalError::new(
                "ArgumentError",
                &format!(
                    "Expected {} arguments, but got {}",
                    func.params.len(),
                    args.len()
                ),
            ));
        }

        let mut new_scope = HashMap::new();
        for (param, val) in func.params.iter().zip(args) {
            if let Some(expected_ty) = &param.param_type {
                if let Err(e) = self.validate_value_type(&val, expected_ty) {
                    return Err(EvalError::new(
                        "TypeError",
                        &format!(
                            "argument {} expected {}, got {}: {}",
                            param.name,
                            expected_ty.name,
                            val.kind_name(),
                            e.message
                        ),
                    ));
                }
            }

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
            (Value::Hash(h), BinaryOp::Index, Value::String(s))
            | (Value::Hash(h), BinaryOp::Index, Value::Symbol(s)) => {
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
                let mut quotient = l / r;
                let remainder = l % r;
                if remainder != 0 && ((remainder < 0) != (r < 0)) {
                    quotient -= 1;
                }
                Ok(Value::Int(quotient))
            }
            (Value::Int(l), BinaryOp::Modulo, Value::Int(r)) => {
                if r == 0 {
                    return Err("Modulo by zero".to_string());
                }
                let mut remainder = l % r;
                if remainder != 0 && ((remainder < 0) != (r < 0)) {
                    remainder += r;
                }
                Ok(Value::Int(remainder))
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

            (Value::Array(al), BinaryOp::Sub, Value::Array(ar)) => {
                let l = al.read().unwrap();
                let r = ar.read().unwrap();
                let mut new_arr = Vec::new();
                for val in l.iter() {
                    if !r.contains(val) {
                        new_arr.push(val.clone());
                    }
                }
                Ok(Value::new_array(new_arr))
            }
            (Value::Hash(hl), BinaryOp::Sub, Value::Array(ar)) => {
                let l = hl.read().unwrap();
                let r = ar.read().unwrap();
                let mut new_map = (*l).clone();
                for val in r.iter() {
                    if let Some(s) = val.as_str() {
                        new_map.remove(s);
                    } else if let Value::Symbol(s) = val {
                        new_map.remove(s);
                    }
                }
                Ok(Value::new_hash(new_map))
            }
            (Value::Hash(hl), BinaryOp::Sub, Value::Hash(hr)) => {
                let l = hl.read().unwrap();
                let r = hr.read().unwrap();
                let mut new_map = (*l).clone();
                for k in r.keys() {
                    new_map.remove(k);
                }
                Ok(Value::new_hash(new_map))
            }

            (Value::Time(t), BinaryOp::Add, Value::Duration(s)) => {
                let d = chrono::Duration::seconds(s);
                Ok(Value::Time(t + d))
            }
            (Value::Duration(s), BinaryOp::Add, Value::Time(t)) => {
                let d = chrono::Duration::seconds(s);
                Ok(Value::Time(t + d))
            }
            (Value::Duration(l), BinaryOp::Add, Value::Duration(r)) => Ok(Value::Duration(l + r)),
            (Value::Time(l), BinaryOp::Sub, Value::Time(r)) => {
                let diff = l - r;
                Ok(Value::Duration(diff.num_seconds()))
            }
            (Value::Time(t), BinaryOp::Sub, Value::Duration(s)) => {
                let d = chrono::Duration::seconds(s);
                Ok(Value::Time(t - d))
            }
            (Value::Duration(l), BinaryOp::Sub, Value::Duration(r)) => Ok(Value::Duration(l - r)),

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

            _ => Err("Binary operation not supported".to_string()),
        }
    }

    fn validate_value_type(&mut self, val: &Value, expected: &TypeExpr) -> Result<(), EvalError> {
        let mut state = TypeValidationState {
            active: std::collections::HashSet::new(),
        };
        self.do_validate_value_type(val, expected, &mut state)
    }

    fn do_validate_value_type(
        &mut self,
        val: &Value,
        expected: &TypeExpr,
        state: &mut TypeValidationState,
    ) -> Result<(), EvalError> {
        if expected.nullable && val.is_nil() {
            return Ok(());
        }

        // Cycle detection for recursive types
        let visit = match val {
            Value::Array(a) => Some((Arc::as_ptr(a) as usize, expected.clone())),
            Value::Hash(h) | Value::Object(h) => Some((Arc::as_ptr(h) as usize, expected.clone())),
            _ => None,
        };

        if let Some(v) = visit.as_ref() {
            if state.active.contains(v) {
                return Ok(());
            }
            state.active.insert(v.clone());
        }

        let result = match &expected.kind {
            TypeKind::Any => Ok(()),
            TypeKind::Int => {
                if matches!(val, Value::Int(_)) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Int, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Float => {
                if matches!(val, Value::Float(_)) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Float, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Number => {
                if matches!(val, Value::Int(_) | Value::Float(_)) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Number, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::String => {
                if matches!(val, Value::String(_)) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected String, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Bool => {
                if matches!(val, Value::Bool(_)) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Bool, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Nil => {
                if matches!(val, Value::Nil) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Nil, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Duration => {
                if matches!(val, Value::Duration(_)) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Duration, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Time => {
                if matches!(val, Value::Time(_)) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Time, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Money => {
                if matches!(val, Value::Money { .. }) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Money, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Array => {
                if let Value::Array(a) = val {
                    if expected.type_args.is_empty() {
                        Ok(())
                    } else if expected.type_args.len() != 1 {
                        Err(EvalError::new(
                            "TypeError",
                            "Array type expects exactly 1 type argument",
                        ))
                    } else {
                        let elem_type = &expected.type_args[0];
                        let arr = a.read().unwrap();
                        for elem in arr.iter() {
                            self.do_validate_value_type(elem, elem_type, state)?;
                        }
                        Ok(())
                    }
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Array, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Hash => {
                if let Value::Hash(h) | Value::Object(h) = val {
                    if expected.type_args.is_empty() {
                        Ok(())
                    } else if expected.type_args.len() != 2 {
                        Err(EvalError::new(
                            "TypeError",
                            "Hash type expects exactly 2 type arguments",
                        ))
                    } else {
                        let key_type = &expected.type_args[0];
                        let val_type = &expected.type_args[1];
                        let hash = h.read().unwrap();
                        for (k, v) in hash.iter() {
                            self.do_validate_value_type(
                                &Value::Symbol(k.clone()),
                                key_type,
                                state,
                            )?;
                            self.do_validate_value_type(v, val_type, state)?;
                        }
                        Ok(())
                    }
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Hash, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Shape => {
                if let Value::Hash(h) | Value::Object(h) = val {
                    let hash = h.read().unwrap();
                    if hash.len() != expected.shape.len() {
                        return Err(EvalError::new(
                            "TypeError",
                            &format!(
                                "Shape mismatch: expected {} fields, got {}",
                                expected.shape.len(),
                                hash.len()
                            ),
                        ));
                    }
                    for (field, field_type) in &expected.shape {
                        if let Some(field_val) = hash.get(field) {
                            self.do_validate_value_type(field_val, field_type, state)?;
                        } else {
                            return Err(EvalError::new(
                                "TypeError",
                                &format!("Missing field: {}", field),
                            ));
                        }
                    }
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Hash for Shape, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Union => {
                for opt in &expected.union_types {
                    if self.do_validate_value_type(val, opt, state).is_ok() {
                        return Ok(());
                    }
                }
                Err(EvalError::new(
                    "TypeError",
                    &format!("Value {} does not match any union type", val.to_string()),
                ))
            }
            TypeKind::Enum => {
                if let Value::EnumVariant { enum_name, .. } = val {
                    if enum_name.to_lowercase() == expected.name.to_lowercase() {
                        Ok(())
                    } else {
                        Err(EvalError::new(
                            "TypeError",
                            &format!("Expected enum {}, got {}", expected.name, enum_name),
                        ))
                    }
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected EnumVariant, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Function => {
                if matches!(val, Value::Function(_) | Value::Builtin(_)) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Function, got {}", val.kind_name()),
                    ))
                }
            }
            TypeKind::Object => {
                if matches!(val, Value::Object(_)) {
                    Ok(())
                } else {
                    Err(EvalError::new(
                        "TypeError",
                        &format!("Expected Object, got {}", val.kind_name()),
                    ))
                }
            }
            _ => Err(EvalError::new(
                "TypeError",
                &format!("Unsupported or unknown type {}", expected.name),
            )),
        };

        if let Some(v) = visit {
            state.active.remove(&v);
        }

        result
    }
}

struct TypeValidationState {
    active: std::collections::HashSet<(usize, TypeExpr)>,
}
