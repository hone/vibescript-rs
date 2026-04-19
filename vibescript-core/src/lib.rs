#![allow(unsafe_op_in_unsafe_fn)]

pub mod ast;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod value;

use logos::Logos;
use chumsky::Parser;

// Generate the WASM Component bindings
wit_bindgen::generate!({
    world: "vibes-provider",
    path: "wit/vibes.wit",
});

use exports::xipkit::vibes::engine_world::{
    Guest, GuestEngine, GuestScript, EngineConfig, NamedValue, Value as WitValue
};

struct MyEngine;

impl Guest for MyEngine {
    type Engine = MyEngineWrapper;
    type Script = MyScriptWrapper;
}

struct MyEngineWrapper {
    _engine: eval::Engine,
}

struct MyScriptWrapper {
    stmts: Vec<ast::Stmt>,
}

impl GuestEngine for MyEngineWrapper {
    fn new(_cfg: EngineConfig) -> Self {
        Self {
            _engine: eval::Engine::new(),
        }
    }

    fn compile(&self, source: String) -> Result<exports::xipkit::vibes::engine_world::Script, String> {
        let tokens: Vec<_> = lexer::Token::lexer(&source)
            .map(|t| t.unwrap_or(lexer::Token::Nil))
            .collect();

        let stmts = parser::parser()
            .parse(&tokens)
            .into_result()
            .map_err(|e| format!("Parse error: {:?}", e))?;

        let script = MyScriptWrapper { stmts };
        Ok(exports::xipkit::vibes::engine_world::Script::new(script))
    }
}

impl GuestScript for MyScriptWrapper {
    fn call(&self, _func_name: String, _args: Vec<WitValue>, _kwargs: Vec<NamedValue>) -> Result<WitValue, String> {
        let mut engine = eval::Engine::new();
        let mut last_val = value::Value::Nil;

        for stmt in &self.stmts {
            last_val = engine.eval_stmt(stmt)?;
        }

        Ok(vibe_to_wit(last_val))
    }
}

fn vibe_to_wit(v: value::Value) -> WitValue {
    match v {
        value::Value::Int(i) => WitValue::I(i),
        value::Value::Float(f) => WitValue::F(f),
        value::Value::String(s) => WitValue::S(s),
        value::Value::Bool(b) => WitValue::B(b),
        value::Value::Nil => WitValue::None,
        _ => WitValue::None, 
    }
}

pub fn execute(source: &str) -> Result<value::Value, String> {
    let tokens: Vec<_> = lexer::Token::lexer(source)
        .map(|t| t.unwrap_or(lexer::Token::Nil)) 
        .collect();

    let stmts = parser::parser()
        .parse(&tokens)
        .into_result()
        .map_err(|e| format!("Parse error: {:?}", e))?;

    let mut engine = eval::Engine::new();
    let mut last_val = value::Value::Nil;

    for stmt in stmts {
        last_val = engine.eval_stmt(&stmt)?;
    }

    Ok(last_val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn test_basic_arithmetic() {
        let source = "1 + 2 * 3";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(7));
    }

    #[test]
    fn test_variable_assignment() {
        let source = "x = 10\ny = 20\nx + y";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(30));
    }

    #[test]
    fn test_floor_division() {
        // Match Go's behavior: -7 / 2 = -4
        let source = "-7 / 2";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(-4));
    }

    #[test]
    fn test_float_division() {
        let source = "7.0 / 2";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Float(3.5));
    }
}

export!(MyEngine);
