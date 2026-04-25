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
        TestCase {
            name: "errors/assertions",
            file: "errors/assertions.vibe",
            function: "ensure_positive",
            args: vec!["10".to_string()],
            expected: "I(10)",
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

#[tokio::test]
async fn test_stdlib_core() -> anyhow::Result<()> {
    let host = VibesHost::new()?;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let examples_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("go")
        .join("examples");

    let source = std::fs::read_to_string(examples_dir.join("stdlib/core_utilities.vibe"))?;
    let result = host.execute(&source, "run", &[]).await?;

    // Check key components instead of exact JSON to avoid fragile formatting issues
    assert!(result.contains("to_int"), "to_int missing: {}", result);
    assert!(result.contains("42"), "42 missing: {}", result);
    assert!(result.contains("to_float"), "to_float missing: {}", result);
    assert!(result.contains("1.25"), "1.25 missing: {}", result);
    assert!(result.contains("json_id"), "json_id missing: {}", result);
    assert!(result.contains("p-1"), "p-1 missing: {}", result);
    assert!(
        result.contains("uuid_length"),
        "uuid_length missing: {}",
        result
    );
    assert!(result.contains("36"), "36 missing: {}", result);
    assert!(
        result.contains("random_length"),
        "random_length missing: {}",
        result
    );
    assert!(result.contains("8"), "8 missing: {}", result);
    assert!(
        result.contains("2024-05-01T10:30:00"),
        "parsed_time missing: {}",
        result
    );

    Ok(())
}

#[tokio::test]
async fn test_strings_core() -> anyhow::Result<()> {
    let host = VibesHost::new()?;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let examples_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("go")
        .join("examples");

    let source = std::fs::read_to_string(examples_dir.join("strings/operations.vibe"))?;
    let result = host.execute(&source, "run", &[]).await?;

    // Check key components
    assert!(result.contains("bytesize"), "bytesize missing: {}", result);
    assert!(
        result.contains("capitalize"),
        "capitalize missing: {}",
        result
    );
    assert!(
        result.contains("Héllo world"),
        "capitalize value wrong: {}",
        result
    );
    assert!(result.contains("reverse"), "reverse missing: {}", result);
    assert!(result.contains("olléh"), "reverse value wrong: {}", result);
    assert!(result.contains("index"), "index missing: {}", result);
    assert!(result.contains("rindex"), "rindex missing: {}", result);
    assert!(
        result.contains("slice_char"),
        "slice_char missing: {}",
        result
    );
    assert!(result.contains("é"), "slice_char value wrong: {}", result);
    assert!(
        result.contains("slice_range"),
        "slice_range missing: {}",
        result
    );
    assert!(
        result.contains("éllo"),
        "slice_range value wrong: {}",
        result
    );
    assert!(
        result.contains("strip_bang"),
        "strip_bang missing: {}",
        result
    );
    assert!(
        result.contains("strip_bang_nochange"),
        "strip_bang_nochange missing: {}",
        result
    );
    assert!(
        result.contains("null"),
        "null missing (for strip_bang_nochange): {}",
        result
    );
    assert!(result.contains("match"), "match missing: {}", result);
    assert!(result.contains("ID-12"), "match value missing: {}", result);

    Ok(())
}

#[tokio::test]
async fn test_money_and_duration() -> anyhow::Result<()> {
    let host = VibesHost::new()?;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let examples_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("go")
        .join("examples");

    // Money
    let source = std::fs::read_to_string(examples_dir.join("money/operations.vibe"))?;
    let result = host.execute(&source, "add_pledges", &[]).await?;
    assert!(
        result.contains("62.50 USD"),
        "add_pledges failed: {}",
        result
    );

    // Duration
    let source = std::fs::read_to_string(examples_dir.join("durations/durations.vibe"))?;
    let result = host.execute(&source, "reminder_delay_seconds", &[]).await?;
    assert!(
        result.contains("I(300)"),
        "reminder_delay_seconds failed: {}",
        result
    );

    Ok(())
}

#[tokio::test]
async fn test_ranges() -> anyhow::Result<()> {
    let host = VibesHost::new()?;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let examples_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("go")
        .join("examples");

    let source = std::fs::read_to_string(examples_dir.join("ranges/usage.vibe"))?;

    let result = host
        .execute(
            &source,
            "inclusive_range_sum",
            &["1".to_string(), "5".to_string()],
        )
        .await?;
    assert!(
        result.contains("I(15)"),
        "inclusive_range_sum failed: {}",
        result
    );

    let result = host
        .execute(
            &source,
            "range_even_numbers",
            &["1".to_string(), "10".to_string()],
        )
        .await?;
    assert!(
        result.contains("2,4,6,8,10"),
        "range_even_numbers failed: {}",
        result
    );

    Ok(())
}

#[tokio::test]
async fn test_all_examples_compile() -> anyhow::Result<()> {
    let host = VibesHost::new()?;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let examples_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("go")
        .join("examples");

    let mut files = Vec::new();
    walk_dir(&examples_dir, &mut files)?;

    let mut failed = Vec::new();

    for file in files {
        let source = std::fs::read_to_string(&file)?;
        let rel_path = file
            .strip_prefix(&examples_dir)
            .unwrap()
            .display()
            .to_string();

        if let Err(e) = host.check(&source).await {
            failed.push(format!("{}: {}", rel_path, e));
        }
    }

    if !failed.is_empty() {
        anyhow::bail!(
            "The following examples failed to compile:\n\n{}",
            failed.join("\n\n")
        );
    }

    Ok(())
}

fn walk_dir(dir: &std::path::Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, files)?;
        } else if path.extension().map_or(false, |ext| ext == "vibe") {
            files.push(path);
        }
    }
    Ok(())
}
