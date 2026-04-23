use crate::ast::*;
use crate::value::{ClassDef, FunctionDef, InstanceData, Param, Value};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct Engine {
    globals: HashMap<String, Value>,
    functions: HashMap<String, FunctionDef>,
    enums: HashMap<String, EnumDef>,
    classes: HashMap<String, Arc<ClassDef>>,
    stack: Vec<HashMap<String, Value>>,
}

struct EnumDef {
    members: Vec<String>,
}

enum ControlFlow {
    Continue(Value),
    Break(Value),
    Next(Value),
    Return(Value),
}

impl Engine {
    pub fn new() -> Self {
        Self {
            globals: HashMap::new(),
            functions: HashMap::new(),
            enums: HashMap::new(),
            classes: HashMap::new(),
            stack: vec![HashMap::new()], // Root scope
        }
    }

    pub fn eval_stmt(&mut self, stmt: &Stmt) -> Result<Value, String> {
        match self.eval_stmt_internal(stmt)? {
            ControlFlow::Continue(v) => Ok(v),
            ControlFlow::Break(v) => Ok(v),
            ControlFlow::Next(v) => Ok(v),
            ControlFlow::Return(v) => Ok(v),
        }
    }

    fn eval_stmt_internal(&mut self, stmt: &Stmt) -> Result<ControlFlow, String> {
        match stmt {
            Stmt::Expression(expr) => Ok(ControlFlow::Continue(self.eval_expr_mut(expr)?)),
            Stmt::Assignment { name, value } => {
                let val = self.eval_expr_mut(value)?;
                self.set_var(name, val.clone());
                Ok(ControlFlow::Continue(val))
            }
            Stmt::IvarAssignment { name, value } => {
                let val = self.eval_expr_mut(value)?;
                self.set_ivar(name, val.clone())?;
                Ok(ControlFlow::Continue(val))
            }
            Stmt::CvarAssignment { name, value } => {
                let val = self.eval_expr_mut(value)?;
                self.set_cvar(name, val.clone())?;
                Ok(ControlFlow::Continue(val))
            }
            Stmt::If {
                condition,
                then_branch,
                elsif_branches,
                else_branch,
            } => {
                let cond_val = self.eval_expr_mut(condition)?;
                if self.is_truthy(&cond_val) {
                    self.eval_block(then_branch)
                } else {
                    for (elsif_cond, elsif_body) in elsif_branches {
                        let elsif_val = self.eval_expr_mut(elsif_cond)?;
                        if self.is_truthy(&elsif_val) {
                            return self.eval_block(elsif_body);
                        }
                    }
                    if let Some(else_b) = else_branch {
                        self.eval_block(else_b)
                    } else {
                        Ok(ControlFlow::Continue(Value::Nil))
                    }
                }
            }
            Stmt::While { condition, body } => {
                let mut last_val = Value::Nil;
                loop {
                    let cond_val = self.eval_expr_mut(condition)?;
                    if !self.is_truthy(&cond_val) {
                        break;
                    }
                    match self.eval_block(body)? {
                        ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                        ControlFlow::Break(v) => {
                            last_val = v;
                            break;
                        }
                        ControlFlow::Next(v) => {
                            last_val = v;
                        }
                        ControlFlow::Continue(v) => {
                            last_val = v;
                        }
                    }
                }
                Ok(ControlFlow::Continue(last_val))
            }
            Stmt::Until { condition, body } => {
                let mut last_val = Value::Nil;
                loop {
                    let cond_val = self.eval_expr_mut(condition)?;
                    if self.is_truthy(&cond_val) {
                        break;
                    }
                    match self.eval_block(body)? {
                        ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                        ControlFlow::Break(v) => {
                            last_val = v;
                            break;
                        }
                        ControlFlow::Next(v) => {
                            last_val = v;
                        }
                        ControlFlow::Continue(v) => {
                            last_val = v;
                        }
                    }
                }
                Ok(ControlFlow::Continue(last_val))
            }
            Stmt::For {
                var,
                iterable,
                body,
            } => {
                let mut last_val = Value::Nil;
                let iter_val = self.eval_expr_mut(iterable)?;
                if let Value::Array(arr) = iter_val {
                    for item in arr {
                        self.set_var(var, item);
                        match self.eval_block(body)? {
                            ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                            ControlFlow::Break(v) => {
                                last_val = v;
                                break;
                            }
                            ControlFlow::Next(v) => {
                                last_val = v;
                            }
                            ControlFlow::Continue(v) => {
                                last_val = v;
                            }
                        }
                    }
                }
                Ok(ControlFlow::Continue(last_val))
            }
            Stmt::Break => Ok(ControlFlow::Break(Value::Nil)),
            Stmt::Next => Ok(ControlFlow::Next(Value::Nil)),
            Stmt::Function(f) => {
                self.functions.insert(
                    f.name.clone(),
                    FunctionDef {
                        params: f.params.clone(),
                        body: f.body.clone(),
                        is_private: f.is_private,
                    },
                );
                Ok(ControlFlow::Continue(Value::Nil))
            }
            Stmt::EnumDef { name, members } => {
                self.enums.insert(
                    name.clone(),
                    EnumDef {
                        members: members.iter().map(|m| m.name.clone()).collect(),
                    },
                );
                Ok(ControlFlow::Continue(Value::Nil))
            }
            Stmt::ClassDef { name, body } => {
                self.eval_class_def(name, body)?;
                Ok(ControlFlow::Continue(Value::Nil))
            }
            Stmt::PropertyDecl { .. } => {
                Err("PropertyDecl only supported inside class definition".to_string())
            }
            Stmt::Return(expr) => {
                let val = if let Some(e) = expr {
                    self.eval_expr_mut(e)?
                } else {
                    Value::Nil
                };
                Ok(ControlFlow::Return(val))
            }
            Stmt::Try {
                body,
                rescue,
                ensure,
            } => {
                let result = self.eval_block(body);
                let final_res = match result {
                    Ok(cf) => Ok(cf),
                    Err(e) => {
                        if let Some(rescue_clause) = rescue {
                            self.eval_block(&rescue_clause.body)
                        } else {
                            Err(e)
                        }
                    }
                };

                if let Some(ensure_body) = ensure {
                    self.eval_block(ensure_body)?;
                }
                final_res
            }
        }
    }

