use clap::Parser;
use color_eyre::{eyre::bail, eyre::ensure, Result};
use markdown::{mdast::Node, ParseOptions, to_mdast};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;
use tracing::info;
use tracing_subscriber::EnvFilter;
use url::Url;
use walkdir::WalkDir;

/// Errors that can occur when validating a position key
#[derive(Debug, Error)]
enum PositionError {
    #[error("Position key must not be empty")]
    Empty,
    #[error("Position key '{0}' must contain only lowercase letters")]
    InvalidCharacters(String),
    #[error("Position key '{0}' must not start with 'm'")]
    ReservedPrefix(String),
}

/// Errors that can occur when parsing a block from a markdown node
#[derive(Debug, Error)]
enum BlockError {
    #[error("Node is not a Code node")]
    NotCodeNode,
    #[error("Code block has no language specified")]
    NoLanguage,
    #[error("Not a tangle URL")]
    NotTangleUrl,
    #[error("URL is not a tangle:// URL")]
    NotTangleScheme,
    #[error("Tangle URL missing host/path")]
    MissingPath,
    #[error(transparent)]
    PositionError(#[from] PositionError),
}

/// Represents a validated position key for ordering blocks
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Position(String);

impl TryFrom<String> for Position {
    type Error = PositionError;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(PositionError::Empty);
        }

        if !value.chars().all(|c| c.is_ascii_lowercase()) {
            return Err(PositionError::InvalidCharacters(value));
        }

        if value.starts_with('m') {
            return Err(PositionError::ReservedPrefix(value));
        }

        Ok(Position(value))
    }
}

impl AsRef<str> for Position {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Represents a single tangle block from markdown
#[derive(Debug, Clone)]
struct Block {
    /// The file path to write this block to
    path: PathBuf,
    /// Optional position key for ordering
    position: Option<Position>,
    /// The content of the code block
    content: String,
}

impl TryFrom<&Node> for Block {
    type Error = BlockError;

    fn try_from(node: &Node) -> std::result::Result<Self, Self::Error> {
        let Node::Code(code) = node else {
            return Err(BlockError::NotCodeNode);
        };

        let Some(lang) = &code.lang else {
            return Err(BlockError::NoLanguage);
        };

        // Parse the tangle:// URL
        let Ok(parsed) = Url::parse(lang) else {
            return Err(BlockError::NotTangleUrl);
        };

        // Verify it's a tangle:// URL
        if parsed.scheme() != "tangle" {
            return Err(BlockError::NotTangleScheme);
        }

        // Get the path (host + path for URLs like tangle://path/to/file)
        let Some(host) = parsed.host_str() else {
            return Err(BlockError::MissingPath);
        };

        let path_str = {
            let path = parsed.path();
            if path.is_empty() || path == "/" {
                host.to_string()
            } else {
                format!("{host}{path}")
            }
        };

        // Parse query parameters to extract the "at" parameter
        let position = parsed
            .query_pairs()
            .find(|(key, _)| key == "at")
            .map(|(_, value)| Position::try_from(value.to_string()))
            .transpose()?;

        Ok(Block {
            path: PathBuf::from(path_str),
            position,
            content: code.value.clone(),
        })
    }
}

/// Represents blocks for a single file, with positioned and unpositioned blocks separated
#[derive(Debug, Default)]
struct FileBlocks {
    /// Blocks with an explicit position (position_key, content)
    positioned: Vec<(Position, String)>,
    /// Blocks without an explicit position
    unpositioned: Vec<String>,
}

impl FileBlocks {
    /// Add a block with an optional position key.
    /// If at is Some, adds to positioned blocks.
    /// If at is None, adds to unpositioned blocks.
    fn add(&mut self, at: Option<Position>, content: String) -> Result<()> {
        match at {
            Some(at) => {
                ensure!(
                    !self.positioned.iter().any(|(p, _)| p == &at),
                    "Duplicate position key '{}' for the same file",
                    at.as_ref()
                );
                self.positioned.push((at, content));
            }
            None => {
                self.unpositioned.push(content);
            }
        }
        Ok(())
    }

