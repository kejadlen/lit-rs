# Lit - A Literate Programming Tool

Lit extracts code from Markdown files and tangles them into source files. This
literate programming tool, written in Rust, enables a documentation-first
development approach.

## Overview

Code blocks marked with `tangle:///` URLs tell Lit where to write them. Each
URL specifies a destination file; an optional `?at=` parameter specifies
position. Lit reads markdown files, extracts these blocks, groups them by
destination, sorts by position, and writes each group to its target file.

## Usage

Lit takes an input directory and an output directory.

```tangle:///src/lib.rs
#[derive(Debug)]
pub struct Lit {
    pub input: PathBuf,
    pub output: PathBuf,
}
```

`tangle` is the main entry point. It reads blocks, renders each file, creates
directories, and writes output.

```tangle:///src/lib.rs?at=ll
    pub fn tangle(&self) -> Result<()> {
        let files = self.read_blocks()?;

        files.into_iter().try_for_each(|file| -> Result<()> {
            let content = file.render();

            let full_path = self.output.join(&file.path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            info!("Writing {}", full_path.display());
            fs::write(&full_path, content)?;

            Ok(())
        })
    }
```

### Parsing Markdown

`parse_markdown` converts markdown text into blocks. It builds an AST using the
`markdown` crate, then extracts top-level code blocks only (ignoring nested
blocks in quotes or lists).

````tangle:///src/lib.rs?at=ll
    /// Parse markdown content and extract code blocks with tangle:// paths
    pub fn parse_markdown(markdown_text: &str) -> Result<Vec<Block>> {
        let ast = to_mdast(markdown_text, &ParseOptions::default())
            .map_err(|e| color_eyre::eyre::eyre!("Failed to parse markdown: {}", e))?;

        let Node::Root(root) = ast else {
            bail!("Expected root node in markdown AST");
        };

        // Extract snippets from top-level code blocks only
        root.children
            .iter()
            .map(Block::try_from)
            .filter_map(|result| match result {
                Ok(block) => Some(Ok(block)),
                Err(BlockError::NotTangleBlock) => None,
                Err(e) => Some(Err(e.into())),
            })
            .collect()
    }
````

### Reading Input Files

`read_blocks` walks the input directory, parses all `.md` files, and groups blocks by destination.

````tangle:///src/lib.rs?at=ll
    /// Read all markdown files from input directory and parse tangle blocks
    pub fn read_blocks(&self) -> Result<Vec<TangledFile>> {
        let mut files = HashMap::<PathBuf, Vec<Block>>::new();

        for entry in WalkDir::new(&self.input)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
        {
            let content = fs::read_to_string(entry.path())?;
            let blocks = Self::parse_markdown(&content)?;

            for block in blocks {
                files.entry(block.path.clone()).or_default().push(block);
            }
        }

        Ok(files
            .into_iter()
            .map(|(path, mut blocks)| {
                blocks.sort();
                TangledFile::new(path, blocks)
            })
            .collect())
    }
````

### Lit Setup

```tangle:///src/lib.rs?at=la
impl Lit {
    pub fn new(input: PathBuf, output: PathBuf) -> Self {
        Lit { input, output }
    }
```

```tangle:///src/lib.rs?at=lz
}
```


### Tests

The tests verify parsing, end-to-end tangling, and file-writing behavior.

````tangle:///src/lib.rs?at=z
    #[test]
    fn test_parse_single_tangle_block() {
        let markdown = r#"# Test

```tangle:///src/main.rs
fn main() {
    println!("Hello");
}
```
"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(blocks[0].position, Position::middle());
        assert_eq!(
            blocks[0].content,
            "fn main() {\n    println!(\"Hello\");\n}"
        );
    }

    #[test]
    fn test_parse_multiple_tangle_blocks() {
        let markdown = r#"# Multiple Blocks

```tangle:///file1.rs
code 1
```

Some text here.

```tangle:///file2.rs
code 2
```
"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].path, PathBuf::from("file1.rs"));
        assert_eq!(blocks[0].content, "code 1");
        assert_eq!(blocks[1].path, PathBuf::from("file2.rs"));
        assert_eq!(blocks[1].content, "code 2");
    }

    #[test]
    fn test_parse_ignore_regular_code_blocks() {
        let markdown = r#"# Test

```rust
// This is regular code
let x = 42;
```

```tangle:///output.rs
// This should be extracted
let y = 10;
```
"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("output.rs"));
        assert_eq!(
            blocks[0].content,
            "// This should be extracted\nlet y = 10;"
        );
    }

    #[test]
    fn test_parse_ignore_nested_in_blockquote() {
        let markdown = r#"# Test

```tangle:///top-level.txt
Top level content
```

> Blockquote here
>
> ```tangle:///nested.txt
> This should NOT be extracted
> ```
"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("top-level.txt"));
        assert_eq!(blocks[0].content, "Top level content");
    }

    #[test]
    fn test_parse_ignore_nested_in_list() {
        let markdown = r#"# Test

```tangle:///top-level.txt
Top level content
```

- Item 1
- Item 2

  ```tangle:///nested.txt
  This should NOT be extracted
  ```
"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("top-level.txt"));
        assert_eq!(blocks[0].content, "Top level content");
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
        let markdown = r#"```tangle:///src/modules/utils.rs
pub fn helper() {}
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("src/modules/utils.rs"));
        assert_eq!(blocks[0].content, "pub fn helper() {}");
    }

    #[test]
    fn test_parse_empty_tangle_block() {
        let markdown = r#"```tangle:///empty.txt
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("empty.txt"));
        assert_eq!(blocks[0].content, "");
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

