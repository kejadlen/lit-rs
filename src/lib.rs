use fs_err as fs;
use markdown::ParseOptions;
use markdown::mdast::Node;
use markdown::to_mdast;
use miette::Diagnostic;
use petgraph::Direction;
use petgraph::graph::DiGraph;
use petgraph::graph::NodeIndex;
use regex::Regex;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;
use thiserror::Error;
use tracing::info;
use url::Url;
use walkdir::WalkDir;

#[derive(Debug)]
pub struct Lit {
    pub input: PathBuf,
    pub output: PathBuf,
}

impl Lit {
    pub fn new(input: PathBuf, output: PathBuf) -> Self {
        Lit { input, output }
    }

    pub fn tangle(&self) -> Result<()> {
        let files = self.read_blocks()?;

        for file in files {
            let content = file.render();

            let full_path = self.output.join(&file.path);
            // Tangle paths always have at least '/' as parent, so this cannot fail.
            #[allow(clippy::unwrap_used)]
            let parent = full_path.parent().unwrap();
            fs::create_dir_all(parent)?;
            info!("Writing {}", full_path.display());
            fs::write(&full_path, content)?;
        }

        Ok(())
    }

    /// Parse markdown content and extract code blocks with tangle:// paths
    pub fn parse_markdown(markdown_text: &str) -> Result<Vec<Block>> {
        let ast = to_mdast(markdown_text, &ParseOptions::default())
            .map_err(|e| LitError::Markdown(e.to_string()))?;

        let Node::Root(root) = ast else {
            return Err(LitError::NotRoot); // cov-excl-line: unreachable — to_mdast always returns Root
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

        files
            .into_iter()
            .map(|(path, blocks)| {
                let sorted_blocks = solve_block_order(&blocks)?;
                Ok(TangledFile::new(path, sorted_blocks))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    #[test]
    fn test_parse_block_with_id_and_constraints() {
        let markdown = r#"```tangle:///output.txt?id=main&last
fn main() {}
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("output.txt"));
        assert_eq!(blocks[0].id.as_ref().unwrap().as_str(), "main");
        assert_eq!(blocks[0].constraints.len(), 1);
        assert!(matches!(blocks[0].constraints[0], Constraint::Last));
    }

    #[test]
    fn test_parse_block_with_after_constraint() {
        let markdown = r#"```tangle:///output.txt?id=b&after=a
Second block
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id.as_ref().unwrap().as_str(), "b");
        match &blocks[0].constraints[0] {
            Constraint::After(ids) => {
                assert_eq!(ids.len(), 1);
                assert_eq!(ids[0].as_str(), "a");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_parse_block_with_multiple_after() {
        let markdown = r#"```tangle:///output.txt?id=c&after=a,b
Third block
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        match &blocks[0].constraints[0] {
            Constraint::After(ids) => {
                assert_eq!(ids.len(), 2);
                assert_eq!(ids[0].as_str(), "a");
                assert_eq!(ids[1].as_str(), "b");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_block_id_display() {
        let id = BlockId::new("my-block".to_string()).unwrap();
        assert_eq!(format!("{id}"), "my-block");
    }

    #[test]
    fn test_parse_block_with_before_constraint() {
        let markdown = r#"```tangle:///output.txt?id=a&before=b
First block
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id.as_ref().unwrap().as_str(), "a");
        match &blocks[0].constraints[0] {
            Constraint::Before(ids) => {
                assert_eq!(ids.len(), 1);
                assert_eq!(ids[0].as_str(), "b");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_parse_block_with_first_constraint() {
        let markdown = r#"```tangle:///output.txt?id=lead&first
First block
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].id.as_ref().unwrap().as_str(), "lead");
        assert_eq!(blocks[0].constraints.len(), 1);
        assert!(matches!(blocks[0].constraints[0], Constraint::First));
    }

    #[test]
    fn test_parse_block_invalid_scheme() {
        // A code block that looks like a tangle URL but uses a non-tangle scheme
        let markdown = r#"```https://example.com/file.txt
code
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn test_parse_block_host_in_tangle_url() {
        let markdown = r#"```tangle://example.com/path.txt
code
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("hostless"));
    }

    #[test]
    fn test_parse_block_missing_path() {
        let markdown = r#"```tangle:///
code
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing path"));
    }

    #[test]
    fn test_parse_block_invalid_path() {
        let markdown = r#"```tangle:////etc/passwd
code
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

    #[test]
    fn test_parse_block_empty_block_id() {
        let markdown = r#"```tangle:///output.txt?id=
code
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_parse_block_invalid_block_id() {
        let markdown = r#"```tangle:///output.txt?id=UPPERCASE
code
```"#;

        let result = Lit::parse_markdown(markdown);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid"));
    }

    #[test]
    fn test_parse_block_unknown_params_ignored() {
        let markdown = r#"```tangle:///output.txt?unknown=value&also-unknown=123
code
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "code");
        assert!(blocks[0].id.is_none());
        assert!(blocks[0].constraints.is_empty());
    }

    #[test]
    fn test_solve_simple_constraint_ordering() {
        let blocks = vec![
            create_constrained_block(
                "c",
                vec![Constraint::After(vec![
                    BlockId::new("b".to_string()).unwrap(),
                ])],
                "Third",
            ),
            create_constrained_block("a", vec![Constraint::First], "First"),
            create_constrained_block(
                "b",
                vec![Constraint::After(vec![
                    BlockId::new("a".to_string()).unwrap(),
                ])],
                "Second",
            ),
        ];

        let sorted = solve_block_order(&blocks).unwrap();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].id.as_ref().unwrap().as_str(), "a");
        assert_eq!(sorted[1].id.as_ref().unwrap().as_str(), "b");
        assert_eq!(sorted[2].id.as_ref().unwrap().as_str(), "c");
    }

    #[test]
    fn test_solve_circular_dependency() {
        let blocks = vec![
            create_constrained_block(
                "a",
                vec![Constraint::After(vec![
                    BlockId::new("b".to_string()).unwrap(),
                ])],
                "A",
            ),
            create_constrained_block(
                "b",
                vec![Constraint::After(vec![
                    BlockId::new("a".to_string()).unwrap(),
                ])],
                "B",
            ),
        ];

        let result = solve_block_order(&blocks);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Constraints are unsatisfiable")
        );
    }

    #[test]
    fn test_solve_unknown_block_id() {
        let blocks = vec![create_constrained_block(
            "a",
            vec![Constraint::After(vec![
                BlockId::new("unknown".to_string()).unwrap(),
            ])],
            "A",
        )];

        let result = solve_block_order(&blocks);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown block ID"));
    }

    #[test]
    fn test_solve_first_and_last() {
        let blocks = vec![
            create_constrained_block("middle", vec![], "Middle"),
            create_constrained_block("first", vec![Constraint::First], "First"),
            create_constrained_block("last", vec![Constraint::Last], "Last"),
        ];

        let sorted = solve_block_order(&blocks).unwrap();
        assert_eq!(sorted[0].id.as_ref().unwrap().as_str(), "first");
        assert_eq!(sorted[2].id.as_ref().unwrap().as_str(), "last");
    }

    #[test]
    fn test_solve_duplicate_id() {
        let blocks = vec![
            create_constrained_block("dup", vec![], "First"),
            create_constrained_block("dup", vec![], "Second"),
        ];

        let result = solve_block_order(&blocks);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Duplicate"));
    }

    #[test]
    fn test_solve_unknown_inside_block_id() {
        let blocks = vec![Block {
            path: PathBuf::from("test.txt"),
            id: Some(BlockId::new("child".to_string()).unwrap()),
            constraints: vec![],
            inside: Some(BlockId::new("nonexistent".to_string()).unwrap()),
            content: "content".to_string(),
        }];

        let result = solve_block_order(&blocks);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown"));
    }

    #[test]
    fn test_solve_empty_input() {
        let blocks: Vec<Block> = vec![];
        let sorted = solve_block_order(&blocks).unwrap();
        assert!(sorted.is_empty());
    }

    fn create_constrained_block(id: &str, constraints: Vec<Constraint>, content: &str) -> Block {
        Block {
            path: PathBuf::from("test.txt"),
            id: Some(BlockId::new(id.to_string()).unwrap()),
            constraints,
            inside: None,
            content: content.to_string(),
        }
    }

    #[test]
    fn test_surround_constraint() {
        let markdown = r##"
```tangle:///output.txt?id=wrapper
struct Foo;

{{}}
```

```tangle:///output.txt?id=impl1&inside=wrapper
impl Foo {
    fn bar(&self) {}
}
```

```tangle:///output.txt?id=impl2&inside=wrapper
impl Foo {
    fn baz(&self) {}
}
```
"##;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 3);

        let sorted = solve_block_order(&blocks).unwrap();
        assert_eq!(sorted.len(), 1); // Surrounded blocks merged into wrapper

        let content = &sorted[0].content;
        assert!(content.contains("struct Foo;"));
        assert!(content.contains("fn bar(&self) {}"));
        assert!(content.contains("fn baz(&self) {}"));
        assert!(!content.contains("{{}}")); // Placeholder replaced
    }

    #[test]
    fn test_surround_preserves_order() {
        let markdown = r##"
```tangle:///output.txt?id=wrapper&first
fn main() {
    {{}}
}
```

```tangle:///output.txt?id=body1&inside=wrapper
println!("Hello");
```

```tangle:///output.txt?id=body2&inside=wrapper&after=body1
println!("World");
```

```tangle:///output.txt?id=after&after=wrapper&last
// End
```
"##;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        let sorted = solve_block_order(&blocks).unwrap();

        assert_eq!(sorted.len(), 2); // wrapper (with surrounded) and after
        assert_eq!(sorted[0].id.as_ref().unwrap().as_str(), "wrapper");
        assert_eq!(sorted[1].id.as_ref().unwrap().as_str(), "after");

        let wrapper_content = &sorted[0].content;
        assert!(wrapper_content.contains("println!(\"Hello\")"));
        assert!(wrapper_content.contains("println!(\"World\")"));

        // Check order of surrounded blocks
        let hello_pos = wrapper_content.find("Hello").unwrap();
        let world_pos = wrapper_content.find("World").unwrap();
        assert!(hello_pos < world_pos);
    }

    #[test]
    fn test_surround_block_without_children() {
        // A block with an id but no blocks inside=it should pass through unchanged;
        // exercises the else branch in apply_surrounds (id present, no children)
        let blocks = vec![Block {
            path: PathBuf::from("test.txt"),
            id: Some(BlockId::new("only".to_string()).unwrap()),
            constraints: vec![],
            inside: None,
            content: "only block".to_string(),
        }];

        let result = apply_surrounds(blocks).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id.as_ref().unwrap().as_str(), "only");
        assert_eq!(result[0].content, "only block");
    }

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

        // Clean up any leftover temp dirs from previous runs
        let _ = fs::remove_dir_all(&temp_input);
        let _ = fs::remove_dir_all(&temp_output);

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

        // Clean up any leftover temp dirs from previous runs
        let _ = fs::remove_dir_all(&temp_input);
        let _ = fs::remove_dir_all(&temp_output);

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
}

/// Regex pattern for valid block IDs: lowercase letter + letters/digits with single hyphens
static BLOCK_ID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // The pattern is a compile-time literal, so compilation cannot fail.
    #[allow(clippy::unwrap_used)]
    let pattern = Regex::new(r"^[a-z][a-z0-9]*(-[a-z0-9]+)*$").unwrap();
    pattern
});

/// Unique identifier for a block
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockId(String);

impl BlockId {
    pub fn new(s: String) -> std::result::Result<Self, BlockIdError> {
        if s.is_empty() {
            return Err(BlockIdError::Empty);
        }
        if !BLOCK_ID_PATTERN.is_match(&s) {
            return Err(BlockIdError::InvalidCharacters(s));
        }
        Ok(BlockId(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Errors that can occur when creating a BlockId
#[derive(Debug, Error, Diagnostic)]
pub enum BlockIdError {
    #[error("Block ID cannot be empty")]
    #[diagnostic(code(lit::block_id::empty))]
    Empty,
    #[error(
        "Block ID '{0}' is invalid (must start with lowercase letter, contain only lowercase letters/digits/hyphens, no leading/trailing/consecutive dashes)"
    )]
    #[diagnostic(
        code(lit::block_id::invalid_characters),
        help("use a lowercase letter followed by letters, digits, or single hyphens")
    )]
    InvalidCharacters(String),
}

/// Ordering constraint for blocks
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    /// Must be first (position = 0)
    First,
    /// Must be last (position = max)
    Last,
    /// Must come after all specified blocks
    After(Vec<BlockId>),
    /// Must come before all specified blocks
    Before(Vec<BlockId>),
}

/// Represents a single tangle block from markdown
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The file path to write this block to
    pub path: PathBuf,
    /// Optional unique identifier for this block
    pub id: Option<BlockId>,
    /// Ordering constraints for this block
    pub constraints: Vec<Constraint>,
    /// Optional: This block is inside another
    pub inside: Option<BlockId>,
    /// The content of the code block
    pub content: String,
}

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
        // A hostless URL path always begins with '/', so stripping it cannot fail.
        #[allow(clippy::unwrap_used)]
        let path_str = path.strip_prefix('/').unwrap().to_string();

