#![allow(unsafe_op_in_unsafe_fn)]

pub mod ast;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod value;
use chumsky::Parser;
use logos::Logos;
// Generate the WASM Component bindings
wit_bindgen::generate!({
    world: "vibes-provider",
    path: "wit/vibes.wit",
});

use exports::xipkit::vibes::engine_world::{
    EngineConfig, Guest, GuestEngine, GuestScript, NamedValue, Value as WitValue,
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

    fn compile(
        &self,
        source: String,
    ) -> Result<exports::xipkit::vibes::engine_world::Script, String> {
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
    fn call(
        &self,
        _func_name: String,
        _args: Vec<WitValue>,
        _kwargs: Vec<NamedValue>,
    ) -> Result<WitValue, String> {
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
        value::Value::Time(t) => WitValue::S(t.to_rfc3339()),
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

    #[test]
    fn test_if_else() {
        let source = "x = 10\nif x > 5\n  y = 1\nelse\n  y = 2\nend\ny";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(1));

        let source = "x = 3\nif x > 5\n  y = 1\nelse\n  y = 2\nend\ny";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(2));
    }

    #[test]
    fn test_elsif() {
        let source = "
            x = 7
            if x == 1
                y = 1
            elsif x == 7
                y = 7
            else
                y = 0
            end
            y
        ";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(7));
    }

    #[test]
    fn test_member_methods() {
        let source = "arr = [1, 2, 3]\narr.length()";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(3));

        let source = "\"gwen\".uppercase()";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::String("GWEN".to_string()));
    }

    #[test]
    fn test_while_loop() {
        let source = "i = 0\nsum = 0\nwhile i < 5\n  i = i + 1\n  sum = sum + i\nend\nsum";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(15));
    }

    #[test]
    fn test_until_loop() {
        let source = "i = 0\nuntil i == 5\n  i = i + 1\nend\ni";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(5));
    }
    #[test]
    fn test_for_loop() {
        let source = "sum = 0\nfor x in [1, 2, 3]\n  sum = sum + x\nend\nsum";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(6));
    }
    #[test]
    fn test_functions() {
        let source = "def add(a, b)\n  return a + b\nend\nadd(10, 20)";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(30));
    }

    #[test]
    fn test_recursion() {
        let source = "def fib(n)\n  if n <= 1\n    return n\n  end\n  return fib(n - 1) + fib(n - 2)\nend\nfib(7)";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(13));
    }

    #[test]
    fn test_collections() {
        let source = "arr = [1, 2, 3]\narr[1]";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(2));

        let source = "h = {name: \"Gwen\", age: 25}\nh[\"name\"]";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::String("Gwen".to_string()));
    }

    #[test]
    fn test_complex_flow() {
        let source = "
            def process(items)
                total = 0
                i = 0
                while i < 3
                    total = total + items[i]
                    i = i + 1
                end
                return total
            end
            process([10, 20, 30])
        ";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(60));
    }

    #[test]
    fn test_logical_operators() {
        let source = "true && false";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Bool(false));

        let source = "true || false";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_error_handling() {
        let source = "
            x = 0
            begin
                1 / 0
            rescue
                x = 1
            ensure
                x = x + 10
            end
            x
        ";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(11));
    }

    #[test]
    fn test_collection_pipelines() {
        let source = "[1, 2, 3].map do |x| x * 2 end";
        let result = execute(source).unwrap();
        assert_eq!(
            result,
            Value::Array(vec![Value::Int(2), Value::Int(4), Value::Int(6)])
        );

        let source = "[1, 2, 3, 4].select do |x| x > 2 end";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Array(vec![Value::Int(3), Value::Int(4)]));

        let source = "[1, 2, 3].reduce(10) do |acc, x| acc + x end";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(16));
    }

    #[test]
    fn test_extended_stdlib() {
        // String
        let source = "\"hello\".contains?(\"ell\")";
        assert_eq!(execute(source).unwrap(), Value::Bool(true));

        let source = "\"abc\" + \"def\"";
        assert_eq!(
            execute(source).unwrap(),
            Value::String("abcdef".to_string())
        );

        // Array
        let source = "[1, 2].push(3, 4)";
        assert_eq!(
            execute(source).unwrap(),
            Value::Array(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4)
            ])
        );

        let source = "[1, 2, 3].include?(2)";
        assert_eq!(execute(source).unwrap(), Value::Bool(true));

        let source = "[1, 2, 3].join(\"-\")";
        assert_eq!(execute(source).unwrap(), Value::String("1-2-3".to_string()));

        // Hash
        let source = "h = {a: 1}.merge({b: 2})\nh.length()";
        assert_eq!(execute(source).unwrap(), Value::Int(2));

        let source = "{a: 1, b: 2}.keys().length()";
        assert_eq!(execute(source).unwrap(), Value::Int(2));
    }

    #[test]
    fn test_builtins() {
        let source = "json_parse(\"{\\\"a\\\": 1}\")";
        let result = execute(source).unwrap();
        let mut expected_hash = std::collections::HashMap::new();
        expected_hash.insert("a".to_string(), Value::Int(1));
        assert_eq!(result, Value::Hash(expected_hash));
        let source = "uuid()";
        let result = execute(source).unwrap();
        if let Value::String(s) = result {
            assert_eq!(s.len(), 36);
        } else {
            panic!("Expected string for uuid");
        }
        let source = "now()";
        let result = execute(source).unwrap();
        assert!(matches!(result, Value::Time(_)));
    }

    #[test]
    fn test_comments() {
        let source = "
            # This is a comment
            x = 10 # another comment
            # final comment
            x
        ";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(10));
    }
}

export!(MyEngine);