    fn eval_block(&mut self, stmts: &[Stmt]) -> Result<ControlFlow, String> {
        let mut last_val = Value::Nil;
        for stmt in stmts {
            match self.eval_stmt_internal(stmt)? {
                ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                ControlFlow::Break(v) => return Ok(ControlFlow::Break(v)),
                ControlFlow::Next(v) => return Ok(ControlFlow::Next(v)),
                ControlFlow::Continue(v) => last_val = v,
            }
        }
        Ok(ControlFlow::Continue(last_val))
    }

    fn set_var(&mut self, name: &str, val: Value) {
        if let Some(scope) = self.stack.last_mut() {
            scope.insert(name.to_string(), val);
        }
    }

    fn get_var(&self, name: &str) -> Option<Value> {
        for scope in self.stack.iter().rev() {
            if let Some(val) = scope.get(name) {
                return Some(val.clone());
            }
        }
        self.globals.get(name).cloned()
    }

    fn set_ivar(&mut self, name: &str, val: Value) -> Result<(), String> {
        let self_val = self.get_var("self").ok_or("No 'self' in current scope")?;
        if let Value::Instance(inst) = self_val {
            inst.write().unwrap().ivars.insert(name.to_string(), val);
            Ok(())
        } else {
            Err("'self' is not an instance".to_string())
        }
    }