        // Parse constraint parameters
        let query_params: HashMap<_, _> = parsed.query_pairs().collect();
        let (id, constraints, inside) = parse_constraints(&query_params)?;

        Ok(Block {
            path: PathBuf::from(path_str),
            id,
            constraints,
            inside,
            content: code.value.clone(),
        })
    }
}

type ParsedConstraints = (Option<BlockId>, Vec<Constraint>, Option<BlockId>);

fn parse_constraints(
    params: &HashMap<std::borrow::Cow<str>, std::borrow::Cow<str>>,
) -> std::result::Result<ParsedConstraints, BlockError> {
    let mut id = None;
    let mut constraints = Vec::new();
    let mut inside = None;

    for (key, value) in params {
        match key.as_ref() {
            "id" => id = Some(BlockId::new(value.to_string())?),
            "after" => {
                let ids = value
                    .split(',')
                    .map(|s| BlockId::new(s.trim().to_string()))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                constraints.push(Constraint::After(ids));
            }
            "before" => {
                let ids = value
                    .split(',')
                    .map(|s| BlockId::new(s.trim().to_string()))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                constraints.push(Constraint::Before(ids));
            }
            "first" => constraints.push(Constraint::First),
            "last" => constraints.push(Constraint::Last),
            "inside" => {
                inside = Some(BlockId::new(value.to_string())?);
            }
            _ => {} // Ignore unknown parameters
        }
    }

    Ok((id, constraints, inside))
}

