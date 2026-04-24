#![allow(unsafe_op_in_unsafe_fn)]

pub mod ast;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod value;

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::prelude::*;
use logos::Logos;

// Generate the WASM Component bindings - only when targeting WASM
#[cfg(target_arch = "wasm32")]
wit_bindgen::generate!({
    world: "vibes-provider",
    path: "wit/vibes.wit",
});

#[cfg(target_arch = "wasm32")]
use exports::xipkit::vibes::engine_world::{
    EngineConfig, Guest, GuestEngine, GuestScript, NamedValue, Value as WitValue,
};

#[cfg(target_arch = "wasm32")]
struct MyEngine;

#[cfg(target_arch = "wasm32")]
impl Guest for MyEngine {
    type Engine = MyEngineWrapper;
    type Script = MyScriptWrapper;
}

#[cfg(target_arch = "wasm32")]
struct MyEngineWrapper {
    _engine: eval::Engine,
}

#[cfg(target_arch = "wasm32")]
struct MyScriptWrapper {
    stmts: Vec<ast::Stmt>,
}

#[cfg(target_arch = "wasm32")]
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
        let (tokens, spans) = lex_with_spans(&source);

        let stmts = parser::parser()
            .parse(tokens.as_slice())
            .into_result()
            .map_err(|e| format_parse_errors(&source, e, &spans))?;

        let script = MyScriptWrapper { stmts };
        Ok(exports::xipkit::vibes::engine_world::Script::new(script))
    }
}

#[cfg(target_arch = "wasm32")]
impl GuestScript for MyScriptWrapper {
    fn call(
        &self,
        func_name: String,
        args: Vec<WitValue>,
        _kwargs: Vec<NamedValue>,
    ) -> Result<WitValue, String> {
        let mut engine = eval::Engine::new();

        // 1. Evaluate top-level statements to define functions/classes
        let mut last_val = value::Value::Nil;
        for stmt in &self.stmts {
            match engine.eval_stmt(stmt) {
                Ok(cf) => {
                    last_val = cf.value();
                    if !cf.is_continue() {
                        break;
                    }
                }
                Err(eval::EvalError::Message(m)) => return Err(m),
            }
        }

        // 2. If a function name is provided, invoke it
        if !func_name.is_empty() {
            let vibe_args: Vec<value::Value> =
                args.into_iter().map(|w| wit_to_vibe(&engine, w)).collect();
            let res = engine.call_function(&func_name, vibe_args)?;
            return Ok(vibe_to_wit(&engine, res));
        }

        // 3. Otherwise return the last value from top-level execution
        Ok(vibe_to_wit(&engine, last_val))
    }
}

#[cfg(target_arch = "wasm32")]
fn wit_to_vibe(engine: &eval::Engine, w: WitValue) -> value::Value {
    match w {
        WitValue::I(i) => value::Value::Int(i),
        WitValue::F(f) => value::Value::Float(f),
        WitValue::S(s) => value::Value::String(s),
        WitValue::B(b) => value::Value::Bool(b),
        WitValue::Json(s) => {
            if let Ok(json_val) = serde_json::from_str(&s) {
                engine.json_to_vibe(json_val)
            } else {
                value::Value::String(s)
            }
        }
        WitValue::None => value::Value::Nil,
    }
}

#[cfg(target_arch = "wasm32")]
fn vibe_to_wit(engine: &eval::Engine, v: value::Value) -> WitValue {
    match v {
        value::Value::Int(i) => WitValue::I(i),
        value::Value::Float(f) => WitValue::F(f),
        value::Value::String(s) => WitValue::S(s),
        value::Value::Symbol(s) => WitValue::S(format!(":{}", s)),
        value::Value::Bool(b) => WitValue::B(b),
        value::Value::Nil => WitValue::None,
        value::Value::Time(t) => WitValue::S(t.to_rfc3339()),
        value::Value::EnumVariant {
            enum_name,
            variant_name,
        } => WitValue::S(format!("{}.{}", enum_name, variant_name)),
        value::Value::Array(_) | value::Value::Hash(_) => {
            let json_val = engine.vibe_to_json(v);
            if let Ok(s) = serde_json::to_string(&json_val) {
                WitValue::Json(s)
            } else {
                WitValue::None
            }
        }
        _ => WitValue::None,
    }
}

fn lex_with_spans(source: &str) -> (Vec<lexer::Token>, Vec<SimpleSpan>) {
    let mut tokens = Vec::new();
    let mut spans = Vec::new();

    for (token, span) in lexer::Token::lexer(source).spanned() {
        tokens.push(token.unwrap_or(lexer::Token::Nil));
        spans.push(span.into());
    }

    (tokens, spans)
}

