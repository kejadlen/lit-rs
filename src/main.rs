use clap::Parser;
use color_eyre::Result;
use markdown::{to_mdast, ParseOptions};
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(name = "lit")]
#[command(about = "A literate programming tool", long_about = None)]
struct Args {
    /// Directory to process
    #[arg(value_name = "DIRECTORY")]
    directory: PathBuf,
}

/// Represents a code snippet with a tangle path
#[derive(Debug, Clone, PartialEq)]
struct Snippet {
    /// The file path where this code should be written
    path: PathBuf,
    /// The code content
    content: String,
}

/// Parse a markdown file and extract all tangle code blocks
fn parse_markdown_file(path: &PathBuf) -> Result<Vec<Snippet>> {
    let content = fs::read_to_string(path)?;
    Ok(parse_tangle_blocks(&content))
}

/// Parse markdown content and extract code blocks with tangle:// paths
fn parse_tangle_blocks(markdown_text: &str) -> Vec<Snippet> {
    use markdown::mdast::Node;

    // Parse markdown to AST
    let ast = match to_mdast(markdown_text, &ParseOptions::default()) {
        Ok(ast) => ast,
        Err(_) => return Vec::new(),
    };

    // Extract snippets from top-level code blocks only
    let mut snippets = Vec::new();
    if let Node::Root(root) = ast {
        for child in &root.children {
            if let Node::Code(code) = child {
                if let Some(lang) = &code.lang {
                    if let Some(path_str) = lang.strip_prefix("tangle://") {
                        snippets.push(Snippet {
                            path: PathBuf::from(path_str),
                            content: code.value.clone(),
                        });
                    }
                }
            }
        }
    }

    snippets
}

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    println!("Processing markdown files in {}:\n", args.directory.display());

    for entry in WalkDir::new(&args.directory)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if ext == "md" {
                    let path = entry.path().to_path_buf();
                    println!("File: {}", path.display());

                    match parse_markdown_file(&path) {
                        Ok(snippets) => {
                            if snippets.is_empty() {
                                println!("  No tangle blocks found");
                            } else {
                                println!("  Found {} tangle block(s):", snippets.len());
                                for snippet in snippets {
                                    println!("    → {}", snippet.path.display());
                                    println!("      {} lines", snippet.content.lines().count());
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("  Error parsing file: {}", e);
                        }
                    }
                    println!();
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_tangle_block() {
        let markdown = r#"# Test

```tangle://src/main.rs
fn main() {
    println!("Hello");
}
```
"#;

        let snippets = parse_tangle_blocks(markdown);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(snippets[0].content, "fn main() {\n    println!(\"Hello\");\n}");
    }

    #[test]
    fn test_multiple_tangle_blocks() {
        let markdown = r#"# Multiple Blocks

```tangle://file1.rs
code 1
```

Some text here.

```tangle://file2.rs
code 2
```
"#;

        let snippets = parse_tangle_blocks(markdown);
        assert_eq!(snippets.len(), 2);
        assert_eq!(snippets[0].path, PathBuf::from("file1.rs"));
        assert_eq!(snippets[0].content, "code 1");
        assert_eq!(snippets[1].path, PathBuf::from("file2.rs"));
        assert_eq!(snippets[1].content, "code 2");
    }

    #[test]
    fn test_ignore_regular_code_blocks() {
        let markdown = r#"# Test

```rust
// This is regular code
let x = 42;
```

```tangle://output.rs
// This should be extracted
let y = 10;
```
"#;

        let snippets = parse_tangle_blocks(markdown);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].path, PathBuf::from("output.rs"));
        assert_eq!(snippets[0].content, "// This should be extracted\nlet y = 10;");
    }

    #[test]
    fn test_ignore_nested_in_blockquote() {
        let markdown = r#"# Test

```tangle://top-level.txt
Top level content
```

> Blockquote here
>
> ```tangle://nested.txt
> This should NOT be extracted
> ```
"#;

        let snippets = parse_tangle_blocks(markdown);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].path, PathBuf::from("top-level.txt"));
        assert_eq!(snippets[0].content, "Top level content");
    }

    #[test]
    fn test_ignore_nested_in_list() {
        let markdown = r#"# Test

```tangle://top-level.txt
Top level content
```

- Item 1
- Item 2

  ```tangle://nested.txt
  This should NOT be extracted
  ```
"#;

        let snippets = parse_tangle_blocks(markdown);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].path, PathBuf::from("top-level.txt"));
        assert_eq!(snippets[0].content, "Top level content");
    }

    #[test]
    fn test_empty_markdown() {
        let markdown = "";
        let snippets = parse_tangle_blocks(markdown);
        assert_eq!(snippets.len(), 0);
    }

    #[test]
    fn test_no_tangle_blocks() {
        let markdown = r#"# Just a regular document

Some text here.

```rust
Regular code block
```

More text.
"#;

        let snippets = parse_tangle_blocks(markdown);
        assert_eq!(snippets.len(), 0);
    }

    #[test]
    fn test_tangle_with_subdirectory_path() {
        let markdown = r#"```tangle://src/modules/utils.rs
pub fn helper() {}
```"#;

        let snippets = parse_tangle_blocks(markdown);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].path, PathBuf::from("src/modules/utils.rs"));
        assert_eq!(snippets[0].content, "pub fn helper() {}");
    }

    #[test]
    fn test_empty_tangle_block() {
        let markdown = r#"```tangle://empty.txt
```"#;

        let snippets = parse_tangle_blocks(markdown);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].path, PathBuf::from("empty.txt"));
        assert_eq!(snippets[0].content, "");
    }
}