/// Errors that can occur when parsing a block from a markdown node
#[derive(Debug, Error, Diagnostic)]
pub enum BlockError {
    #[error("Not a tangle block")]
    #[diagnostic(code(lit::block::not_tangle))]
    NotTangleBlock,
    #[error("Tangle URL must be hostless (use tangle:///path, not tangle://path)")]
    #[diagnostic(code(lit::block::invalid_url))]
    InvalidTangleUrl,
    #[error("Tangle URL missing path")]
    #[diagnostic(code(lit::block::missing_path))]
    MissingPath,
    #[error("Invalid tangle URL path")]
    #[diagnostic(code(lit::block::invalid_path))]
    InvalidPath,
    #[error(transparent)]
    #[diagnostic(transparent)]
    BlockIdError(#[from] BlockIdError),
    #[error("Unknown block ID referenced in constraint: {0}")]
    #[diagnostic(
        code(lit::block::unknown_id),
        help("declare the referenced block with ?id=… or fix the constraint")
    )]
    UnknownBlockId(BlockId),
    #[error("Duplicate block ID within file: {0}")]
    #[diagnostic(
        code(lit::block::duplicate_id),
        help("each block ID must be unique within a destination file")
    )]
    DuplicateId(BlockId),
    #[error("Constraints are unsatisfiable (circular dependency detected)")]
    #[diagnostic(code(lit::block::unsatisfiable))]
    UnsatisfiableConstraints,
    #[error("Constraint solver timeout")]
    #[diagnostic(code(lit::block::solver_timeout))]
    SolverTimeout,
}

