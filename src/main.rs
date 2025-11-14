use clap::Parser;
use color_eyre::{eyre::eyre, Result};
use markdown::{to_mdast, ParseOptions};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

/// Represents blocks for a single file, with positioned and unpositioned blocks separated
#[derive(Debug, Default)]
struct FileBlocks {
    /// Blocks with an explicit position (position_key, content)
    positioned: Vec<(String, String)>,
    /// Blocks without an explicit position
    unpositioned: Vec<String>,
}

impl FileBlocks {
    /// Add a positioned block, returning an error if the position key is not unique
    /// or if it contains non-alphabetic characters
    fn add_positioned(&mut self, at: String, content: String) -> Result<()> {
        // Validate that position key is not empty
        if at.is_empty() {
            return Err(eyre!("Position key must not be empty"));
        }

        // Validate that position key only contains alphabetic characters
        if !at.chars().all(|c| c.is_alphabetic()) {
            return Err(eyre!(
                "Position key '{}' must contain only alphabetic letters",
                at
            ));
        }

        // Disallow position keys starting with 'm' or 'M'
        if at.starts_with('m') || at.starts_with('M') {
            return Err(eyre!(
                "Position key '{}' must not start with 'm' or 'M'",
                at
            ));
        }

        if self.positioned.iter().any(|(p, _)| p == &at) {
            return Err(eyre!("Duplicate position key '{}' for the same file", at));
        }
        self.positioned.push((at, content));
        Ok(())
    }

    /// Add an unpositioned block
    fn add_unpositioned(&mut self, content: String) {
        self.unpositioned.push(content);
    }

