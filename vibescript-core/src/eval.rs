use crate::ast::*;
use crate::value::Value;
use std::collections::HashMap;

pub struct Engine {
    env: HashMap<String, Value>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            env: HashMap::new(),
        }
    }

    pub fn eval_stmt(&mut self, stmt: &Stmt) -> Result<Value, String> {
        match stmt {
            Stmt::Expression(expr) => self.eval_expr(expr),
            Stmt::Assignment { name, value } => {
                let val = self.eval_expr(value)?;
                self.env.insert(name.clone(), val.clone());
                Ok(val)
            }
            _ => Err("Statement not yet implemented in MVP".to_string()),
        }
    }

    pub fn eval_expr(&self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Literal(val) => Ok(val.clone()),
            Expr::Variable(name) => self
                .env
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Variable '{}' not found", name)),
            Expr::Unary { op, expr } => {
                let val = self.eval_expr(expr)?;
                self.eval_unary(*op, val)
            }
            Expr::Binary { left, op, right } => {
                let lhs = self.eval_expr(left)?;
                let rhs = self.eval_expr(right)?;
                self.eval_binary(lhs, *op, rhs)
            }
            _ => Err("Expression not yet implemented in MVP".to_string()),
        }
    }

    fn eval_unary(&self, op: UnaryOp, val: Value) -> Result<Value, String> {
        match (op, val) {
            (UnaryOp::Neg, Value::Int(i)) => Ok(Value::Int(-i)),
            (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
            (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            _ => Err("Unary operation not supported for this type".to_string()),
        }
    }

    fn eval_binary(&self, lhs: Value, op: BinaryOp, rhs: Value) -> Result<Value, String> {
        match (lhs, op, rhs) {
            // Int + Int
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

            // Float + Float (or mixed)
            (Value::Float(l), BinaryOp::Add, Value::Float(r)) => Ok(Value::Float(l + r)),
            (Value::Float(l), BinaryOp::Add, Value::Int(r)) => Ok(Value::Float(l + r as f64)),
            (Value::Int(l), BinaryOp::Add, Value::Float(r)) => Ok(Value::Float(l as f64 + r)),

            (Value::Float(l), BinaryOp::Div, Value::Int(r)) => Ok(Value::Float(l / r as f64)),
            (Value::Int(l), BinaryOp::Div, Value::Float(r)) => Ok(Value::Float(l as f64 / r)),
            (Value::Float(l), BinaryOp::Div, Value::Float(r)) => Ok(Value::Float(l / r)),

            // Add more cases as needed for MVP
            _ => Err("Binary operation not supported for these types".to_string()),
        }
    }
}
