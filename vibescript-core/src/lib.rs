#![allow(unsafe_op_in_unsafe_fn)]

pub mod ast;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod value;

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::prelude::*;

// Generate the WASM Component bindings - only when targeting WASM
#[cfg(target_arch = "wasm32")]
wit_bindgen::generate!({
    world: "engine",
    path: "../wit/vibes.wit",
});

#[cfg(target_arch = "wasm32")]
use exports::xipkit::vibes::engine_world::{
    EngineConfig, Guest, GuestEngine, GuestScript, NamedValue, Script, Value,
};

#[cfg(target_arch = "wasm32")]
use xipkit::vibes::loader;

#[cfg(target_arch = "wasm32")]
struct WasmModuleResolver;

#[cfg(target_arch = "wasm32")]
impl eval::ModuleResolver for WasmModuleResolver {
    fn load_module(
        &self,
        name: &str,
        caller_path: Option<&str>,
    ) -> Result<(String, String), String> {
        loader::load_module(name, caller_path)
    }
}

#[cfg(target_arch = "wasm32")]
struct VibeCompiler;

#[cfg(target_arch = "wasm32")]
impl eval::Compiler for VibeCompiler {
    fn compile(&self, source: &str) -> Result<Vec<ast::Stmt>, String> {
        let (tokens, spans) = lex_with_spans(source);
        parser::parser()
            .parse(tokens.as_slice())
            .into_result()
            .map_err(|e| format_parse_errors(source, e, &spans))
    }
}

#[cfg(target_arch = "wasm32")]
struct MyEngine;

#[cfg(target_arch = "wasm32")]
impl Guest for MyEngine {
    type Engine = MyEngineWrapper;
    type Script = MyScriptWrapper;
}

#[cfg(target_arch = "wasm32")]
struct MyEngineWrapper {
    engine: std::sync::Arc<std::sync::Mutex<eval::Engine>>,
}

#[cfg(target_arch = "wasm32")]
struct MyScriptWrapper {
    stmts: Vec<ast::Stmt>,
    engine: std::sync::Arc<std::sync::Mutex<eval::Engine>>,
}

#[cfg(target_arch = "wasm32")]
impl GuestEngine for MyEngineWrapper {
    fn new(_cfg: EngineConfig) -> Self {
        let mut engine = eval::Engine::new();
        engine.module_resolver = Some(std::sync::Arc::new(WasmModuleResolver));
        engine.compiler = Some(std::sync::Arc::new(VibeCompiler));

        Self {
            engine: std::sync::Arc::new(std::sync::Mutex::new(engine)),
        }
    }

    fn compile(&self, source: String) -> Result<Script, String> {
        let (tokens, spans) = lex_with_spans(&source);

        let stmts = parser::parser()
            .parse(tokens.as_slice())
            .into_result()
            .map_err(|e| format_parse_errors(&source, e, &spans))?;

        let script = MyScriptWrapper {
            stmts,
            engine: self.engine.clone(),
        };
        Ok(Script::new(script))
    }
}

