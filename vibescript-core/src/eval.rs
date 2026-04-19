use crate::ast::*;
use crate::value::Value;
use std::collections::HashMap;

pub struct Engine {
    globals: HashMap<String, Value>,
    functions: HashMap<String, FunctionDef>,
    stack: Vec<HashMap<String, Value>>,
}

struct FunctionDef {
    params: Vec<String>,
    body: Vec<Stmt>,
}

enum EvalResult {
    Value(Value),
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
            EvalResult::Value(v) => Ok(v),
            EvalResult::Return(v) => Ok(v),
        }
    }

    fn eval_stmt_internal(&mut self, stmt: &Stmt) -> Result<EvalResult, String> {
        match stmt {
            Stmt::Expression(expr) => Ok(EvalResult::Value(self.eval_expr_mut(expr)?)),
            Stmt::Assignment { name, value } => {
                let val = self.eval_expr_mut(value)?;
                self.set_var(name, val.clone());
                Ok(EvalResult::Value(val))
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_val = self.eval_expr_mut(condition)?;
                if self.is_truthy(&cond_val) {
                    self.eval_block(then_branch)
                } else if let Some(else_b) = else_branch {
                    self.eval_block(else_b)
                } else {
                    Ok(EvalResult::Value(Value::Nil))
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
                        EvalResult::Return(v) => return Ok(EvalResult::Return(v)),
                        EvalResult::Value(v) => last_val = v,
                    }
                }
                Ok(EvalResult::Value(last_val))
            }
            Stmt::Function { name, params, body } => {
                self.functions.insert(
                    name.clone(),
                    FunctionDef {
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
                Ok(EvalResult::Value(Value::Nil))
            }
            Stmt::Return(expr) => {
                let val = if let Some(e) = expr {
                    self.eval_expr_mut(e)?
                } else {
                    Value::Nil
                };
                Ok(EvalResult::Return(val))
            }
        }
    }

    fn eval_block(&mut self, stmts: &[Stmt]) -> Result<EvalResult, String> {
        let mut last_val = Value::Nil;
        for stmt in stmts {
            match self.eval_stmt_internal(stmt)? {
                EvalResult::Return(v) => return Ok(EvalResult::Return(v)),
                EvalResult::Value(v) => last_val = v,
            }
        }
        Ok(EvalResult::Value(last_val))
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
                let lhs = self.eval_expr_mut(left)?;
                let rhs = self.eval_expr_mut(right)?;
                self.eval_binary(lhs, *op, rhs)
            }
            Expr::Call { func, args, .. } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr_mut(arg)?);
                }
                self.call_function(func, arg_vals)
            }
            _ => Err("Expression not yet implemented".to_string()),
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

        // New scope
        let mut new_scope = HashMap::new();
        for (param, val) in params.iter().zip(args) {
            new_scope.insert(param.clone(), val);
        }

        self.stack.push(new_scope);
        let result = self.eval_block(&body);
        self.stack.pop();

        match result? {
            EvalResult::Value(v) => Ok(v),
            EvalResult::Return(v) => Ok(v),
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
            (Value::Float(l), BinaryOp::Add, Value::Float(r)) => Ok(Value::Float(l + r)),
            (Value::Float(l), BinaryOp::Add, Value::Int(r)) => Ok(Value::Float(l + r as f64)),
            (Value::Int(l), BinaryOp::Add, Value::Float(r)) => Ok(Value::Float(l as f64 + r)),
            (Value::Float(l), BinaryOp::Div, Value::Int(r)) => Ok(Value::Float(l / r as f64)),
            (Value::Int(l), BinaryOp::Div, Value::Float(r)) => Ok(Value::Float(l as f64 / r)),
            (Value::Float(l), BinaryOp::Div, Value::Float(r)) => Ok(Value::Float(l / r)),
            _ => Err("Binary operation not supported".to_string()),
        }
    }
}