/// Top-level library error wrapping everything that can go wrong while tangling.
#[derive(Debug, Error, Diagnostic)]
pub enum LitError {
    #[error(transparent)]
    #[diagnostic(transparent)]
    Block(#[from] BlockError),

    #[error("failed to parse markdown: {0}")]
    #[diagnostic(code(lit::markdown))]
    Markdown(String),

    #[error("markdown did not parse to a root node")]
    #[diagnostic(code(lit::markdown::not_root))]
    NotRoot,

    #[error(transparent)]
    #[diagnostic(code(lit::io))]
    Io(#[from] std::io::Error),
}

/// Result alias used throughout the library.
pub type Result<T> = std::result::Result<T, LitError>;

/// Solve block ordering constraints using a topological sort
pub fn solve_block_order(blocks: &[Block]) -> Result<Vec<Block>> {
    if blocks.is_empty() {
        return Ok(Vec::new());
    }

    // Collect blocks with IDs (for constraint solving)
    let with_ids: Vec<_> = blocks.iter().filter(|b| b.id.is_some()).collect();

    // Collect blocks without IDs (will be placed at default position)
    let without_ids: Vec<_> = blocks.iter().filter(|b| b.id.is_none()).cloned().collect();

    if with_ids.is_empty() {
        // No constraints, return as-is
        return Ok(blocks.to_vec());
    }

    // Check for duplicate IDs
    let mut seen = HashSet::new();
    for block in &with_ids {
        if let Some(id) = &block.id
            && !seen.insert(id.as_str())
        {
            return Err(BlockError::DuplicateId(id.clone()).into());
        }
    }

    // Build ID to index map. `with_ids` is filtered to blocks whose id is Some,
    // so unwrapping the id here cannot fail.
    #[allow(clippy::unwrap_used)]
    let id_to_idx: HashMap<_, _> = with_ids
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id.as_ref().unwrap().clone(), i))
        .collect();