```tangle:///test.txt
Hello World
```

```tangle:///subdir/test2.txt
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

```tangle:///test.txt
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
````

## Tangled Files

`TangledFile` groups blocks destined for the same output file. In the future, a
potential improvement could be to store the blocks in a data structure that
guarantees sorting, but for now, the constructor enforces sorted blocks as an
invariant. When rendering, it concatenates blocks with double newlines.

```tangle:///src/lib.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TangledFile {
    pub path: PathBuf,
    pub blocks: Vec<Block>,
}

impl TangledFile {
    pub fn new(path: PathBuf, blocks: Vec<Block>) -> Self {
        assert!(blocks.is_sorted(), "blocks must be sorted by position");
        TangledFile { path, blocks }
    }

    pub fn render(&self) -> String {
        let content = self
            .blocks
            .iter()
            .map(|b| b.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        format!("{content}\n")
    }
}
```

### Tests

````tangle:///src/lib.rs?at=z
    #[test]
    #[should_panic(expected = "blocks must be sorted by position")]
    fn test_tangled_file_new_panics_on_unsorted_blocks() {
        let block1 = Block {
            path: PathBuf::from("test.txt"),
            position: Position::try_from("z".to_string()).unwrap(),
            content: "last".to_string(),
        };
        let block2 = Block {
            path: PathBuf::from("test.txt"),
            position: Position::try_from("a".to_string()).unwrap(),
            content: "first".to_string(),
        };

        // blocks are intentionally unsorted (z before a)
        TangledFile::new(PathBuf::from("test.txt"), vec![block1, block2]);
    }
````

## Blocks

`Block` represents a single code snippet extracted from markdown:
- Destination file path
- Position for ordering (defaults to "m")
- Code content

Position alone determines block order, enabling fine-grained control over final output even when blocks scatter across multiple markdown files.

### Block Structure

````tangle:///src/lib.rs?at=f
/// Represents a single tangle block from markdown
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The file path to write this block to
    pub path: PathBuf,
    /// Position key for ordering (defaults to "m" for unpositioned blocks)
    pub position: Position,
    /// The content of the code block
    pub content: String,
}

impl PartialOrd for Block {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Block {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.position.cmp(&other.position)
    }
}
````

### Parsing from Markdown

To convert a markdown AST node into `Block`, the code validates:
1. Code block has a language tag
2. Language tag is a `tangle:///` URL
3. URL format is hostless
4. File path and optional position parameter exist

````tangle:///src/lib.rs?at=f
impl TryFrom<&Node> for Block {
    type Error = BlockError;

    fn try_from(node: &Node) -> std::result::Result<Self, Self::Error> {
        let Node::Code(code) = node else {
            return Err(BlockError::NotTangleBlock);
        };

        let lang = code.lang.as_ref().ok_or(BlockError::NotTangleBlock)?;

        // Parse the tangle:/// URL (hostless format)
        let parsed = Url::parse(lang).map_err(|_| BlockError::NotTangleBlock)?;

        // Check if it's a tangle URL
        if parsed.scheme() != "tangle" {
            return Err(BlockError::NotTangleBlock);
        }

        // Ensure it's hostless (tangle:///path, not tangle://path)
        if parsed.host_str().is_some() {
            return Err(BlockError::InvalidTangleUrl);
        }

        // Get the path from hostless URL (tangle:///path/to/file)
        let path = parsed.path();
        if path.is_empty() || path == "/" {
            return Err(BlockError::MissingPath);
        }
        if path.starts_with("//") {
            return Err(BlockError::InvalidPath);
        }
        // Strip the single leading slash to get a relative path
        let path_str = path.strip_prefix('/').unwrap().to_string();

        // Parse query parameters to extract the "at" parameter
        let position = parsed
            .query_pairs()
            .find(|(key, _)| key == "at")
            .map(|(_, value)| Position::try_from(value.to_string()))
            .transpose()?
            .unwrap_or_else(Position::middle);

        Ok(Block {
            path: PathBuf::from(path_str),
            position,
            content: code.value.clone(),
        })
    }
}
````

