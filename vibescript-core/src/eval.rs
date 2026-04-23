use crate::ast::*;
use crate::value::Value;
use chrono::Utc;
use std::collections::HashMap;
use uuid::Uuid;

pub struct Engine {
    globals: HashMap<String, Value>,
    functions: HashMap<String, FunctionDef>,
    stack: Vec<HashMap<String, Value>>,
}

struct FunctionDef {
    params: Vec<String>,
    body: Vec<Stmt>,
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
            Stmt::Function { name, params, body } => {
                self.functions.insert(
                    name.clone(),
                    FunctionDef {
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
                Ok(ControlFlow::Continue(Value::Nil))
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
            Expr::Variable(name) => self
                .get_var(name)
                .ok_or_else(|| format!("Variable '{}' not found", name)),
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
                    _ => self.call_function(func, arg_vals),
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
            Value::Time(t) => serde_json::Value::String(t.to_string()),
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
        }
    }
    fn eval_member_mut(
        &mut self,
        receiver: Value,
        method: &str,
        args: Vec<Value>,
        block: Option<Value>,
    ) -> Result<Value, String> {
        match (receiver, method, block) {
            (Value::Array(arr), "length", _) => Ok(Value::Int(arr.len() as i64)),
            (Value::String(s), "length", _) => Ok(Value::Int(s.len() as i64)),
            (Value::Hash(h), "length", _) => Ok(Value::Int(h.len() as i64)),
            (Value::String(s), "uppercase", _) => Ok(Value::String(s.to_uppercase())),
            (Value::String(s), "lowercase", _) => Ok(Value::String(s.to_lowercase())),
            (Value::String(s), "split", _) => {
                if let Some(Value::String(sep)) = args.first() {
                    let parts = s.split(sep).map(|p| Value::String(p.to_string())).collect();
                    Ok(Value::Array(parts))
                } else {
                    Err("split expects a string separator".to_string())
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
            (Value::Array(arr), "map", Some(Value::Block { params, body })) => {
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
            }
            _ => Err(format!("Method '{}' not supported for this type", method)),
        }
    }

    fn call_function(&mut self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let (params, body) = {
            let func = self
                .functions
                .get(name)
                .ok_or_else(|| format!("Function '{}' not found", name))?;

            if args.len() != func.params.len() {
                return Err(format!(
                    "Function '{}' expected {} args, got {}",
                    name,
                    func.params.len(),
                    args.len()
                ));
            }
            (func.params.clone(), func.body.clone())
        };

        let mut new_scope = HashMap::new();
        for (param, val) in params.iter().zip(args) {
            new_scope.insert(param.clone(), val);
        }

        self.stack.push(new_scope);
        let result = self.eval_block(&body);
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
            _ => Err("Binary operation not supported".to_string()),
        }
    }
}
