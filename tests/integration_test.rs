use std::fs;
use tempfile::TempDir;

#[test]
fn test_golden_path() {
    // Create temporary directories
    let temp_dir = TempDir::new().unwrap();
    let input_dir = temp_dir.path().join("input");
    let output_dir = temp_dir.path().join("output");
    fs::create_dir_all(&input_dir).unwrap();

    // Create markdown file with tangle blocks
    let markdown = r#"# Example Project

This is a simple example demonstrating literate programming.

## Main Program

```tangle://main.rs
fn main() {
    println!("Hello, World!");
}
```

## Configuration

```tangle://config.toml
name = "example"
version = "1.0.0"
```

## Multiple blocks for same file

```tangle://lib.rs?at=z
// Footer comment
```

```tangle://lib.rs
// Main content
pub fn greet() {
    println!("Hello!");
}
```

```tangle://lib.rs?at=a
// Header comment
```
"#;

    fs::write(input_dir.join("example.md"), markdown).unwrap();

    // Run lit using the public API (via subprocess since we're testing the binary)
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lit"))
        .arg(&input_dir)
        .arg(&output_dir)
        .output()
        .expect("Failed to execute lit");

    assert!(
        output.status.success(),
        "lit command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify output files exist
    assert!(output_dir.join("main.rs").exists());
    assert!(output_dir.join("config.toml").exists());
    assert!(output_dir.join("lib.rs").exists());

    // Verify content of main.rs
    let main_content = fs::read_to_string(output_dir.join("main.rs")).unwrap();
    assert_eq!(
        main_content,
        "fn main() {\n    println!(\"Hello, World!\");\n}\n"
    );

    // Verify content of config.toml
    let config_content = fs::read_to_string(output_dir.join("config.toml")).unwrap();
    assert_eq!(config_content, "name = \"example\"\nversion = \"1.0.0\"\n");

    // Verify content of lib.rs (blocks ordered: a, m, z)
    let lib_content = fs::read_to_string(output_dir.join("lib.rs")).unwrap();
    assert_eq!(
        lib_content,
        "// Header comment\n\n// Main content\npub fn greet() {\n    println!(\"Hello!\");\n}\n\n// Footer comment\n"
    );
}
