use clap::Parser;
use color_eyre::Result;
use markdown::{to_mdast, ParseOptions};
use std::collections::HashMap;
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

/// Manages tangle blocks grouped by file path
#[derive(Debug)]
struct Lit {
    /// Input directory path (if applicable)
    input: Option<PathBuf>,
    /// Map from file path to list of content snippets for that file
    output: HashMap<PathBuf, Vec<String>>,
}

impl Lit {
    /// Parse markdown content and extract code blocks with tangle:// paths
    fn from_markdown(markdown_text: &str) -> Self {
        use markdown::mdast::Node;

        // Parse markdown to AST
        let ast = match to_mdast(markdown_text, &ParseOptions::default()) {
            Ok(ast) => ast,
            Err(_) => return Lit { input: None, output: HashMap::new() },
        };

        let mut files: HashMap<PathBuf, Vec<String>> = HashMap::new();

        // Extract snippets from top-level code blocks only
        if let Node::Root(root) = ast {
            root.children.iter()
                .filter_map(|child| match child {
                    Node::Code(code) => Some(code),
                    _ => None,
                })
                .filter_map(|code| {
                    code.lang.as_ref()
                        .and_then(|lang| lang.strip_prefix("tangle://"))
                        .map(|path_str| (path_str, &code.value))
                })
                .for_each(|(path_str, value)| {
                    files
                        .entry(PathBuf::from(path_str))
                        .or_insert_with(Vec::new)
                        .push(value.clone());
                });
        }

        Lit { input: None, output: files }
    }

    /// Walk a directory and parse all markdown files, collecting tangle blocks
    fn from_directory(directory: &PathBuf) -> Result<Self> {
        let mut files: HashMap<PathBuf, Vec<String>> = HashMap::new();

        WalkDir::new(directory)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| {
                entry.path().extension()
                    .map_or(false, |ext| ext == "md")
            })
            .try_for_each(|entry| -> Result<()> {
                let content = fs::read_to_string(entry.path())?;
                let lit = Lit::from_markdown(&content);

                // Merge the parsed lit into our files HashMap
                for (path, contents) in lit.output {
                    files.entry(path).or_insert_with(Vec::new).extend(contents);
                }
                Ok(())
            })?;

        Ok(Lit { input: Some(directory.clone()), output: files })
    }

    /// Create an iterator over all snippets as (path, content) tuples
    fn iter(&self) -> impl Iterator<Item = (&PathBuf, &str)> + '_ {
        self.output.iter().flat_map(|(path, contents)| {
            contents.iter().map(move |content| (path, content.as_str()))
        })
    }

    /// Get the total number of snippets across all files
    fn len(&self) -> usize {
        self.output.values().map(|v| v.len()).sum()
    }

    /// Check if there are no snippets
    fn is_empty(&self) -> bool {
        self.output.is_empty()
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    println!("Processing markdown files in {}:\n", args.directory.display());

    let lit = Lit::from_directory(&args.directory)?;

    if lit.is_empty() {
        println!("No tangle blocks found");
    } else {
        println!("Found {} tangle block(s) across all files:\n", lit.len());
        for (path, content) in lit.iter() {
            println!("  → {}", path.display());
            println!("    {} lines", content.lines().count());
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

        let lit = Lit::from_markdown(markdown);
        assert_eq!(lit.len(), 1);

        let snippets: Vec<_> = lit.iter().collect();
        assert_eq!(snippets[0].0, &PathBuf::from("src/main.rs"));
        assert_eq!(snippets[0].1, "fn main() {\n    println!(\"Hello\");\n}");
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

        let lit = Lit::from_markdown(markdown);
        assert_eq!(lit.len(), 2);

        let snippets: Vec<_> = lit.iter().collect();
        // HashMap doesn't guarantee order, so check both snippets exist
        assert!(snippets.iter().any(|(path, content)| path == &&PathBuf::from("file1.rs") && *content == "code 1"));
        assert!(snippets.iter().any(|(path, content)| path == &&PathBuf::from("file2.rs") && *content == "code 2"));
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

        let lit = Lit::from_markdown(markdown);
        assert_eq!(lit.len(), 1);

        let snippets: Vec<_> = lit.iter().collect();
        assert_eq!(snippets[0].0, &PathBuf::from("output.rs"));
        assert_eq!(snippets[0].1, "// This should be extracted\nlet y = 10;");
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

        let lit = Lit::from_markdown(markdown);
        assert_eq!(lit.len(), 1);

        let snippets: Vec<_> = lit.iter().collect();
        assert_eq!(snippets[0].0, &PathBuf::from("top-level.txt"));
        assert_eq!(snippets[0].1, "Top level content");
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

        let lit = Lit::from_markdown(markdown);
        assert_eq!(lit.len(), 1);

        let snippets: Vec<_> = lit.iter().collect();
        assert_eq!(snippets[0].0, &PathBuf::from("top-level.txt"));
        assert_eq!(snippets[0].1, "Top level content");
    }

    #[test]
    fn test_empty_markdown() {
        let markdown = "";
        let lit = Lit::from_markdown(markdown);
        assert_eq!(lit.len(), 0);
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

        let lit = Lit::from_markdown(markdown);
        assert_eq!(lit.len(), 0);
    }

    #[test]
    fn test_tangle_with_subdirectory_path() {
        let markdown = r#"```tangle://src/modules/utils.rs
pub fn helper() {}
```"#;

        let lit = Lit::from_markdown(markdown);
        assert_eq!(lit.len(), 1);

        let snippets: Vec<_> = lit.iter().collect();
        assert_eq!(snippets[0].0, &PathBuf::from("src/modules/utils.rs"));
        assert_eq!(snippets[0].1, "pub fn helper() {}");
    }

    #[test]
    fn test_empty_tangle_block() {
        let markdown = r#"```tangle://empty.txt
```"#;

        let lit = Lit::from_markdown(markdown);
        assert_eq!(lit.len(), 1);

        let snippets: Vec<_> = lit.iter().collect();
        assert_eq!(snippets[0].0, &PathBuf::from("empty.txt"));
        assert_eq!(snippets[0].1, "");
    }
}