#[cfg(target_arch = "wasm32")]
impl GuestScript for MyScriptWrapper {
    fn call(
        &self,
        func_name: String,
        args: Vec<Value>,
        _kwargs: Vec<NamedValue>,
    ) -> Result<Value, String> {
        let mut engine = self.engine.lock().unwrap();

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
                Err(e) => return Err(format!("{}: {}", e.kind, e.message)),
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
fn wit_to_vibe(engine: &eval::Engine, w: Value) -> value::Value {
    match w {
        Value::I(i) => value::Value::Int(i),
        Value::F(f) => value::Value::Float(f),
        Value::S(s) => value::Value::String(s),
        Value::B(b) => value::Value::Bool(b),
        Value::Json(j) => {
            let json: serde_json::Value =
                serde_json::from_str(&j).unwrap_or(serde_json::Value::Null);
            engine.json_to_vibe(json)
        }
        Value::None => value::Value::Nil,
    }
}

#[cfg(target_arch = "wasm32")]
fn vibe_to_wit(engine: &eval::Engine, v: value::Value) -> Value {
    match v {
        value::Value::Int(i) => Value::I(i),
        value::Value::Float(f) => Value::F(f),
        value::Value::String(s) => Value::S(s),
        value::Value::Bool(b) => Value::B(b),
        value::Value::Nil => Value::None,
        _ => Value::Json(engine.vibe_to_json(v).to_string()),
    }
}

fn lex_with_spans(source: &str) -> (Vec<lexer::Token>, Vec<SimpleSpan>) {
    let (tokens, ranges) = lexer::lex_with_spans(source);
    let spans = ranges.into_iter().map(SimpleSpan::from).collect();
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
        let mut start_byte = spans
            .get(token_span.start)
            .map(|s| s.start)
            .unwrap_or(source.len());
        let mut end_byte = spans
            .get(token_span.end.saturating_sub(1))
            .map(|s| s.end)
            .unwrap_or(source.len());

        if end_byte < start_byte {
            std::mem::swap(&mut start_byte, &mut end_byte);
        }
        if start_byte == end_byte {
            end_byte += 1;
        }
        if end_byte > source.len() {
            end_byte = source.len();
        }
        if start_byte >= end_byte && start_byte > 0 {
            start_byte = end_byte - 1;
        }

        let report = Report::build(ReportKind::Error, (), start_byte)
            .with_message(error.to_string())
            .with_label(
                Label::new(start_byte..end_byte)
                    .with_message(format!("{}", error.reason()))
                    .with_color(Color::Red),
            )
            .finish();

        let mut buf = Vec::new();
        report.write(Source::from(source), &mut buf).unwrap();
        reports.push(String::from_utf8_lossy(&buf).into_owned());
    }

    reports.join("\n")
}

pub fn execute(source: &str) -> Result<value::Value, String> {
    let mut engine = eval::Engine::new();
    let (tokens, spans) = lex_with_spans(source);

    let stmts = parser::parser()
        .parse(tokens.as_slice())
        .into_result()
        .map_err(|e| format_parse_errors(source, e, &spans))?;

    engine
        .eval_script(&stmts)
        .map_err(|e| format!("{}: {}", e.kind, e.message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value as VibeValue;

    #[test]
    fn test_basic_eval() {
        let source = "1 + 2";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(3));
    }

    #[test]
    fn test_basic_arithmetic() {
        let source = "
        def gcd(a, b)
          while b != 0
            next_value = a % b
            a = b
            b = next_value
          end
          a
        end

        def hailstone(n)
          out = [n]
          while n != 1
            if n % 2 == 0
              n = n / 2
            else
              n = n * 3 + 1
            end
            out = out + [n]
          end
          out
        end

        {
          simple: 1 + 2 * 3,
          int_div: 7 / 2,
          neg_div_left: -7 / 2,
          neg_div_right: 7 / -2,
          neg_div_both: -7 / -2,
          float_div: 7.0 / 2,
          mod_chain: 10 / 2 % 3,
          neg_mod_left: -7 % 2,
          neg_mod_right: 7 % -2,
          gcd: gcd(54, 24),
          hailstone_len: hailstone(7).length
        }
        ";
        let result = execute(source).unwrap();
        let hash = result.as_hash().unwrap();
        let h = hash.read().unwrap();

        assert_eq!(h.get("simple").unwrap(), &VibeValue::Int(7));
        assert_eq!(h.get("int_div").unwrap(), &VibeValue::Int(3));
        assert_eq!(h.get("neg_div_left").unwrap(), &VibeValue::Int(-4));
        assert_eq!(h.get("neg_div_right").unwrap(), &VibeValue::Int(-4));
        assert_eq!(h.get("neg_div_both").unwrap(), &VibeValue::Int(3));
        assert_eq!(h.get("float_div").unwrap(), &VibeValue::Float(3.5));
        assert_eq!(h.get("mod_chain").unwrap(), &VibeValue::Int(2));
        assert_eq!(h.get("neg_mod_left").unwrap(), &VibeValue::Int(1));
        assert_eq!(h.get("neg_mod_right").unwrap(), &VibeValue::Int(-1));
        assert_eq!(h.get("gcd").unwrap(), &VibeValue::Int(6));
        assert_eq!(h.get("hailstone_len").unwrap(), &VibeValue::Int(17));
    }

    #[test]
    fn test_variable_assignment() {
        let source = "x = 10\ny = 20\nx + y";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(30));
    }

    #[test]
    fn test_if_else() {
        let source = "x = 10\nif x > 5\n  y = 1\nelse\n  y = 2\nend\ny";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(1));

        let source = "x = 3\nif x > 5\n  y = 1\nelse\n  y = 2\nend\ny";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(2));
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
        assert_eq!(result, VibeValue::Int(7));
    }

    #[test]
    fn test_member_methods() {
        let source = "arr = [1, 2, 3]\narr.length";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(3));

        let source = "\"gwen\".uppercase()";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::String("GWEN".to_string()));
    }

    #[test]
    fn test_while_loop() {
        let source = "i = 0\nsum = 0\nwhile i < 5\n  i = i + 1\n  sum = sum + i\nend\nsum";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(15));
    }

    #[test]
    fn test_until_loop() {
        let source = "i = 0\nuntil i == 5\n  i = i + 1\nend\ni";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(5));
    }

    #[test]
    fn test_for_loop() {
        let source = "sum = 0\nfor x in [1, 2, 3]\n  sum = sum + x\nend\nsum";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(6));
    }

    #[test]
    fn test_functions() {
        let source = "def add(a, b)\n  return a + b\nend\nadd(10, 20)";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(30));
    }

    #[test]
    fn test_recursion() {
        let source = "def fib(n)\n  if n <= 1\n    return n\n  end\n  return fib(n - 1) + fib(n - 2)\nend\nfib(7)";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(13));
    }

    #[test]
    fn test_logical_operators() {
        let source = "
        def bad_index
          [1][4]
        end

        def explode
          raise \"boom\"
        end

        {
          and_short: false && bad_index(),
          or_short: true || explode(),
          complex: (true && 1 < 2) || false,
          simple_and: true && false,
          simple_or: true || false
        }
        ";
        let result = execute(source).unwrap();
        let hash = result.as_hash().unwrap();
        let h = hash.read().unwrap();

        assert_eq!(h.get("and_short").unwrap(), &VibeValue::Bool(false));
        assert_eq!(h.get("or_short").unwrap(), &VibeValue::Bool(true));
        assert_eq!(h.get("complex").unwrap(), &VibeValue::Bool(true));
        assert_eq!(h.get("simple_and").unwrap(), &VibeValue::Bool(false));
        assert_eq!(h.get("simple_or").unwrap(), &VibeValue::Bool(true));
    }

    #[test]
    fn test_error_handling() {
        let source = "
        def safe_div(a, b)
          begin
            a / b
          rescue
            \"fallback\"
          end
        end

        def ensure_trace(fail)
          trace = []
          begin
            trace = trace.push(\"body\")
            if fail
              1 / 0
            end
            trace = trace.push(\"body_done\")
          rescue
            trace = trace.push(\"rescue\")
          ensure
            trace = trace.push(\"ensure\")
          end
          trace
        end

        {
          success: safe_div(10, 2),
          failure: safe_div(10, 0),
          trace_ok: ensure_trace(false),
          trace_fail: ensure_trace(true)
        }
        ";
        let result = execute(source).unwrap();
        let hash = result.as_hash().unwrap();
        let h = hash.read().unwrap();

        assert_eq!(h.get("success").unwrap(), &VibeValue::Int(5));
        assert_eq!(
            h.get("failure").unwrap(),
            &VibeValue::String("fallback".to_string())
        );

        let ok_arr = h.get("trace_ok").unwrap().as_array().unwrap();
        assert_eq!(ok_arr.read().unwrap().len(), 3);

        let fail_arr = h.get("trace_fail").unwrap().as_array().unwrap();
        assert_eq!(fail_arr.read().unwrap().len(), 3);
    }

    #[test]
    fn test_collection_pipelines() {
        let source = "[1, 2, 3].map do |x| x * 2 end";
        let result = execute(source).unwrap();
        assert_eq!(
            result,
            VibeValue::new_array(vec![
                VibeValue::Int(2),
                VibeValue::Int(4),
                VibeValue::Int(6)
            ])
        );

        let source = "[1, 2, 3, 4].select do |x| x > 2 end";
        let result = execute(source).unwrap();
        assert_eq!(
            VibeValue::new_array(vec![VibeValue::Int(3), VibeValue::Int(4)]),
            result
        );

        let source = "[1, 2, 3].reduce(10) do |acc, x| acc + x end";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(16));
    }

    #[test]
    fn test_extended_stdlib() {
        // String
        let source = "\"hello\".contains?(\"ell\")";
        assert_eq!(execute(source).unwrap(), VibeValue::Bool(true));

        let source = "\"abc\" + \"def\"";
        assert_eq!(
            execute(source).unwrap(),
            VibeValue::String("abcdef".to_string())
        );

        // Array
        let source = "[1, 2].push(3, 4)";
        assert_eq!(
            execute(source).unwrap(),
            VibeValue::new_array(vec![
                VibeValue::Int(1),
                VibeValue::Int(2),
                VibeValue::Int(3),
                VibeValue::Int(4)
            ])
        );

        let source = "[1, 2, 3].include?(2)";
        assert_eq!(execute(source).unwrap(), VibeValue::Bool(true));

        let source = "[1, 2, 3].join(\"-\")";
        assert_eq!(
            execute(source).unwrap(),
            VibeValue::String("1-2-3".to_string())
        );

        // Hash
        let source = "h = {a: 1}.merge({b: 2})\nh.length";
        assert_eq!(execute(source).unwrap(), VibeValue::Int(2));

        let source = "{a: 1, b: 2}.keys().length";
        assert_eq!(execute(source).unwrap(), VibeValue::Int(2));
    }

    #[test]
    fn test_string_methods_expanded() {
        let cases = vec![
            ("\"Gwen\".upcase()", VibeValue::String("GWEN".to_string())),
            ("\"Gwen\".downcase()", VibeValue::String("gwen".to_string())),
            (
                "\"gwen\".capitalize()",
                VibeValue::String("Gwen".to_string()),
            ),
            ("\"Gwen\".swapcase()", VibeValue::String("gWEN".to_string())),
            ("\"gwen\".reverse()", VibeValue::String("newg".to_string())),
            ("\"\".empty?()", VibeValue::Bool(true)),
            ("\"gwen\".empty?()", VibeValue::Bool(false)),
            ("\"gwen\".start_with?(\"gw\")", VibeValue::Bool(true)),
            ("\"gwen\".end_with?(\"en\")", VibeValue::Bool(true)),
            (
                "\"  gwen  \".lstrip()",
                VibeValue::String("gwen  ".to_string()),
            ),
            (
                "\"  gwen  \".rstrip()",
                VibeValue::String("  gwen".to_string()),
            ),
            (
                "\"  gwen  \".strip()",
                VibeValue::String("gwen".to_string()),
            ),
            (
                "\"prefix_gwen\".delete_prefix(\"prefix_\")",
                VibeValue::String("gwen".to_string()),
            ),
            (
                "\"gwen_suffix\".delete_suffix(\"_suffix\")",
                VibeValue::String("gwen".to_string()),
            ),
            ("\"gwen\".clear()", VibeValue::String("".to_string())),
            (
                "\"gwen\".concat(\" is \", 25)",
                VibeValue::String("gwen is 25".to_string()),
            ),
            ("\"hé\".bytesize()", VibeValue::Int(3)), // h is 1, é is 2 bytes
            ("\"h\".ord()", VibeValue::Int(104)),
            ("\"hé\".chr()", VibeValue::String("h".to_string())),
            (
                "\"a b c\".split()",
                VibeValue::new_array(vec![
                    VibeValue::String("a".to_string()),
                    VibeValue::String("b".to_string()),
                    VibeValue::String("c".to_string()),
                ]),
            ),
            (
                "\"a,b,c\".split(\",\")",
                VibeValue::new_array(vec![
                    VibeValue::String("a".to_string()),
                    VibeValue::String("b".to_string()),
                    VibeValue::String("c".to_string()),
                ]),
            ),
            ("\"hello\".index(\"e\")", VibeValue::Int(1)),
            ("\"hello\".rindex(\"l\")", VibeValue::Int(3)),
            ("\"hello\".index(\"z\")", VibeValue::Nil),
        ];

        for (source, expected) in cases {
            let result = execute(source).unwrap();
            assert_eq!(result, expected, "Failed on: {}", source);
        }
    }

    #[test]
    fn test_builtins() {
        let source = "JSON.parse(\"{\\\"a\\\": 1}\")";
        let result = execute(source).unwrap();
        let mut expected_hash = std::collections::HashMap::new();
        expected_hash.insert("a".to_string(), VibeValue::Int(1));
        assert_eq!(result, VibeValue::new_hash(expected_hash));
        let source = "uuid()";
        let result = execute(source).unwrap();
        if let VibeValue::String(s) = result {
            assert_eq!(s.len(), 36);
        } else {
            panic!("Expected string for uuid");
        }
        let source = "now()";
        let result = execute(source).unwrap();
        assert!(matches!(result, VibeValue::Time(_)));
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
        assert_eq!(result, VibeValue::Int(10));
    }

    #[test]
    fn test_loop_control() {
        let source = "
        def for_break()
          out = []
          for n in [1, 2, 3, 4]
            if n == 3
              break
            end
            out = out.push(n)
          end
          out
        end

        def while_next()
          n = 0
          out = []
          while n < 4
            n = n + 1
            if n % 2 == 0
              next
            end
            out = out.push(n)
          end
          out
        end

        {
          b: for_break(),
          n: while_next()
        }
        ";
        let result = execute(source).unwrap();
        let hash = result.as_hash().unwrap();
        let h = hash.read().unwrap();

        let b_arr = h.get("b").unwrap().as_array().unwrap();
        assert_eq!(b_arr.read().unwrap().len(), 2);

        let n_arr = h.get("n").unwrap().as_array().unwrap();
        assert_eq!(n_arr.read().unwrap().len(), 2);
    }

    #[test]
    fn test_durations_and_time() {
        let source = "
        t = Time.parse(\"2024-05-01 10:30:00\", in: \"UTC\")
        d = 1.hours + 30.minutes
        later = t + d
        {
          iso: d.iso8601,
          seconds: d.seconds,
          later_fmt: later.format(\"2006-01-02T15:04:05Z07:00\")
        }
        ";
        let result = execute(source).unwrap();
        let hash = result.as_hash().unwrap();
        let h = hash.read().unwrap();

        assert_eq!(
            h.get("iso").unwrap(),
            &VibeValue::String("PT1H30M".to_string())
        );
        assert_eq!(h.get("seconds").unwrap(), &VibeValue::Int(5400));
        assert_eq!(
            h.get("later_fmt").unwrap(),
            &VibeValue::String("2024-05-01T12:00:00UTC".to_string())
        );
    }

    #[test]
    fn test_interpolation() {
        let source = "name = \"Gwen\"\n\"Hello #{name}!\"";
        let result = execute(source).unwrap();
        assert_eq!(result.to_string(), "Hello Gwen!");
    }

    #[test]
    fn test_enums() {
        let source = "
        enum Status
          Active
          Inactive
        end
        Status::Active
        ";
        let result = execute(source).unwrap();
        assert!(result.to_string().contains("Active"));

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
            VibeValue::EnumVariant {
                enum_name: "Color".to_string(),
                variant_name: "Red".to_string(),
            }
        );
    }

    #[test]
    fn test_classes() {
        let source = "
        class Point
          def initialize(x, y)
            @x = x
            @y = y
          end
          def x() @x end
          def y() @y end
        end
        p = Point.new(10, 20)
        p.x() + p.y()
        ";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(30));

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
            VibeValue::String("Hello, I am Gwen and I am 26 years old.".to_string())
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
        assert_eq!(result, VibeValue::Int(31));
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
        assert_eq!(result, VibeValue::Int(150));

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
    fn test_collections() {
        let source = "
        a = [1, 2, 3]
        a[0] = 10
        a
        ";
        let result = execute(source).unwrap();
        assert_eq!(
            result,
            VibeValue::new_array(vec![
                VibeValue::Int(10),
                VibeValue::Int(2),
                VibeValue::Int(3)
            ])
        );

        let source = "arr = [1, 2, 3]\narr[1]";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::Int(2));

        let source = "h = {name: \"Gwen\", age: 25}\nh[\"name\"]";
        let result = execute(source).unwrap();
        assert_eq!(result, VibeValue::String("Gwen".to_string()));
    }

    #[test]
    fn test_hash_reference_parity() {
        let source = "
        def run()
          record = { b: 2, a: 1, c: 3 }
          with_nil = { a: 1, b: nil, c: 3 }
          nested = { user: { profile: { name: \"Alex\" } } }

          each_pairs = []
          # record.each do |k, v|
          #   each_pairs = each_pairs.push(k + \"=\" + v.to_string())
          # end

          select_gt1 = record.select do |k, v|
            v > 1
          end

          reject_even = record.reject do |k, v|
            v % 2 == 0
          end

          transform_keys = record.transform_keys do |k|
            \"x_\" + k
          end

          transform_values = record.transform_values do |v|
            v * 10
          end

          {
            size: record.size,
            length: record.length,
            empty_false: record.empty?,
            empty_true: {}.empty?,
            key_symbol: record.key?(:a),
            key_string: record.has_key?(\"b\"),
            include_symbol: record.include?(:c),
            missing_key: record.key?(:missing),
            # keys: record.keys,
            # values: record.values,
            fetch_hit: record.fetch(:a),
            fetch_default: record.fetch(:missing, 99),
            fetch_nil: record.fetch(:missing),
            dig_hit: nested.dig(:user, :profile, :name),
            dig_miss: nested.dig(:user, :profile, :missing),
            slice: record.slice(:a, \"c\"),
            except: record.except(:b),
            select_gt1: select_gt1,
            reject_even: reject_even,
            transform_keys: transform_keys,
            transform_values: transform_values,
            compact: with_nil.compact()
          }
        end
        run()
        ";
        let result = execute(source).unwrap();
        let h = result.as_hash().unwrap();
        let got = h.read().unwrap();

        assert_eq!(got.get("size").unwrap(), &VibeValue::Int(3));
        assert_eq!(got.get("fetch_hit").unwrap(), &VibeValue::Int(1));
        assert_eq!(got.get("fetch_default").unwrap(), &VibeValue::Int(99));
        assert_eq!(
            got.get("dig_hit").unwrap(),
            &VibeValue::String("Alex".to_string())
        );
        assert_eq!(
            got.get("compact")
                .unwrap()
                .as_hash()
                .unwrap()
                .read()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn test_array_reference_parity() {
        let source = "
        def run()
          values = [3, 1, 2, 1]

          find_hit = values.find do |v| v > 2 end
          find_index_hit = values.find_index do |v| v % 2 == 0 end
          count_block = values.count do |v| v > 1 end

          {
            size: values.size,
            empty_false: values.empty?,
            include_hit: values.include?(2),
            index_hit: values.index(1),
            rindex_hit: values.rindex(1),
            fetch_hit: values.fetch(2),
            find_hit: find_hit,
            find_index_hit: find_index_hit,
            count_all: values.count,
            count_value: values.count(1),
            count_block: count_block,
            reverse: values.reverse,
            flatten: [[1, 2], [3]].flatten
          }
        end
        run()
        ";
        let result = execute(source).unwrap();
        let h = result.as_hash().unwrap();
        let got = h.read().unwrap();

        assert_eq!(got.get("size").unwrap(), &VibeValue::Int(4));
        assert_eq!(got.get("include_hit").unwrap(), &VibeValue::Bool(true));
        assert_eq!(got.get("index_hit").unwrap(), &VibeValue::Int(1));
        assert_eq!(got.get("rindex_hit").unwrap(), &VibeValue::Int(3));
        assert_eq!(got.get("find_hit").unwrap(), &VibeValue::Int(3));
        assert_eq!(got.get("find_index_hit").unwrap(), &VibeValue::Int(2));
        assert_eq!(got.get("count_block").unwrap(), &VibeValue::Int(2));
        assert_eq!(
            got.get("flatten")
                .unwrap()
                .as_array()
                .unwrap()
                .read()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn test_collection_advanced_parity() {
        let source = "
        def run()
          nums = [1, 2, 3, 4, 5, 6]
          words = [\"apple\", \"bat\", \"cat\", \"apple\", \"bat\", \"apple\"]

          {
            chunk: nums.chunk(2),
            window: [1, 2, 3].window(2),
            partition: nums.partition do |n| n % 2 == 0 end,
            group_by: words.group_by do |w| w.length end,
            tally: words.tally(),
            tally_block: words.tally do |w| w.uppercase() end,
            sort: [3, 1, 2].sort(),
            sort_by: [\"ccc\", \"a\", \"bb\"].sort_by do |s| s.length end,
            reduce_sum: [1, 2, 3].reduce(10) do |acc, n| acc + n end
          }
        end
        run()
        ";
        let result = execute(source).unwrap();
        let h = result.as_hash().unwrap();
        let got = h.read().unwrap();

        assert_eq!(
            got.get("chunk")
                .unwrap()
                .as_array()
                .unwrap()
                .read()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            got.get("window")
                .unwrap()
                .as_array()
                .unwrap()
                .read()
                .unwrap()
                .len(),
            2
        );

        let part = got.get("partition").unwrap().as_array().unwrap();
        assert_eq!(
            part.read().unwrap()[0]
                .as_array()
                .unwrap()
                .read()
                .unwrap()
                .len(),
            3
        ); // evens

        let groups = got.get("group_by").unwrap().as_hash().unwrap();
        assert_eq!(
            groups
                .read()
                .unwrap()
                .get("3")
                .unwrap()
                .as_array()
                .unwrap()
                .read()
                .unwrap()
                .len(),
            3
        ); // bat, cat, bat

        let tally = got.get("tally").unwrap().as_hash().unwrap();
        assert_eq!(
            tally.read().unwrap().get("apple").unwrap(),
            &VibeValue::Int(3)
        );

        assert_eq!(
            got.get("sort").unwrap().as_array().unwrap().read().unwrap()[0],
            VibeValue::Int(1)
        );

        let sorted_by = got.get("sort_by").unwrap().as_array().unwrap();
        assert_eq!(
            sorted_by.read().unwrap()[0],
            VibeValue::String("a".to_string())
        );

        assert_eq!(got.get("reduce_sum").unwrap(), &VibeValue::Int(16));
    }

    #[test]
    fn test_numeric_and_temporal_parity() {
        let source = "
        def run()
          t = Time.parse(\"2024-05-01 10:30:05\", in: \"UTC\")
          m = money(\"50.00 USD\")

          counter = 0
          3.times do |i| counter = counter + 1 end

          {
            int_abs: (-5).abs(),
            int_even: 4.even?(),
            int_odd: 4.odd?(),
            int_clamp: 10.clamp(1, 5),
            float_abs: -5.5.abs(),
            float_round: 5.6.round(),
            float_floor: 5.6.floor(),
            float_ceil: 5.1.ceil(),
            float_clamp: 10.5.clamp(1.0, 5.5),
            times_count: counter,
            time_year: t.year,
            time_hour: t.hour,
            money_cents: m.cents,
            money_fmt: m.format()
          }
        end
        run()
        ";
        let result = execute(source).unwrap();
        let h = result.as_hash().unwrap();
        let got = h.read().unwrap();

        assert_eq!(got.get("int_abs").unwrap(), &VibeValue::Int(5));
        assert_eq!(got.get("int_even").unwrap(), &VibeValue::Bool(true));
        assert_eq!(got.get("int_clamp").unwrap(), &VibeValue::Int(5));
        assert_eq!(got.get("float_round").unwrap(), &VibeValue::Int(6));
        assert_eq!(got.get("float_floor").unwrap(), &VibeValue::Int(5));
        assert_eq!(got.get("times_count").unwrap(), &VibeValue::Int(3));
        assert_eq!(got.get("time_year").unwrap(), &VibeValue::Int(2024));
        assert_eq!(got.get("time_hour").unwrap(), &VibeValue::Int(10));
        assert_eq!(got.get("money_cents").unwrap(), &VibeValue::Int(5000));
        assert_eq!(
            got.get("money_fmt").unwrap(),
            &VibeValue::String("50.00 USD".to_string())
        );
    }

    #[test]
    fn test_string_advanced_parity() {
        let source = "
        def run()
          s = \"  Gwen  \"
          {
            chomp: \"line\\n\".chomp(),
            squish: \"  too   many   spaces  \".squish(),
            match: \"user_123\".match(\"user_(\\\\d+)\"),
            scan: \"a1 b2 c3\".scan(\"\\\\d+\"),
            sub: \"hello world\".sub(\"hello\", \"hi\"),
            gsub: \"ba na na\".gsub(\" \", \"\"),
            strip_bang: s.strip!(),
            strip_bang_nil: \"Gwen\".strip!(),
            template: \"Hello {{user.name}}!\".template({ user: { name: \"Gwen\" } })
          }
        end
        run()
        ";
        let result = execute(source).unwrap();
        let h = result.as_hash().unwrap();
        let got = h.read().unwrap();

        assert_eq!(
            got.get("chomp").unwrap(),
            &VibeValue::String("line".to_string())
        );
        assert_eq!(
            got.get("squish").unwrap(),
            &VibeValue::String("too many spaces".to_string())
        );

        let mat = got.get("match").unwrap().as_array().unwrap();
        assert_eq!(mat.read().unwrap()[1], VibeValue::String("123".to_string()));

        let scan = got.get("scan").unwrap().as_array().unwrap();
        assert_eq!(scan.read().unwrap().len(), 3);

        assert_eq!(
            got.get("sub").unwrap(),
            &VibeValue::String("hi world".to_string())
        );
        assert_eq!(
            got.get("gsub").unwrap(),
            &VibeValue::String("banana".to_string())
        );
        assert_eq!(
            got.get("strip_bang").unwrap(),
            &VibeValue::String("Gwen".to_string())
        );
        assert_eq!(got.get("strip_bang_nil").unwrap(), &VibeValue::Nil);
        assert_eq!(
            got.get("template").unwrap(),
            &VibeValue::String("Hello Gwen!".to_string())
        );
    }

    #[test]
    fn test_stdlib_final_parity() {
        let source = "
        def run()
          # Array.sample
          arr = [1, 2, 3]
          s1 = arr.sample()
          s2 = arr.sample(2)

          # Hash helpers
          h = { a: 1, b: 2 }
          remapped = h.remap_keys({ a: :alpha })

          deep_h = { user: { name: \"Gwen\" } }
          deep_transformed = deep_h.deep_transform_keys do |k| k.upcase() end

          # Duration offsets
          anchor = Time.parse(\"2024-05-01 10:00:00\", in: \"UTC\")

          {
            sample_single: arr.include?(s1),
            sample_multi_len: s2.length,
            remap_hit: remapped.key?(:alpha),
            remap_old_miss: remapped.key?(:a),
            deep_key: deep_transformed.fetch(:USER).key?(:NAME),
            ago: 1.hours.ago(anchor).format(\"15:04:05\"),
            after: 30.minutes.after(anchor).format(\"15:04:05\")
          }
        end
        run()
        ";
        let result = execute(source).unwrap();
        let h = result.as_hash().unwrap();
        let got = h.read().unwrap();

        assert_eq!(got.get("sample_single").unwrap(), &VibeValue::Bool(true));
        assert_eq!(got.get("sample_multi_len").unwrap(), &VibeValue::Int(2));
        assert_eq!(got.get("remap_hit").unwrap(), &VibeValue::Bool(true));
        assert_eq!(got.get("remap_old_miss").unwrap(), &VibeValue::Bool(false));
        assert_eq!(got.get("deep_key").unwrap(), &VibeValue::Bool(true));
        assert_eq!(
            got.get("ago").unwrap(),
            &VibeValue::String("09:00:00UTC".to_string())
        );
        assert_eq!(
            got.get("after").unwrap(),
            &VibeValue::String("10:30:00UTC".to_string())
        );
    }

    #[test]
    fn test_gradual_typing() {
        let source = "
        def add(a: int, b: int) -> int
          a + b
        end

        def find_user(id: int) -> string?
          if id == 1
            \"Gwen\"
          else
            nil
          end
        end

        def process_union(v: int | string) -> string
          v.to_string()
        end

        {
          sum: add(1, 2),
          user1: find_user(1),
          user2: find_user(2),
          union_int: process_union(10),
          union_str: process_union(\"ok\")
        }
        ";
        let result = execute(source).unwrap();
        let h = result.as_hash().unwrap();
        let got = h.read().unwrap();

        assert_eq!(got.get("sum").unwrap(), &VibeValue::Int(3));
        assert_eq!(
            got.get("user1").unwrap(),
            &VibeValue::String("Gwen".to_string())
        );
        assert_eq!(got.get("user2").unwrap(), &VibeValue::Nil);
        assert_eq!(
            got.get("union_int").unwrap(),
            &VibeValue::String("10".to_string())
        );
        assert_eq!(
            got.get("union_str").unwrap(),
            &VibeValue::String("ok".to_string())
        );

        // Test violations
        assert!(execute("def f(n: int) n end; f(\"wrong\")").is_err());
        assert!(execute("def f -> int; \"wrong\" end; f()").is_err());
    }

    #[test]
    fn test_yield_and_raise_parity() {
        let source = "
        def three_times
          yield 1
          yield 2
          yield 3
        end

        def safe_div(a: int, b: int)
          begin
            if b == 0
              raise \"division by zero\"
            end
            a / b
          rescue(RuntimeError)
            -1
          end
        end

        def typed_rescue_mismatch
          begin
            raise \"boom\"
          rescue(TypeError)
            \"caught type error\"
          end
        end

        def catch_fail_helper
          begin
            typed_rescue_mismatch()
          rescue
            \"caught runtime error\"
          end
        end

        results = []
        three_times() do |x|
          results = results.push(x)
        end

        {
          yield_sum: results.sum(),
          div_ok: safe_div(10, 2),
          div_fail: safe_div(10, 0),
          catch_fail: catch_fail_helper()
        }
        ";
        let result = execute(source).unwrap();
        let h = result.as_hash().unwrap();
        let got = h.read().unwrap();

        assert_eq!(got.get("yield_sum").unwrap(), &VibeValue::Int(6));
        assert_eq!(got.get("div_ok").unwrap(), &VibeValue::Int(5));
        assert_eq!(got.get("div_fail").unwrap(), &VibeValue::Int(-1));
        assert_eq!(
            got.get("catch_fail").unwrap(),
            &VibeValue::String("caught runtime error".to_string())
        );
    }

    #[test]
    fn test_array_mutation() {
        let source = "
            arr = [1, 2]
            other = arr.push(3)
            # push returns a NEW array, arr is still [1, 2]
            arr[0] = 10
            # but index assignment IS mutating!
            [arr.length, arr[0]]
        ";
        let result = execute(source).unwrap();
        assert_eq!(
            result,
            VibeValue::new_array(vec![VibeValue::Int(2), VibeValue::Int(10)])
        );
    }
}

#[cfg(target_arch = "wasm32")]
export!(MyEngine);