    /// Get the concatenated content with blocks sorted lexicographically by position key.
    /// Unpositioned blocks are implicitly sorted at position "m".
    fn to_content(&self) -> String {
        let mut all_blocks: Vec<(&str, &str)> = Vec::new();

        for (at, content) in &self.positioned {
            all_blocks.push((at.as_ref(), content.as_str()));
        }

        // Add unpositioned blocks with implicit "m" key
        for content in &self.unpositioned {
            all_blocks.push(("m", content.as_str()));
        }

        all_blocks.sort_by(|a, b| a.0.cmp(b.0));

        let content = all_blocks
            .iter()
            .map(|(_, content)| *content)
            .collect::<Vec<&str>>()
            .join("\n\n");

        format!("{content}\n")
    }
}

#[derive(Parser, Debug)]
#[command(name = "lit")]
#[command(about = "A literate programming tool", long_about = None)]
struct Args {
    /// Input directory to process
    #[arg(value_name = "INPUT")]
    directory: PathBuf,

    /// Output directory for tangled files (defaults to INPUT/out)
    #[arg(value_name = "OUTPUT")]
    output: Option<PathBuf>,
}

/// Manages input and output directories for literate programming
#[derive(Debug)]
struct Lit {
    /// Input directory path
    input: PathBuf,
    /// Output directory path
    output: PathBuf,
}

impl Lit {
    /// Create a new Lit instance with input and output directories
    fn new(input: PathBuf, output: PathBuf) -> Self {
        Lit { input, output }
    }

    /// Parse markdown content and extract code blocks with tangle:// paths
    fn parse_markdown(markdown_text: &str) -> Result<HashMap<PathBuf, FileBlocks>> {
        let ast = match to_mdast(markdown_text, &ParseOptions::default()) {
            Ok(ast) => ast,
            Err(_) => return Ok(HashMap::new()),
        };

        let mut files: HashMap<PathBuf, FileBlocks> = HashMap::new();

        // Extract snippets from top-level code blocks only
        if let Node::Root(root) = ast {
            for child in &root.children {
                // Try to parse as a Block - skip if it's not a tangle block
                match Block::try_from(child) {
                    Ok(block) => {
                        let file_blocks = files.entry(block.path).or_default();
                        file_blocks.add(block.position, block.content)?;
                    }
                    Err(BlockError::PositionError(e)) => {
                        // Propagate position errors for tangle blocks
                        bail!(e);
                    }
                    Err(_) => {
                        // Skip non-tangle code blocks silently
                    }
                }
            }
        }

        Ok(files)
    }

    /// Read all markdown files from input directory and parse tangle blocks
    fn read_blocks(&self) -> Result<HashMap<PathBuf, FileBlocks>> {
        let mut files: HashMap<PathBuf, FileBlocks> = HashMap::new();

        WalkDir::new(&self.input)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
            .try_for_each(|entry| -> Result<()> {
                let content = fs::read_to_string(entry.path())?;
                let blocks = Self::parse_markdown(&content)?;

                for (path, file_blocks) in blocks {
                    let target = files.entry(path).or_default();

                    for (at, content) in file_blocks.positioned {
                        target.add(Some(at), content)?;
                    }

                    for content in file_blocks.unpositioned {
                        target.add(None, content)?;
                    }
                }
                Ok(())
            })?;

        Ok(files)
    }