### Block Parsing Errors

Parsing tangle blocks from markdown fails when URLs are malformed, required components are missing, or formats are invalid.

````tangle:///src/lib.rs?at=f
/// Errors that can occur when parsing a block from a markdown node
#[derive(Debug, Error)]
pub enum BlockError {
    #[error("Not a tangle block")]
    NotTangleBlock,
    #[error("Tangle URL must be hostless (use tangle:///path, not tangle://path)")]
    InvalidTangleUrl,
    #[error("Tangle URL missing path")]
    MissingPath,
    #[error("Invalid tangle URL path")]
    InvalidPath,
    #[error(transparent)]
    PositionError(#[from] PositionError),
}
````

### Tests

The tests verify Block parsing, validation, and sorting.

````tangle:///src/lib.rs?at=z
    #[test]
    fn test_parse_block_with_at() {
        let markdown = r#"```tangle:///output.txt?at=a
First block
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("output.txt"));
        assert_eq!(blocks[0].position.as_ref(), "a");
        assert_eq!(blocks[0].content, "First block");
    }

    #[test]
    fn test_parse_multiple_blocks_with_different_positions() {
        let markdown = r#"```tangle:///output.txt?at=c
Third block
```

```tangle:///output.txt?at=a
First block
```

```tangle:///output.txt?at=b
Second block
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 3);
        assert!(blocks.iter().all(|b| b.path == PathBuf::from("output.txt")));
        assert!(blocks.iter().all(|b| b.position.as_ref() != "m"));
    }

    #[test]
    fn test_positioned_blocks_sorted_lexicographically() {
        let markdown = r#"```tangle:///output.txt?at=c
Third
```

```tangle:///output.txt?at=a
First
```

```tangle:///output.txt?at=b
Second
```"#;

        let mut blocks = Lit::parse_markdown(markdown).unwrap();
        blocks.sort();
        let content = blocks
            .iter()
            .map(|b| b.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let content = format!("{content}\n");
        assert_eq!(content, "First\n\nSecond\n\nThird\n");
    }

    #[test]
    fn test_positioned_blocks_around_implicit_m() {
        let markdown = r#"```tangle:///output.txt
Unpositioned 1
```

```tangle:///output.txt?at=a
Before m
```

```tangle:///output.txt?at=z
After m
```

```tangle:///output.txt
Unpositioned 2
```"#;

        let mut blocks = Lit::parse_markdown(markdown).unwrap();
        blocks.sort();
        let content = blocks
            .iter()
            .map(|b| b.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let content = format!("{content}\n");
        // "a" < "m" (implicit for unpositioned) < "z"
        assert_eq!(
            content,
            "Before m\n\nUnpositioned 1\n\nUnpositioned 2\n\nAfter m\n"
        );
    }

    #[test]
    fn test_duplicate_position_keys_allowed() {
        let markdown = r#"```tangle:///output.txt?at=a
First
```

```tangle:///output.txt?at=a
Duplicate
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].path, PathBuf::from("output.txt"));
        assert_eq!(blocks[0].position.as_ref(), "a");
        assert_eq!(blocks[1].path, PathBuf::from("output.txt"));
        assert_eq!(blocks[1].position.as_ref(), "a");

        // Sort and concatenate like tangle does
        let mut blocks = blocks;
        blocks.sort();
        let content = blocks
            .iter()
            .map(|b| b.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let content = format!("{content}\n");
        assert_eq!(content, "First\n\nDuplicate\n");
    }

    #[test]
    fn test_block_from_node_without_at() {
        use markdown::mdast::{Code, Node};

        let code = Node::Code(Code {
            lang: Some("tangle:///path/to/file.txt".to_string()),
            value: "test content".to_string(),
            meta: None,
            position: None,
        });

        let block = Block::try_from(&code).unwrap();
        assert_eq!(block.path, PathBuf::from("path/to/file.txt"));
        assert_eq!(block.position, Position::middle());
        assert_eq!(block.content, "test content");
    }

    #[test]
    fn test_block_from_node_with_at() {
        use markdown::mdast::{Code, Node};

        let code = Node::Code(Code {
            lang: Some("tangle:///path/to/file.txt?at=xyz".to_string()),
            value: "test content".to_string(),
            meta: None,
            position: None,
        });

        let block = Block::try_from(&code).unwrap();
        assert_eq!(block.path, PathBuf::from("path/to/file.txt"));
        assert_eq!(block.position.as_ref(), "xyz");
        assert_eq!(block.content, "test content");
    }

    #[test]
    fn test_block_from_node_with_query_but_no_at() {
        use markdown::mdast::{Code, Node};

        let code = Node::Code(Code {
            lang: Some("tangle:///path/to/file.txt?other=value".to_string()),
            value: "test content".to_string(),
            meta: None,
            position: None,
        });

        let block = Block::try_from(&code).unwrap();
        assert_eq!(block.path, PathBuf::from("path/to/file.txt"));
        assert_eq!(block.position, Position::middle());
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
    fn test_block_with_host_rejected() {
        let markdown = r#"```tangle://path/to/file.txt
test content
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be hostless"));
    }

    #[test]
    fn test_block_with_double_slash_path_rejected() {
        let markdown = r#"```tangle:////absolute/path.txt
test content
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid tangle URL path")
        );
    }