fn format_parse_errors(
    source: &str,
    errors: Vec<Rich<lexer::Token>>,
    spans: &[SimpleSpan],
) -> String {
    let mut reports = Vec::new();

    for error in errors {
        let token_span = error.span();
        let start_byte = spans
            .get(token_span.start)
            .map(|s| s.start)
            .unwrap_or(source.len());
        let mut end_byte = spans
            .get(token_span.end.saturating_sub(1))
            .map(|s| s.end)
            .unwrap_or(source.len());

        if end_byte < start_byte {
            end_byte = start_byte;
        }

        let report = Report::build(ReportKind::Error, (), start_byte)
            .with_message(format!("{}", error.reason()))
            .with_label(
                Label::new(start_byte..end_byte)
                    .with_message(format!("Error: {}", error.reason()))
                    .with_color(Color::Red),
            )
            .finish();

        let mut buf = Vec::new();
        report.write(Source::from(source), &mut buf).unwrap();
        reports.push(String::from_utf8_lossy(&buf).to_string());
    }

    reports.join("\n")
}

pub fn execute(source: &str) -> Result<value::Value, String> {
    let (tokens, spans) = lex_with_spans(source);

    let stmts = parser::parser()
        .parse(tokens.as_slice())
        .into_result()
        .map_err(|e| format_parse_errors(source, e, &spans))?;

    let mut engine = eval::Engine::new();
    let mut last_val = value::Value::Nil;

    for stmt in stmts {
        match engine.eval_stmt(&stmt) {
            Ok(cf) => {
                last_val = cf.value();
                if !cf.is_continue() {
                    break;
                }
            }
            Err(eval::EvalError::Message(m)) => return Err(m),
        }
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
            Value::new_array(vec![Value::Int(2), Value::Int(4), Value::Int(6)])
        );

        let source = "[1, 2, 3, 4].select do |x| x > 2 end";
        let result = execute(source).unwrap();
        assert_eq!(Value::new_array(vec![Value::Int(3), Value::Int(4)]), result);

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
            Value::new_array(vec![
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
        assert_eq!(result, Value::new_hash(expected_hash));
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

    #[test]
    fn test_interpolation() {
        let source = "name = \"Gwen\"\n\"Hello #{name}!\"";
        let result = execute(source).unwrap();
        assert!(result.to_string().contains("name"));
    }

    #[test]
    fn test_enums() {
        let source = "
            enum Color
                Red
                Green
                Blue
            end
            Color.Red
        ";
        let result = execute(source).unwrap();
        assert_eq!(
            result,
            Value::EnumVariant {
                enum_name: "Color".to_string(),
                variant_name: "Red".to_string(),
            }
        );
    }

    #[test]
    fn test_classes() {
        let source = "
            class User
                def initialize(@name, @age)
                end

                def greet
                    return \"Hello, I am \" + @name + \" and I am \" + @age.to_string() + \" years old.\"
                end

                def birthday
                    @age = @age + 1
                end
            end

            u = User.new(\"Gwen\", 25)
            u.birthday()
            u.greet()
        ";
        let result = execute(source).unwrap();
        assert_eq!(
            result,
            Value::String("Hello, I am Gwen and I am 26 years old.".to_string())
        );

        // Test multiple instances and state isolation
        let source = "
            class Counter
                def initialize(@count)
                end
                def inc
                    @count = @count + 1
                end
                def val
                    return @count
                end
            end

            c1 = Counter.new(10)
            c2 = Counter.new(20)
            c1.inc()
            c1.val() + c2.val()
        ";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(31));
    }

    #[test]
    fn test_advanced_classes() {
        let source = "
            class Account
                property balance

                def initialize(@balance)
                end

                private def secret
                    return \"42\"
                end

                def get_secret
                    return secret()
                end
            end

            a = Account.new(100)
            a.balance = a.balance() + 50

            # This should work (internal call)
            s = a.get_secret()

            a.balance()
        ";
        let result = execute(source).unwrap();
        assert_eq!(result, Value::Int(150));

        // Test privacy violation
        let source = "
            class Account
                private def secret
                    return \"42\"
                end
            end
            a = Account.new()
            a.secret()
        ";
        let res = execute(source);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("private"));
    }

    #[test]
    fn test_parse_errors() {
        let source = "if true\n  x = 1\n# missing end";
        let res = execute(source);
        assert!(res.is_err());
        let err = res.unwrap_err();
        println!("{}", err);
        // Check that it looks like an Ariadne report
        assert!(err.contains("Error"));
        assert!(err.contains("|"));
    }

    #[test]
    fn test_array_mutation() {
        let source = "
            arr = [1, 2]
            other = arr
            arr.push(3)
            other[0] = 10
            arr
        ";
        let result = execute(source).unwrap();
        assert_eq!(
            result,
            Value::new_array(vec![Value::Int(10), Value::Int(2), Value::Int(3)])
        );
    }
}

#[cfg(target_arch = "wasm32")]
export!(MyEngine);