    fn get_ivar(&self, name: &str) -> Result<Value, String> {
        let self_val = self.get_var("self").ok_or("No 'self' in current scope")?;
        if let Value::Instance(inst) = self_val {
            Ok(inst
                .read()
                .unwrap()
                .ivars
                .get(name)
                .cloned()
                .unwrap_or(Value::Nil))
        } else {
            Err("'self' is not an instance".to_string())
        }
    }

    fn set_cvar(&mut self, _name: &str, _val: Value) -> Result<(), String> {
        Err("Class variables not fully implemented in MVP".to_string())
    }

    fn get_cvar(&self, _name: &str) -> Result<Value, String> {
        Err("Class variables not fully implemented in MVP".to_string())
    }

    fn is_truthy(&self, val: &Value) -> bool {
        match val {
            Value::Nil => false,
            Value::Bool(b) => *b,
            _ => true,
        }
    }

    pub fn eval_expr_mut(&mut self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Literal(val) => Ok(val.clone()),
            Expr::Variable(name) => {
                if let Some(val) = self.get_var(name) {
                    return Ok(val);
                }
                if let Some(c) = self.classes.get(name) {
                    return Ok(Value::Class(c.clone()));
                }
                if self.enums.contains_key(name) {
                    return Ok(Value::String(format!("ENUM:{}", name)));
                }
                Err(format!("Variable '{}' not found", name))
            }
            Expr::InstanceVar(name) => self.get_ivar(name),
            Expr::ClassVar(name) => self.get_cvar(name),
            Expr::Unary { op, expr } => {
                let val = self.eval_expr_mut(expr)?;
                self.eval_unary(*op, &val)
            }
            Expr::Binary { left, op, right } => {
                if *op == BinaryOp::And {
                    let lhs = self.eval_expr_mut(left)?;
                    if !self.is_truthy(&lhs) {
                        return Ok(lhs);
                    }
                    return self.eval_expr_mut(right);
                }
                if *op == BinaryOp::Or {
                    let lhs = self.eval_expr_mut(left)?;
                    if self.is_truthy(&lhs) {
                        return Ok(lhs);
                    }
                    return self.eval_expr_mut(right);
                }

                let lhs = self.eval_expr_mut(left)?;
                let rhs = self.eval_expr_mut(right)?;
                self.eval_binary(lhs, *op, rhs)
            }
            Expr::Member {
                receiver,
                method,
                args,
                block,
            } => {
                let rec_val = self.eval_expr_mut(receiver)?;

                if let Value::String(s) = &rec_val {
                    if let Some(enum_name) = s.strip_prefix("ENUM:") {
                        if let Some(enum_def) = self.enums.get(enum_name) {
                            if enum_def.members.contains(&method.to_string()) {
                                return Ok(Value::EnumVariant {
                                    enum_name: enum_name.to_string(),
                                    variant_name: method.to_string(),
                                });
                            }
                        }
                    }
                }

                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr_mut(arg)?);
                }
                let block_val = if let Some(b) = block {
                    Some(self.eval_expr_mut(b)?)
                } else {
                    None
                };
                self.eval_member_mut(rec_val, method, arg_vals, block_val)
            }
            Expr::Call { func, args, .. } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr_mut(arg)?);
                }
                match func.as_str() {
                    "print" => {
                        for arg in &arg_vals {
                            print!("{} ", arg.to_string());
                        }
                        println!();
                        Ok(Value::Nil)
                    }
                    "now" => Ok(Value::Time(Utc::now())),
                    "uuid" => Ok(Value::String(Uuid::new_v4().to_string())),
                    "json_parse" => {
                        if let Some(Value::String(s)) = arg_vals.first() {
                            let json_val: serde_json::Value =
                                serde_json::from_str(s).map_err(|e| e.to_string())?;
                            Ok(self.json_to_vibe(json_val))
                        } else {
                            Err("json_parse expects a string argument".to_string())
                        }
                    }
                    "json_stringify" => {
                        if let Some(val) = arg_vals.first() {
                            let json_val = self.vibe_to_json(val.clone());
                            let s = serde_json::to_string(&json_val).map_err(|e| e.to_string())?;
                            Ok(Value::String(s))
                        } else {
                            Err("json_stringify expects an argument".to_string())
                        }
                    }
                    _ => {
                        // Method Resolution Logic for local calls
                        let current_self = self.get_var("self");
                        if let Some(Value::Instance(inst)) = current_self {
                            let class_def = inst.read().unwrap().class.clone();
                            if let Some(fn_def) = class_def.methods.get(func) {
                                // Calling a local method within 'self' context
                                self.stack.push(HashMap::from([(
                                    "self".to_string(),
                                    Value::Instance(inst.clone()),
                                )]));
                                let res = self.call_function_def(fn_def, arg_vals)?;
                                self.stack.pop();
                                return Ok(res);
                            }
                        }
                        self.call_function(func, arg_vals)
                    }
                }
            }
            Expr::Array(elements) => {
                let mut vals = Vec::new();
                for e in elements {
                    vals.push(self.eval_expr_mut(e)?);
                }
                Ok(Value::Array(vals))
            }
            Expr::Hash(pairs) => {
                let mut hash = HashMap::new();
                for (key, val_expr) in pairs {
                    hash.insert(key.clone(), self.eval_expr_mut(val_expr)?);
                }
                Ok(Value::Hash(hash))
            }
            Expr::Case {
                target,
                clauses,
                else_expr,
            } => {
                let target_val = self.eval_expr_mut(target)?;
                for clause in clauses {
                    for val_expr in &clause.values {
                        let val = self.eval_expr_mut(val_expr)?;
                        if target_val == val {
                            return match self.eval_block(&clause.body)? {
                                ControlFlow::Continue(v) => Ok(v),
                                ControlFlow::Return(v) => Ok(v),
                                _ => Err("Control flow in case result not supported".to_string()),
                            };
                        }
                    }
                }
                if let Some(else_b) = else_expr {
                    match self.eval_block(else_b)? {
                        ControlFlow::Continue(v) => Ok(v),
                        _ => Err("Control flow in case else result not supported".to_string()),
                    }
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
                            let val = self.eval_expr_mut(e)?;
                            result.push_str(&val.to_string());
                        }
                    }
                }
                Ok(Value::String(result))
            }
        }
    }

    fn json_to_vibe(&self, json: serde_json::Value) -> Value {
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
                Value::Array(a.into_iter().map(|v| self.json_to_vibe(v)).collect())
            }
            serde_json::Value::Object(o) => {
                let mut hash = HashMap::new();
                for (k, v) in o {
                    hash.insert(k, self.json_to_vibe(v));
                }
                Value::Hash(hash)
            }
        }
    }

    fn vibe_to_json(&self, vibe: Value) -> serde_json::Value {
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
            Value::Time(t) => serde_json::Value::String(t.to_rfc3339()),
            Value::EnumVariant {
                enum_name,
                variant_name,
            } => serde_json::Value::String(format!("{}.{}", enum_name, variant_name)),
            Value::Array(a) => {
                serde_json::Value::Array(a.into_iter().map(|v| self.vibe_to_json(v)).collect())
            }
            Value::Hash(h) => {
                let mut map = serde_json::Map::new();
                for (k, v) in h {
                    map.insert(k, self.vibe_to_json(v));
                }
                serde_json::Value::Object(map)
            }
            Value::Block { .. } => serde_json::Value::Null,
            Value::Class(c) => serde_json::Value::String(format!("<class {}>", c.name)),
            Value::Instance(i) => {
                serde_json::Value::String(format!("<instance of {}>", i.read().unwrap().class.name))
            }
        }
    }

    fn eval_class_def(&mut self, name: &str, body: &[Stmt]) -> Result<(), String> {
        let mut class_methods = HashMap::new();
        let mut methods = HashMap::new();

        for stmt in body {
            match stmt {
                Stmt::Function(f) => {
                    let def = FunctionDef {
                        params: f.params.clone(),
                        body: f.body.clone(),
                        is_private: f.is_private,
                    };
                    if f.is_class_method {
                        class_methods.insert(f.name.clone(), def);
                    } else {
                        methods.insert(f.name.clone(), def);
                    }
                }
                Stmt::PropertyDecl { names, kind } => {
                    for prop_name in names {
                        match kind {
                            PropertyKind::Property => {
                                methods.insert(prop_name.clone(), self.make_getter(prop_name));
                                methods
                                    .insert(format!("{}=", prop_name), self.make_setter(prop_name));
                            }
                            PropertyKind::Getter => {
                                methods.insert(prop_name.clone(), self.make_getter(prop_name));
                            }
                            PropertyKind::Setter => {
                                methods
                                    .insert(format!("{}=", prop_name), self.make_setter(prop_name));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let class_def = Arc::new(ClassDef {
            name: name.to_string(),
            methods,
            class_methods,
            class_vars: RwLock::new(HashMap::new()),
        });

        self.classes.insert(name.to_string(), class_def);
        Ok(())
    }

    fn make_getter(&self, name: &str) -> FunctionDef {
        FunctionDef {
            params: vec![],
            body: vec![Stmt::Expression(Expr::InstanceVar(name.to_string()))],
            is_private: false,
        }
    }

    fn make_setter(&self, name: &str) -> FunctionDef {
        FunctionDef {
            params: vec![Param {
                name: "val".to_string(),
                is_ivar: false,
            }],
            body: vec![Stmt::IvarAssignment {
                name: name.to_string(),
                value: Expr::Variable("val".to_string()),
            }],
            is_private: false,
        }
    }

    fn eval_member_mut(
        &mut self,
        receiver: Value,
        method: &str,
        args: Vec<Value>,
        block: Option<Value>,
    ) -> Result<Value, String> {
        match (receiver.clone(), method, block) {
            (Value::Class(c), "new", _) => {
                let instance = Arc::new(RwLock::new(InstanceData {
                    class: c.clone(),
                    ivars: HashMap::new(),
                }));
                let inst_val = Value::Instance(instance);

                if let Some(init_fn) = c.methods.get("initialize") {
                    self.stack
                        .push(HashMap::from([("self".to_string(), inst_val.clone())]));
                    self.call_function_def(init_fn, args)?;
                    self.stack.pop();
                }
                Ok(inst_val)
            }
            (Value::Instance(inst), method_name, _) => {
                let class_def = inst.read().unwrap().class.clone();
                if let Some(fn_def) = class_def.methods.get(method_name) {
                    if fn_def.is_private {
                        let current_self = self.get_var("self");
                        let is_internal = if let Some(Value::Instance(curr)) = current_self {
                            Arc::ptr_eq(&curr, &inst)
                        } else {
                            false
                        };
                        if !is_internal {
                            return Err(format!("Method '{}' is private", method_name));
                        }
                    }

                    self.stack.push(HashMap::from([(
                        "self".to_string(),
                        Value::Instance(inst.clone()),
                    )]));
                    let res = self.call_function_def(fn_def, args)?;
                    self.stack.pop();
                    Ok(res)
                } else {
                    Err(format!(
                        "Method '{}' not found in class {}",
                        method_name, class_def.name
                    ))
                }
            }
            (Value::Array(arr), "length" | "size", _) => Ok(Value::Int(arr.len() as i64)),
            (Value::String(s), "length" | "size", _) => Ok(Value::Int(s.len() as i64)),
            (Value::Hash(h), "length" | "size", _) => Ok(Value::Int(h.len() as i64)),

            (_, "to_string", _) => Ok(Value::String(receiver.to_string())),

            (Value::String(s), "uppercase", _) => Ok(Value::String(s.to_uppercase())),
            (Value::String(s), "lowercase", _) => Ok(Value::String(s.to_lowercase())),
            (Value::String(s), "contains?" | "include?", _) => {
                if let Some(Value::String(pat)) = args.first() {
                    Ok(Value::Bool(s.contains(pat)))
                } else {
                    Err("contains? expects a string argument".to_string())
                }
            }
            (Value::String(s), "split", _) => {
                if let Some(Value::String(sep)) = args.first() {
                    let parts = s.split(sep).map(|p| Value::String(p.to_string())).collect();
                    Ok(Value::Array(parts))
                } else {
                    Err("split expects a string separator".to_string())
                }
            }

            (Value::Array(arr), "push", _) => {
                let mut new_arr = arr.clone();
                for arg in args {
                    new_arr.push(arg);
                }
                Ok(Value::Array(new_arr))
            }
            (Value::Array(arr), "pop", _) => {
                let mut new_arr = arr.clone();
                new_arr.pop();
                Ok(Value::Array(new_arr))
            }
            (Value::Array(arr), "include?" | "contains?", _) => {
                if let Some(val) = args.first() {
                    Ok(Value::Bool(arr.contains(val)))
                } else {
                    Err("include? expects an argument".to_string())
                }
            }
            (Value::Array(arr), "join", _) => {
                let sep = if let Some(Value::String(s)) = args.first() {
                    s.clone()
                } else {
                    "".to_string()
                };
                let joined = arr
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(&sep);
                Ok(Value::String(joined))
            }

            (Value::Hash(h), "keys", _) => {
                let keys = h.keys().map(|k| Value::String(k.clone())).collect();
                Ok(Value::Array(keys))
            }
            (Value::Hash(h), "values", _) => {
                let values = h.values().cloned().collect();
                Ok(Value::Array(values))
            }
            (Value::Hash(h), "has_key?" | "key?", _) => {
                if let Some(Value::String(k)) = args.first() {
                    Ok(Value::Bool(h.contains_key(k)))
                } else {
                    Err("has_key? expects a string key".to_string())
                }
            }
            (Value::Hash(h), "merge", _) => {
                if let Some(Value::Hash(other)) = args.first() {
                    let mut new_hash = h.clone();
                    for (k, v) in other {
                        new_hash.insert(k.clone(), v.clone());
                    }
                    Ok(Value::Hash(new_hash))
                } else {
                    Err("merge expects a hash argument".to_string())
                }
            }

            // Collection Pipelines
            (Value::Array(arr), "each", Some(Value::Block { params, body })) => {
                let mut last_val = Value::Nil;
                for item in arr {
                    let mut scope = HashMap::new();
                    if let Some(param) = params.first() {
                        scope.insert(param.clone(), item);
                    }
                    self.stack.push(scope);
                    let res = self.eval_block(&body);
                    self.stack.pop();
                    match res? {
                        ControlFlow::Break(v) => {
                            last_val = v;
                            break;
                        }
                        ControlFlow::Return(v) => return Ok(v),
                        ControlFlow::Next(v) => {
                            last_val = v;
                        }
                        ControlFlow::Continue(v) => {
                            last_val = v;
                        }
                    }
                }
                Ok(last_val)
            }
            (Value::Array(arr), "map" | "select", Some(Value::Block { params, body })) => {
                if method == "map" {
                    let mut results = Vec::new();
                    for item in arr {
                        let mut scope = HashMap::new();
                        if let Some(param) = params.first() {
                            scope.insert(param.clone(), item);
                        }
                        self.stack.push(scope);
                        let res = self.eval_block(&body);
                        self.stack.pop();
                        match res? {
                            ControlFlow::Break(v) => {
                                results.push(v);
                                break;
                            }
                            ControlFlow::Return(v) => return Ok(v),
                            ControlFlow::Next(v) => {
                                results.push(v);
                            }
                            ControlFlow::Continue(v) => {
                                results.push(v);
                            }
                        }
                    }
                    Ok(Value::Array(results))
                } else {
                    // "select" (filter)
                    let mut results = Vec::new();
                    for item in arr {
                        let mut scope = HashMap::new();
                        if let Some(param) = params.first() {
                            scope.insert(param.clone(), item.clone());
                        }
                        self.stack.push(scope);
                        let res = self.eval_block(&body);
                        self.stack.pop();
                        match res? {
                            ControlFlow::Break(v) => {
                                if self.is_truthy(&v) {
                                    results.push(item);
                                }
                                break;
                            }
                            ControlFlow::Return(v) => return Ok(v),
                            ControlFlow::Next(v) => {
                                if self.is_truthy(&v) {
                                    results.push(item);
                                }
                            }
                            ControlFlow::Continue(v) => {
                                if self.is_truthy(&v) {
                                    results.push(item);
                                }
                            }
                        }
                    }
                    Ok(Value::Array(results))
                }
            }
            (Value::Array(arr), "reduce", Some(Value::Block { params, body })) => {
                if arr.is_empty() && args.is_empty() {
                    return Err("reduce on empty array requires initial value".to_string());
                }
                let mut acc = if !args.is_empty() {
                    args[0].clone()
                } else {
                    arr[0].clone()
                };
                let start = if !args.is_empty() { 0 } else { 1 };

                for i in start..arr.len() {
                    let mut scope = HashMap::new();
                    if params.len() >= 2 {
                        scope.insert(params[0].clone(), acc);
                        scope.insert(params[1].clone(), arr[i].clone());
                    }
                    self.stack.push(scope);
                    let res = self.eval_block(&body);
                    self.stack.pop();
                    match res? {
                        ControlFlow::Break(v) => {
                            acc = v;
                            break;
                        }
                        ControlFlow::Return(v) => return Ok(v),
                        ControlFlow::Next(v) => {
                            acc = v;
                        }
                        ControlFlow::Continue(v) => {
                            acc = v;
                        }
                    }
                }
                Ok(acc)
            }

            _ => Err(format!("Method '{}' not supported for this type", method)),
        }
    }

    fn call_function(&mut self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let fn_def = self
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Function '{}' not found", name))?;

        if fn_def.is_private {
            return Err(format!("Function '{}' is private", name));
        }

        self.call_function_def(&fn_def, args)
    }

    fn call_function_def(&mut self, func: &FunctionDef, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != func.params.len() {
            return Err(format!(
                "Expected {} args, got {}",
                func.params.len(),
                args.len()
            ));
        }

        let mut new_scope = HashMap::new();
        for (param, val) in func.params.iter().zip(args) {
            new_scope.insert(param.name.clone(), val.clone());
            // If the parameter is an ivar, set it on self
            if param.is_ivar {
                self.stack.push(new_scope.clone()); // Temporary push to have 'self' if it exists
                self.set_ivar(&param.name, val.clone())?;
                new_scope = self.stack.pop().unwrap();
            }
        }

        self.stack.push(new_scope);
        let result = self.eval_block(&func.body);
        self.stack.pop();

        match result? {
            ControlFlow::Return(v) => Ok(v),
            ControlFlow::Break(v) => Ok(v),
            ControlFlow::Next(v) => Ok(v),
            ControlFlow::Continue(v) => Ok(v),
        }
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
            (Value::Array(arr), BinaryOp::Index, Value::Int(i)) => {
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
            (Value::Hash(hash), BinaryOp::Index, Value::String(s)) => {
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

            // String concatenation support
            (Value::String(l), BinaryOp::Add, Value::String(r)) => {
                Ok(Value::String(format!("{}{}", l, r)))
            }

            _ => Err("Binary operation not supported".to_string()),
        }
    }
}