````

## Position System

The `?at=` parameter specifies position keys that control block order. For example, `tangle:///file.txt?at=a` precedes `tangle:///file.txt?at=z`. Blocks without explicit positions default to "m" (middle), allowing positioned blocks before or after.

### Valid Positions

Position keys must not be empty to ensure every position has a defined sort order.

```tangle:///src/lib.rs?at=e
    #[error("Position key must not be empty")]
    Empty,
```

```tangle:///src/lib.rs?at=d
        if value.is_empty() {
            return Err(PositionError::Empty);
        }
```

````tangle:///src/lib.rs?at=z
    #[test]
    fn test_empty_position_key_rejected() {
        let markdown = r#"```tangle:///output.txt?at=
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
````

Positions consist only of lowercase characters.

```tangle:///src/lib.rs?at=e
    #[error("Position key '{0}' must contain only lowercase letters")]
    InvalidCharacters(String),
```

```tangle:///src/lib.rs?at=d
        if !value.chars().all(|c| c.is_ascii_lowercase()) {
            return Err(PositionError::InvalidCharacters(value));
        }
```

````tangle:///src/lib.rs?at=z
    #[test]
    fn test_numeric_position_keys_rejected() {
        let markdown = r#"```tangle:///output.txt?at=10
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
        let markdown = r#"```tangle:///output.txt?at=a1
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
        let markdown = r#"```tangle:///output.txt?at=a-b
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
    fn test_position_key_starting_with_m_allowed() {
        let markdown = r#"```tangle:///output.txt?at=main
Content
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("output.txt"));
        assert_eq!(blocks[0].position.as_ref(), "main");
        assert_eq!(blocks[0].content, "Content");
    }

    #[test]
    fn test_position_key_starting_with_capital_m_rejected() {
        let markdown = r#"```tangle:///output.txt?at=Main
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
    fn test_position_key_just_m_allowed() {
        let markdown = r#"```tangle:///output.txt?at=m
Content
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("output.txt"));
        assert_eq!(blocks[0].position.as_ref(), "m");
        assert_eq!(blocks[0].content, "Content");
    }

    #[test]
    fn test_lowercase_position_keys_allowed() {
        let markdown = r#"```tangle:///output.txt?at=abc
First
```

```tangle:///output.txt?at=xyz
Second
```

```tangle:///output.txt?at=def
Third
```"#;

        let mut blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 3);
        blocks.sort();
        let content = blocks
            .iter()
            .map(|b| b.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let content = format!("{content}\n");
        // Lexicographic: "abc" < "def" < "xyz"
        assert_eq!(content, "First\n\nThird\n\nSecond\n");
    }

    #[test]
    fn test_uppercase_position_key_rejected() {
        let markdown = r#"```tangle:///output.txt?at=ABC
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
        let markdown = r#"```tangle:///output.txt?at=aBc
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
````

### Position Setup

```tangle:///src/lib.rs?at=bz
/// Represents a validated position key for ordering blocks
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position(String);

impl Position {
    /// Creates the implicit middle position "m" for unpositioned blocks.
    pub fn middle() -> Self {
        Position("m".to_string())
    }
}

impl TryFrom<String> for Position {
    type Error = PositionError;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
```

```tangle:///src/lib.rs?at=dz
        Ok(Position(value))
    }
}

impl AsRef<str> for Position {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Errors that can occur when validating a position key
#[derive(Debug, Error)]
pub enum PositionError {
```

```tangle:///src/lib.rs?at=ez
}
```

## Test Setup

```tangle:///src/lib.rs?at=yz
#[cfg(test)]
mod tests {
    use super::*;
```

```tangle:///src/lib.rs?at=zz
}
```