    /// Get the concatenated content with blocks sorted lexicographically by position key.
    /// Unpositioned blocks are implicitly sorted at position "m".
    fn to_content(&self) -> String {
        // Collect all blocks with their effective sort keys
        let mut all_blocks: Vec<(&str, &str)> = Vec::new();

        // Add positioned blocks with their explicit keys
        for (at, content) in &self.positioned {
            all_blocks.push((at.as_str(), content.as_str()));
        }

        // Add unpositioned blocks with implicit "m" key
        for content in &self.unpositioned {
            all_blocks.push(("m", content.as_str()));
        }

        // Sort by position key lexicographically
        all_blocks.sort_by(|a, b| a.0.cmp(b.0));

        // Extract content and join
        all_blocks.iter().map(|(_, content)| *content).collect::<Vec<&str>>().join("\n")
    }
}

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

    /// Parse a tangle:// URL to extract the path and optional at parameter
    /// Returns (path, at) where at is Some(position_key) if present
    fn parse_tangle_url(url: &str) -> Option<(PathBuf, Option<String>)> {
        let stripped = url.strip_prefix("tangle://")?;

        if let Some((path, query)) = stripped.split_once('?') {
            // Parse query parameters - for now we only support at=value
            let at = query.strip_prefix("at=").map(|s| s.to_string());
            Some((PathBuf::from(path), at))
        } else {
            Some((PathBuf::from(stripped), None))
        }
    }

    /// Parse markdown content and extract code blocks with tangle:// paths
    fn parse_markdown(markdown_text: &str) -> Result<HashMap<PathBuf, FileBlocks>> {
        use markdown::mdast::Node;

        // Parse markdown to AST
        let ast = match to_mdast(markdown_text, &ParseOptions::default()) {
            Ok(ast) => ast,
            Err(_) => return Ok(HashMap::new()),
        };

        let mut files: HashMap<PathBuf, FileBlocks> = HashMap::new();

        // Extract snippets from top-level code blocks only
        if let Node::Root(root) = ast {
            for child in &root.children {
                if let Node::Code(code) = child {
                    if let Some(lang) = &code.lang {
                        if let Some((path, at)) = Self::parse_tangle_url(lang) {
                            let file_blocks = files.entry(path).or_insert_with(FileBlocks::default);

                            if let Some(at_key) = at {
                                file_blocks.add_positioned(at_key, code.value.clone())?;
                            } else {
                                file_blocks.add_unpositioned(code.value.clone());
                            }
                        }
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
            .filter(|entry| {
                entry.path().extension()
                    .map_or(false, |ext| ext == "md")
            })
            .try_for_each(|entry| -> Result<()> {
                let content = fs::read_to_string(entry.path())?;
                let blocks = Self::parse_markdown(&content)?;

                // Merge the parsed blocks into our files HashMap
                for (path, file_blocks) in blocks {
                    let target = files.entry(path).or_insert_with(FileBlocks::default);

                    // Add positioned blocks
                    for (at, content) in file_blocks.positioned {
                        target.add_positioned(at, content)?;
                    }

                    // Add unpositioned blocks
                    for content in file_blocks.unpositioned {
                        target.add_unpositioned(content);
                    }
                }
                Ok(())
            })?;

        Ok(files)
    }

    /// Create TangledFiles by concatenating all snippets for each file
    fn to_tangled_files(blocks: HashMap<PathBuf, FileBlocks>) -> TangledFiles {
        let files = blocks
            .iter()
            .map(|(path, file_blocks)| {
                let content = file_blocks.to_content();
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

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        let file_blocks = blocks.get(&PathBuf::from("src/main.rs")).unwrap();
        assert_eq!(file_blocks.unpositioned.len(), 1);
        assert_eq!(file_blocks.unpositioned[0], "fn main() {\n    println!(\"Hello\");\n}");
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
        assert_eq!(blocks.get(&PathBuf::from("file1.rs")).unwrap().unpositioned[0], "code 1");
        assert_eq!(blocks.get(&PathBuf::from("file2.rs")).unwrap().unpositioned[0], "code 2");
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
        assert_eq!(blocks.get(&PathBuf::from("output.rs")).unwrap().unpositioned[0], "// This should be extracted\nlet y = 10;");
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
        assert_eq!(blocks.get(&PathBuf::from("top-level.txt")).unwrap().unpositioned[0], "Top level content");
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
        assert_eq!(blocks.get(&PathBuf::from("top-level.txt")).unwrap().unpositioned[0], "Top level content");
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
        assert_eq!(blocks.get(&PathBuf::from("src/modules/utils.rs")).unwrap().unpositioned[0], "pub fn helper() {}");
    }

    #[test]
    fn test_parse_empty_tangle_block() {
        let markdown = r#"```tangle://empty.txt
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks.get(&PathBuf::from("empty.txt")).unwrap().unpositioned[0], "");
    }

    #[test]
    fn test_to_tangled_files_concatenates_snippets() {
        let mut blocks = HashMap::new();
        let mut file_blocks = FileBlocks::default();
        file_blocks.add_unpositioned("Line 1".to_string());
        file_blocks.add_unpositioned("Line 2".to_string());
        blocks.insert(PathBuf::from("output.txt"), file_blocks);

        let tangled = Lit::to_tangled_files(blocks);

        assert_eq!(tangled.len(), 1);
        assert_eq!(tangled.files.get(&PathBuf::from("output.txt")), Some(&"Line 1\nLine 2".to_string()));
    }

    #[test]
    fn test_to_tangled_files_multiple_files() {
        let mut blocks = HashMap::new();
        let mut file_blocks1 = FileBlocks::default();
        file_blocks1.add_unpositioned("Content 1".to_string());
        blocks.insert(PathBuf::from("file1.txt"), file_blocks1);

        let mut file_blocks2 = FileBlocks::default();
        file_blocks2.add_unpositioned("Content 2".to_string());
        blocks.insert(PathBuf::from("file2.txt"), file_blocks2);

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

    #[test]
    fn test_parse_block_with_at() {
        let markdown = r#"```tangle://output.txt?at=a
First block
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        let file_blocks = blocks.get(&PathBuf::from("output.txt")).unwrap();
        assert_eq!(file_blocks.positioned.len(), 1);
        assert_eq!(file_blocks.positioned[0].0, "a");
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
        assert_eq!(content, "First\nSecond\nThird");
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
        assert_eq!(content, "Before m\nUnpositioned 1\nUnpositioned 2\nAfter m");
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
        assert!(result.unwrap_err().to_string().contains("Duplicate position key"));
    }

    #[test]
    fn test_parse_tangle_url_without_at() {
        let result = Lit::parse_tangle_url("tangle://path/to/file.txt");
        assert_eq!(result, Some((PathBuf::from("path/to/file.txt"), None)));
    }

    #[test]
    fn test_parse_tangle_url_with_at() {
        let result = Lit::parse_tangle_url("tangle://path/to/file.txt?at=xyz");
        assert_eq!(result, Some((PathBuf::from("path/to/file.txt"), Some("xyz".to_string()))));
    }

    #[test]
    fn test_parse_tangle_url_with_query_but_no_at() {
        let result = Lit::parse_tangle_url("tangle://path/to/file.txt?other=value");
        assert_eq!(result, Some((PathBuf::from("path/to/file.txt"), None)));
    }

    #[test]
    fn test_parse_tangle_url_non_tangle() {
        let result = Lit::parse_tangle_url("rust");
        assert_eq!(result, None);
    }

    #[test]
    fn test_numeric_position_keys_rejected() {
        let markdown = r#"```tangle://output.txt?at=10
Ten
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must contain only alphabetic letters"));
    }

    #[test]
    fn test_position_key_with_numbers_rejected() {
        let markdown = r#"```tangle://output.txt?at=a1
Mixed
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must contain only alphabetic letters"));
    }

    #[test]
    fn test_position_key_with_special_chars_rejected() {
        let markdown = r#"```tangle://output.txt?at=a-b
Special
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must contain only alphabetic letters"));
    }

    #[test]
    fn test_empty_position_key_rejected() {
        let markdown = r#"```tangle://output.txt?at=
Empty
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must not be empty"));
    }

    #[test]
    fn test_position_key_starting_with_m_rejected() {
        let markdown = r#"```tangle://output.txt?at=main
Content
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must not start with 'm' or 'M'"));
    }

    #[test]
    fn test_position_key_starting_with_capital_m_rejected() {
        let markdown = r#"```tangle://output.txt?at=Main
Content
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must not start with 'm' or 'M'"));
    }

    #[test]
    fn test_position_key_just_m_rejected() {
        let markdown = r#"```tangle://output.txt?at=m
Content
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must not start with 'm' or 'M'"));
    }

    #[test]
    fn test_alphabetic_position_keys_allowed() {
        let markdown = r#"```tangle://output.txt?at=abc
First
```

```tangle://output.txt?at=xyz
Second
```

```tangle://output.txt?at=ABC
Third
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        let file_blocks = blocks.get(&PathBuf::from("output.txt")).unwrap();
        assert_eq!(file_blocks.positioned.len(), 3);
        let content = file_blocks.to_content();
        // Lexicographic: "ABC" < "abc" < "xyz"
        assert_eq!(content, "Third\nFirst\nSecond");
    }
}
