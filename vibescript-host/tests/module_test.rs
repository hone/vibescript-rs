use std::path::PathBuf;
use vibescript_host::VibesHost;

fn get_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("modules")
}

#[tokio::test]
async fn test_module_system() -> anyhow::Result<()> {
    let host = VibesHost::new()?;
    host.search_paths.write().unwrap().push(get_fixtures_dir());

    let source = r#"
        mod = require("lib_test")
        greet("Gwen")
    "#;

    let result = host.execute(source, "", &[]).await?;
    assert!(result.contains("Hello, Gwen!"), "Got: {}", result);

    Ok(())
}

#[tokio::test]
async fn test_module_alias() -> anyhow::Result<()> {
    let host = VibesHost::new()?;
    host.search_paths.write().unwrap().push(get_fixtures_dir());

    let source = r#"
        require("alias_test", as: "my_mod")
        my_mod.val()
    "#;

    let result = host.execute(source, "", &[]).await?;
    assert!(result.contains("100"), "Got: {}", result);

    Ok(())
}

#[tokio::test]
async fn test_module_sandbox_violation() -> anyhow::Result<()> {
    let host = VibesHost::new()?;

    // Attempt to require a file outside the search path (current dir)
    // Create a file in a location we KNOW is outside the default search path
    let tmp_dir = std::env::temp_dir();
    let outside_file = tmp_dir.join("sandbox_violation.vibe");
    std::fs::write(&outside_file, "def secret(); 42; end")?;

    let source = format!(r#"require("{}")"#, outside_file.display());
    let result = host.execute(&source, "", &[]).await;

    let err_msg = result.unwrap_err().to_string();
    println!("Caught expected error: {}", err_msg);
    assert!(err_msg.contains("Security error"), "Got: {}", err_msg);

    let _ = std::fs::remove_file(outside_file);

    Ok(())
}

#[tokio::test]
async fn test_circular_dependency() -> anyhow::Result<()> {
    let host = VibesHost::new()?;
    host.search_paths.write().unwrap().push(get_fixtures_dir());

    let source = r#"require("circular_a")"#;
    let result = host.execute(source, "", &[]).await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Circular dependency")
    );

    Ok(())
}
