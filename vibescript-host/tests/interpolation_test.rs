use vibescript_host::VibesHost;

#[tokio::test]
async fn test_full_interpolation() -> anyhow::Result<()> {
    let host = VibesHost::new()?;

    let cases = vec![
        (
            "simple",
            r##"def run() "hello #{1 + 2}" end"##,
            "S(\"hello 3\")",
        ),
        (
            "nested_strings",
            r##"def run() "hello #{"world"}" end"##,
            "S(\"hello world\")",
        ),
        (
            "complex_expr",
            r##"def run() "val: #{[1, 2, 3].size()}" end"##,
            "S(\"val: 3\")",
        ),
        (
            "deep_nesting",
            r##"def run() "deep: #{ { a: { b: "c" } }[:a][:b].upcase() }" end"##,
            "S(\"deep: C\")",
        ),
        (
            "conditional",
            r##"def run(x) "is #{if x > 0 then "pos" else "neg" end}" end"##,
            "S(\"is pos\")",
        ),
        (
            "multiple",
            r##"def run() "#{1} + #{2} = #{1 + 2}" end"##,
            "S(\"1 + 2 = 3\")",
        ),
    ];

    for (name, source, expected) in cases {
        let args = if name == "conditional" {
            vec!["10".to_string()]
        } else {
            vec![]
        };
        let result = host
            .execute(source, "run", &args)
            .await
            .map_err(|e| anyhow::anyhow!("Test '{}' failed to execute: {}", name, e))?;
        assert!(
            result.contains(expected),
            "Test '{}' failed: expected {}, got {}",
            name,
            expected,
            result
        );
    }

    Ok(())
}
