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

/// Manages input and output directories for literate programming
#[derive(Debug)]
struct Lit {
    /// Input directory path
    input: PathBuf,
    /// Output directory path
    output: PathBuf,
}

/// Represents tangled files ready to be written to disk
#[derive(Debug)]
struct TangledFiles {
    /// Map from file path to concatenated content
    files: HashMap<PathBuf, String>,
}

impl TangledFiles {
    /// Write all tangled files to the specified output directory
    fn write_all(&self, output_dir: &PathBuf) -> Result<()> {
        for (path, content) in &self.files {
            let full_path = output_dir.join(path);

            // Create parent directories if they don't exist
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }

            // Write the file
            fs::write(&full_path, content)?;
        }

        Ok(())
    }

    /// Get the number of files
    fn len(&self) -> usize {
        self.files.len()
    }

    /// Check if there are no files
    fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl Lit {
    /// Create a new Lit instance with input and output directories
    fn new(input: PathBuf, output: PathBuf) -> Self {
        Lit { input, output }
    }

    /// Parse markdown content and extract code blocks with tangle:// paths
    fn parse_markdown(markdown_text: &str) -> HashMap<PathBuf, Vec<String>> {
        use markdown::mdast::Node;

        // Parse markdown to AST
        let ast = match to_mdast(markdown_text, &ParseOptions::default()) {
            Ok(ast) => ast,
            Err(_) => return HashMap::new(),
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

        files
    }

    /// Read all markdown files from input directory and parse tangle blocks
    fn read_blocks(&self) -> Result<HashMap<PathBuf, Vec<String>>> {
        let mut files: HashMap<PathBuf, Vec<String>> = HashMap::new();

        WalkDir::new(&self.input)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| {
                entry.path().extension()
                    .map_or(false, |ext| ext == "md")
            })
            .try_for_each(|entry| -> Result<()> {
                let content = fs::read_to_string(entry.path())?;
                let blocks = Self::parse_markdown(&content);

                // Merge the parsed blocks into our files HashMap
                for (path, contents) in blocks {
                    files.entry(path).or_insert_with(Vec::new).extend(contents);
                }
                Ok(())
            })?;

        Ok(files)
    }

    /// Create TangledFiles by concatenating all snippets for each file
    fn to_tangled_files(blocks: HashMap<PathBuf, Vec<String>>) -> TangledFiles {
        let files = blocks
            .iter()
            .map(|(path, snippets)| {
                let content = snippets.join("\n");
                (path.clone(), content)
            })
            .collect();

        TangledFiles { files }
    }

    /// Tangle the code blocks: read from input, parse, and write to output
    fn tangle(&self) -> Result<()> {
        let blocks = self.read_blocks()?;
        let tangled = Self::to_tangled_files(blocks);
        tangled.write_all(&self.output)?;
        Ok(())
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // For now, use a default output directory (could be made configurable)
    let output_dir = args.directory.join("output");

    println!("Reading markdown files from: {}", args.directory.display());
    println!("Writing tangled files to: {}\n", output_dir.display());

    let lit = Lit::new(args.directory, output_dir);
    lit.tangle()?;

    println!("Tangling complete!");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_tangle_block() {
        let markdown = r#"# Test

```tangle://src/main.rs
fn main() {
    println!("Hello");
}
```
"#;

        let blocks = Lit::parse_markdown(markdown);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks.get(&PathBuf::from("src/main.rs")).unwrap()[0], "fn main() {\n    println!(\"Hello\");\n}");
    }

    #[test]
    fn test_parse_multiple_tangle_blocks() {
        let markdown = r#"# Multiple Blocks

```tangle://file1.rs
code 1
```

Some text here.

```tangle://file2.rs
code 2
```
"#;

        let blocks = Lit::parse_markdown(markdown);
        assert_eq!(blocks.len(), 2);
        assert!(blocks.contains_key(&PathBuf::from("file1.rs")));
        assert!(blocks.contains_key(&PathBuf::from("file2.rs")));
        assert_eq!(blocks.get(&PathBuf::from("file1.rs")).unwrap()[0], "code 1");
        assert_eq!(blocks.get(&PathBuf::from("file2.rs")).unwrap()[0], "code 2");
    }

    #[test]
    fn test_parse_ignore_regular_code_blocks() {
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

        let blocks = Lit::parse_markdown(markdown);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks.get(&PathBuf::from("output.rs")).unwrap()[0], "// This should be extracted\nlet y = 10;");
    }

    #[test]
    fn test_parse_ignore_nested_in_blockquote() {
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

        let blocks = Lit::parse_markdown(markdown);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks.get(&PathBuf::from("top-level.txt")).unwrap()[0], "Top level content");
    }

    #[test]
    fn test_parse_ignore_nested_in_list() {
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

        let blocks = Lit::parse_markdown(markdown);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks.get(&PathBuf::from("top-level.txt")).unwrap()[0], "Top level content");
    }

    #[test]
    fn test_parse_empty_markdown() {
        let markdown = "";
        let blocks = Lit::parse_markdown(markdown);
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn test_parse_no_tangle_blocks() {
        let markdown = r#"# Just a regular document

Some text here.

```rust
Regular code block
```

More text.
"#;

        let blocks = Lit::parse_markdown(markdown);
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn test_parse_subdirectory_path() {
        let markdown = r#"```tangle://src/modules/utils.rs
pub fn helper() {}
```"#;

        let blocks = Lit::parse_markdown(markdown);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks.get(&PathBuf::from("src/modules/utils.rs")).unwrap()[0], "pub fn helper() {}");
    }

    #[test]
    fn test_parse_empty_tangle_block() {
        let markdown = r#"```tangle://empty.txt
```"#;

        let blocks = Lit::parse_markdown(markdown);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks.get(&PathBuf::from("empty.txt")).unwrap()[0], "");
    }

    #[test]
    fn test_to_tangled_files_concatenates_snippets() {
        let mut blocks = HashMap::new();
        blocks.insert(PathBuf::from("output.txt"), vec!["Line 1".to_string(), "Line 2".to_string()]);

        let tangled = Lit::to_tangled_files(blocks);

        assert_eq!(tangled.len(), 1);
        assert_eq!(tangled.files.get(&PathBuf::from("output.txt")), Some(&"Line 1\nLine 2".to_string()));
    }

    #[test]
    fn test_to_tangled_files_multiple_files() {
        let mut blocks = HashMap::new();
        blocks.insert(PathBuf::from("file1.txt"), vec!["Content 1".to_string()]);
        blocks.insert(PathBuf::from("file2.txt"), vec!["Content 2".to_string()]);

        let tangled = Lit::to_tangled_files(blocks);

        assert_eq!(tangled.len(), 2);
        assert_eq!(tangled.files.get(&PathBuf::from("file1.txt")), Some(&"Content 1".to_string()));
        assert_eq!(tangled.files.get(&PathBuf::from("file2.txt")), Some(&"Content 2".to_string()));
    }

    #[test]
    fn test_tangle_end_to_end() -> Result<()> {
        use std::env;

        // Create a temporary input directory with markdown files
        let temp_input = env::temp_dir().join("lit-test-input");
        let temp_output = env::temp_dir().join("lit-test-output");

        // Clean up if they exist
        if temp_input.exists() {
            fs::remove_dir_all(&temp_input)?;
        }
        if temp_output.exists() {
            fs::remove_dir_all(&temp_output)?;
        }

        // Create input directory and markdown file
        fs::create_dir_all(&temp_input)?;
        let markdown = r#"# Test

```tangle://test.txt
Hello World
```

```tangle://subdir/test2.txt
Nested file
```
"#;
        fs::write(temp_input.join("test.md"), markdown)?;

        // Create Lit and tangle
        let lit = Lit::new(temp_input.clone(), temp_output.clone());
        lit.tangle()?;

        // Verify the files were created
        assert!(temp_output.join("test.txt").exists());
        assert!(temp_output.join("subdir/test2.txt").exists());

        // Verify the content
        let content1 = fs::read_to_string(temp_output.join("test.txt"))?;
        assert_eq!(content1, "Hello World");

        let content2 = fs::read_to_string(temp_output.join("subdir/test2.txt"))?;
        assert_eq!(content2, "Nested file");

        // Clean up
        fs::remove_dir_all(&temp_input)?;
        fs::remove_dir_all(&temp_output)?;

        Ok(())
    }
}