    // Validate inside relationships
    for block in &with_ids {
        if let Some(surrounding_id) = &block.inside
            && !id_to_idx.contains_key(surrounding_id)
        {
            return Err(BlockError::UnknownBlockId(surrounding_id.clone()).into());
        }
    }

    // Build a dependency graph: an edge a -> b means "a must come before b".
    // Nodes are added in input order, so node index == index into `with_ids`.
    let mut graph = DiGraph::<usize, ()>::new();
    let nodes: Vec<NodeIndex> = (0..with_ids.len()).map(|i| graph.add_node(i)).collect();

    // Every index used to address `nodes` is in range by construction.
    #[allow(clippy::indexing_slicing)]
    for (i, block) in with_ids.iter().enumerate() {
        for constraint in &block.constraints {
            match constraint {
                Constraint::First => {
                    // Edge to every other block, so this is the only node with
                    // in-degree zero and the sort emits it first (absolute pos 0).
                    for j in 0..with_ids.len() {
                        if j != i {
                            graph.add_edge(nodes[i], nodes[j], ());
                        }
                    }
                }
                Constraint::Last => {
                    // Edge from every other block, so this node is reached only
                    // after all of them and the sort emits it last.
                    for j in 0..with_ids.len() {
                        if j != i {
                            graph.add_edge(nodes[j], nodes[i], ());
                        }
                    }
                }
                Constraint::After(ids) => {
                    for other_id in ids {
                        let &j = id_to_idx
                            .get(other_id)
                            .ok_or_else(|| BlockError::UnknownBlockId(other_id.clone()))?;
                        graph.add_edge(nodes[j], nodes[i], ());
                    }
                }
                Constraint::Before(ids) => {
                    for other_id in ids {
                        let &j = id_to_idx
                            .get(other_id)
                            .ok_or_else(|| BlockError::UnknownBlockId(other_id.clone()))?;
                        graph.add_edge(nodes[i], nodes[j], ());
                    }
                }
            }
        }
    }