    /// Tangle the code blocks: read from input, parse, and write to output
    fn tangle(&self) -> Result<()> {
        let blocks = self.read_blocks()?;

        for (path, file_blocks) in blocks {
            let content = file_blocks.to_content();
            let full_path = self.output.join(path);

            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::write(&full_path, content)?;
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let output = args.output.unwrap_or_else(|| args.directory.join("out"));

    let input_display = args.directory.display();
    let output_display = output.display();
    info!("Reading markdown files from: {input_display}");
    info!("Writing tangled files to: {output_display}");

    let lit = Lit::new(args.directory, output);
    lit.tangle()?;

    info!("Tangling complete!");

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

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        let file_blocks = blocks.get(&PathBuf::from("src/main.rs")).unwrap();
        assert_eq!(file_blocks.unpositioned.len(), 1);
        assert_eq!(
            file_blocks.unpositioned[0],
            "fn main() {\n    println!(\"Hello\");\n}"
        );
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

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 2);
        assert!(blocks.contains_key(&PathBuf::from("file1.rs")));
        assert!(blocks.contains_key(&PathBuf::from("file2.rs")));
        assert_eq!(
            blocks.get(&PathBuf::from("file1.rs")).unwrap().unpositioned[0],
            "code 1"
        );
        assert_eq!(
            blocks.get(&PathBuf::from("file2.rs")).unwrap().unpositioned[0],
            "code 2"
        );
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

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks
                .get(&PathBuf::from("output.rs"))
                .unwrap()
                .unpositioned[0],
            "// This should be extracted\nlet y = 10;"
        );
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

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks
                .get(&PathBuf::from("top-level.txt"))
                .unwrap()
                .unpositioned[0],
            "Top level content"
        );
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

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks
                .get(&PathBuf::from("top-level.txt"))
                .unwrap()
                .unpositioned[0],
            "Top level content"
        );
    }

    #[test]
    fn test_parse_empty_markdown() {
        let markdown = "";
        let blocks = Lit::parse_markdown(markdown).unwrap();
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

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn test_parse_subdirectory_path() {
        let markdown = r#"```tangle://src/modules/utils.rs
pub fn helper() {}
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks
                .get(&PathBuf::from("src/modules/utils.rs"))
                .unwrap()
                .unpositioned[0],
            "pub fn helper() {}"
        );
    }

    #[test]
    fn test_parse_empty_tangle_block() {
        let markdown = r#"```tangle://empty.txt
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks
                .get(&PathBuf::from("empty.txt"))
                .unwrap()
                .unpositioned[0],
            ""
        );
    }

    #[test]
    fn test_tangle_end_to_end() -> Result<()> {
        use std::env;

        let temp_input = env::temp_dir().join("lit-test-input");
        let temp_output = env::temp_dir().join("lit-test-output");

        if temp_input.exists() {
            fs::remove_dir_all(&temp_input)?;
        }
        if temp_output.exists() {
            fs::remove_dir_all(&temp_output)?;
        }

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

        let lit = Lit::new(temp_input.clone(), temp_output.clone());
        lit.tangle()?;

        assert!(temp_output.join("test.txt").exists());
        assert!(temp_output.join("subdir/test2.txt").exists());

        let content1 = fs::read_to_string(temp_output.join("test.txt"))?;
        assert_eq!(content1, "Hello World\n");

        let content2 = fs::read_to_string(temp_output.join("subdir/test2.txt"))?;
        assert_eq!(content2, "Nested file\n");

        fs::remove_dir_all(&temp_input)?;
        fs::remove_dir_all(&temp_output)?;

        Ok(())
    }

    #[test]
    fn test_parse_block_with_at() {
        let markdown = r#"```tangle://output.txt?at=a
First block
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        let file_blocks = blocks.get(&PathBuf::from("output.txt")).unwrap();
        assert_eq!(file_blocks.positioned.len(), 1);
        assert_eq!(file_blocks.positioned[0].0.as_ref(), "a");
        assert_eq!(file_blocks.positioned[0].1, "First block");
    }

    #[test]
    fn test_parse_multiple_blocks_with_different_positions() {
        let markdown = r#"```tangle://output.txt?at=c
Third block
```

```tangle://output.txt?at=a
First block
```

```tangle://output.txt?at=b
Second block
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        let file_blocks = blocks.get(&PathBuf::from("output.txt")).unwrap();
        assert_eq!(file_blocks.positioned.len(), 3);
    }

    #[test]
    fn test_positioned_blocks_sorted_lexicographically() {
        let markdown = r#"```tangle://output.txt?at=c
Third
```

```tangle://output.txt?at=a
First
```

```tangle://output.txt?at=b
Second
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        let file_blocks = blocks.get(&PathBuf::from("output.txt")).unwrap();
        let content = file_blocks.to_content();
        assert_eq!(content, "First\n\nSecond\n\nThird\n");
    }

    #[test]
    fn test_positioned_blocks_around_implicit_m() {
        let markdown = r#"```tangle://output.txt
Unpositioned 1
```

```tangle://output.txt?at=a
Before m
```

```tangle://output.txt?at=z
After m
```

```tangle://output.txt
Unpositioned 2
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        let file_blocks = blocks.get(&PathBuf::from("output.txt")).unwrap();
        let content = file_blocks.to_content();
        // "a" < "m" (implicit for unpositioned) < "z"
        assert_eq!(
            content,
            "Before m\n\nUnpositioned 1\n\nUnpositioned 2\n\nAfter m\n"
        );
    }

    #[test]
    fn test_duplicate_position_key_returns_error() {
        let markdown = r#"```tangle://output.txt?at=a
First
```

```tangle://output.txt?at=a
Duplicate
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate position key")
        );
    }

    #[test]
    fn test_block_from_node_without_at() {
        use markdown::mdast::{Code, Node};

        let code = Node::Code(Code {
            lang: Some("tangle://path/to/file.txt".to_string()),
            value: "test content".to_string(),
            meta: None,
            position: None,
        });

        let block = Block::try_from(&code).unwrap();
        assert_eq!(block.path, PathBuf::from("path/to/file.txt"));
        assert_eq!(block.position, None);
        assert_eq!(block.content, "test content");
    }

    #[test]
    fn test_block_from_node_with_at() {
        use markdown::mdast::{Code, Node};

        let code = Node::Code(Code {
            lang: Some("tangle://path/to/file.txt?at=xyz".to_string()),
            value: "test content".to_string(),
            meta: None,
            position: None,
        });

        let block = Block::try_from(&code).unwrap();
        assert_eq!(block.path, PathBuf::from("path/to/file.txt"));
        assert_eq!(
            block.position.as_ref().map(|p| p.as_ref()),
            Some("xyz")
        );
        assert_eq!(block.content, "test content");
    }

    #[test]
    fn test_block_from_node_with_query_but_no_at() {
        use markdown::mdast::{Code, Node};

        let code = Node::Code(Code {
            lang: Some("tangle://path/to/file.txt?other=value".to_string()),
            value: "test content".to_string(),
            meta: None,
            position: None,
        });

        let block = Block::try_from(&code).unwrap();
        assert_eq!(block.path, PathBuf::from("path/to/file.txt"));
        assert_eq!(block.position, None);
    }

    #[test]
    fn test_block_from_node_non_tangle() {
        use markdown::mdast::{Code, Node};

        let code = Node::Code(Code {
            lang: Some("rust".to_string()),
            value: "test content".to_string(),
            meta: None,
            position: None,
        });

        let result = Block::try_from(&code);
        assert!(result.is_err());
    }

    #[test]
    fn test_numeric_position_keys_rejected() {
        let markdown = r#"```tangle://output.txt?at=10
Ten
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must contain only lowercase letters")
        );
    }

    #[test]
    fn test_position_key_with_numbers_rejected() {
        let markdown = r#"```tangle://output.txt?at=a1
Mixed
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must contain only lowercase letters")
        );
    }

    #[test]
    fn test_position_key_with_special_chars_rejected() {
        let markdown = r#"```tangle://output.txt?at=a-b
Special
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must contain only lowercase letters")
        );
    }

    #[test]
    fn test_empty_position_key_rejected() {
        let markdown = r#"```tangle://output.txt?at=
Empty
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must not be empty")
        );
    }

    #[test]
    fn test_position_key_starting_with_m_rejected() {
        let markdown = r#"```tangle://output.txt?at=main
Content
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must not start with 'm'")
        );
    }

    #[test]
    fn test_position_key_starting_with_capital_m_rejected() {
        let markdown = r#"```tangle://output.txt?at=Main
Content
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must contain only lowercase letters")
        );
    }

    #[test]
    fn test_position_key_just_m_rejected() {
        let markdown = r#"```tangle://output.txt?at=m
Content
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must not start with 'm'")
        );
    }

    #[test]
    fn test_lowercase_position_keys_allowed() {
        let markdown = r#"```tangle://output.txt?at=abc
First
```

```tangle://output.txt?at=xyz
Second
```

```tangle://output.txt?at=def
Third
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        let file_blocks = blocks.get(&PathBuf::from("output.txt")).unwrap();
        assert_eq!(file_blocks.positioned.len(), 3);
        let content = file_blocks.to_content();
        // Lexicographic: "abc" < "def" < "xyz"
        assert_eq!(content, "First\n\nThird\n\nSecond\n");
    }

    #[test]
    fn test_uppercase_position_key_rejected() {
        let markdown = r#"```tangle://output.txt?at=ABC
Content
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must contain only lowercase letters")
        );
    }

    #[test]
    fn test_mixed_case_position_key_rejected() {
        let markdown = r#"```tangle://output.txt?at=aBc
Content
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must contain only lowercase letters")
        );
    }

    #[test]
    fn test_tangled_files_end_with_newline() -> Result<()> {
        use std::env;

        let temp_input = env::temp_dir().join("lit-test-newline-input");
        let temp_output = env::temp_dir().join("lit-test-newline-output");

        if temp_input.exists() {
            fs::remove_dir_all(&temp_input)?;
        }
        if temp_output.exists() {
            fs::remove_dir_all(&temp_output)?;
        }

        fs::create_dir_all(&temp_input)?;
        let markdown = r#"# Test

```tangle://test.txt
Line 1
```
"#;
        fs::write(temp_input.join("test.md"), markdown)?;

        let lit = Lit::new(temp_input.clone(), temp_output.clone());
        lit.tangle()?;

        let content = fs::read_to_string(temp_output.join("test.txt"))?;
        assert!(
            content.ends_with('\n'),
            "Tangled file should end with a newline"
        );

        fs::remove_dir_all(&temp_input)?;
        fs::remove_dir_all(&temp_output)?;

        Ok(())
    }
}
