use std::path::PathBuf;
use vibescript_host::VibesHost;

struct TestCase {
    name: &'static str,
    file: &'static str,
    function: &'static str,
    args: Vec<String>,
    expected: &'static str,
}

#[tokio::test]
async fn test_examples() -> anyhow::Result<()> {
    let host = VibesHost::new()?;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Examples are in the go/examples directory
    let examples_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("go")
        .join("examples");

    let cases = vec![
        TestCase {
            name: "basics/add_numbers",
            file: "basics/literals_and_operators.vibe",
            function: "add_numbers",
            args: vec!["2".to_string(), "3".to_string()],
            expected: "I(5)",
        },
        TestCase {
            name: "basics/combine_strings",
            file: "basics/literals_and_operators.vibe",
            function: "combine_strings",
            args: vec!["hello".to_string(), "world".to_string()],
            expected: "S(\"hello world\")",
        },
        TestCase {
            name: "basics/negate",
            file: "basics/literals_and_operators.vibe",
            function: "negate",
            args: vec!["7".to_string()],
            expected: "I(-7)",
        },
        TestCase {
            name: "basics/truth_table_true",
            file: "basics/literals_and_operators.vibe",
            function: "truth_table",
            args: vec!["true".to_string()],
            expected: "B(true)",
        },
        TestCase {
            name: "basics/mix_literals",
            file: "basics/literals_and_operators.vibe",
            function: "mix_literals",
            args: vec![],
            expected: "Json(\"{\\\"answer\\\":42,\\\"flags\\\":[true,false,null],\\\"quote\\\":\\\"keep going\\\",\\\"ratio\\\":3.75}\")",
        },
        TestCase {
            name: "functions/greet",
            file: "basics/functions_and_calls.vibe",
            function: "greet",
            args: vec!["martin".to_string()],
            expected: "S(\"hello martin\")",
        },
        TestCase {
            name: "functions/sum_three",
            file: "basics/functions_and_calls.vibe",
            function: "sum_three",
            args: vec!["1".to_string(), "2".to_string(), "3".to_string()],
            expected: "I(6)",
        },
        TestCase {
            name: "control_flow/while_loop",
            file: "control_flow/while_loop.vibe",
            function: "run",
            args: vec![],
            expected: "Json(\"[5,4,3,2,1]\")",
        },
        TestCase {
            name: "arrays/first_two",
            file: "arrays/extras.vibe",
            function: "first_two",
            args: vec!["[1,2,3,4]".to_string()],
            expected: "Json(\"[1,2]\")",
        },
        TestCase {
            name: "arrays/numeric_sum",
            file: "arrays/extras.vibe",
            function: "numeric_sum",
            args: vec!["[2,3,5]".to_string()],
            expected: "I(10)",
        },
        TestCase {
            name: "enums/member_name",
            file: "enums/operations.vibe",
            function: "member_name",
            args: vec![],
            expected: "S(\"Draft\")",
        },
    ];

    for case in cases {
        let path = examples_dir.join(case.file);
        let source = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;

        let result = host
            .execute(&source, case.function, &case.args)
            .await
            .map_err(|e| anyhow::anyhow!("Test '{}' failed: {}", case.name, e))?;

        assert!(
            result.contains(case.expected),
            "Test '{}' failed: expected result to contain {}, but got {}",
            case.name,
            case.expected,
            result
        );
    }

    Ok(())
}