    // Stable topological sort (Kahn's algorithm). Ties are broken by original
    // input index so that unconstrained blocks keep their document order.
    let mut in_degree: Vec<usize> = nodes
        .iter()
        .map(|&n| graph.neighbors_directed(n, Direction::Incoming).count())
        .collect();

    let mut ready: BinaryHeap<Reverse<usize>> = in_degree
        .iter()
        .enumerate()
        .filter(|&(_, &d)| d == 0)
        .map(|(i, _)| Reverse(i))
        .collect();

    // Indices come from the graph's own node set, so addressing `nodes` and
    // `in_degree` cannot go out of bounds; the in-degree never underflows
    // because each edge is only decremented once.
    let mut order = Vec::with_capacity(with_ids.len());
    #[allow(clippy::indexing_slicing)]
    while let Some(Reverse(i)) = ready.pop() {
        order.push(i);
        for neighbor in graph.neighbors_directed(nodes[i], Direction::Outgoing) {
            let j = neighbor.index();
            in_degree[j] = in_degree[j].saturating_sub(1);
            if in_degree[j] == 0 {
                ready.push(Reverse(j));
            }
        }
    }

    // If not every node was emitted, some nodes never reached in-degree zero,
    // which means the graph contains a cycle (contradictory constraints).
    if order.len() != with_ids.len() {
        return Err(BlockError::UnsatisfiableConstraints.into());
    }

    // `order` is a permutation of `0..with_ids.len()`, so every index is valid.
    #[allow(clippy::indexing_slicing)]
    let sorted: Vec<Block> = order.iter().map(|&i| with_ids[i].clone()).collect();

    // Apply surround relationships
    let mut sorted_blocks = apply_surrounds(sorted)?;

    // Add blocks without IDs at the end
    sorted_blocks.extend(without_ids);

    Ok(sorted_blocks)
}

/// Apply surround relationships to blocks
fn apply_surrounds(blocks: Vec<Block>) -> Result<Vec<Block>> {
    // Build map of surrounded blocks
    let mut surrounded: HashMap<BlockId, Vec<Block>> = HashMap::new();
    let mut non_surrounded = Vec::new();

    for block in blocks {
        if let Some(ref parent_id) = block.inside {
            surrounded.entry(parent_id.clone()).or_default().push(block);
        } else {
            non_surrounded.push(block);
        }
    }

    // Process blocks and apply surrounds
    let mut result = Vec::new();
    for block in non_surrounded {
        // Check if this block has children (blocks marked as inside=this_id)
        match block.id.as_ref().and_then(|id| surrounded.get(id)) {
            Some(children) => {
                // This block has children, replace {{}} placeholder
                let children_content = children
                    .iter()
                    .map(|b| b.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");

                // Replace {{}} with children content
                let content = block.content.replace("{{}}", &children_content);

                result.push(Block {
                    path: block.path.clone(),
                    id: block.id.clone(),
                    constraints: block.constraints.clone(),
                    inside: block.inside.clone(),
                    content,
                });
            }
            None => result.push(block),
        }
    }

    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TangledFile {
    pub path: PathBuf,
    pub blocks: Vec<Block>,
}

impl TangledFile {
    pub fn new(path: PathBuf, blocks: Vec<Block>) -> Self {
        // Blocks are assumed to be pre-sorted by solve_block_order
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
