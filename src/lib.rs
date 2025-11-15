use color_eyre::Result;
use color_eyre::eyre::{bail, eyre};
use markdown::{ParseOptions, mdast::Node, to_mdast};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;
use thiserror::Error;
use tracing::info;
use url::Url;
use walkdir::WalkDir;
use z3::ast::{Ast, Int};
use z3::{SatResult, Solver};

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

    /// Parse markdown content and extract code blocks with tangle:// paths
    pub fn parse_markdown(markdown_text: &str) -> Result<Vec<Block>> {
        let ast = to_mdast(markdown_text, &ParseOptions::default())
            .map_err(|e| eyre!("Failed to parse markdown: {}", e))?;

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
            _ => panic!("Expected After constraint"),
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
            _ => panic!("Expected After constraint"),
        }
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
}

/// Regex pattern for valid block IDs: lowercase letter + letters/digits with single hyphens
static BLOCK_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9]*(-[a-z0-9]+)*$").unwrap());

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
#[derive(Debug, Error)]
pub enum BlockIdError {
    #[error("Block ID cannot be empty")]
    Empty,
    #[error(
        "Block ID '{0}' is invalid (must start with lowercase letter, contain only lowercase letters/digits/hyphens, no leading/trailing/consecutive dashes)"
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

impl Ord for Block {
    fn cmp(&self, _other: &Self) -> std::cmp::Ordering {
        panic!("Blocks cannot be directly compared; use solve_block_order instead")
    }
}

impl PartialOrd for Block {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
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
        // Strip the single leading slash to get a relative path
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
    BlockIdError(#[from] BlockIdError),
    #[error("Unknown block ID referenced in constraint: {0}")]
    UnknownBlockId(BlockId),
    #[error("Duplicate block ID within file: {0}")]
    DuplicateId(BlockId),
    #[error("Constraints are unsatisfiable (circular dependency detected)")]
    UnsatisfiableConstraints,
    #[error("Constraint solver timeout")]
    SolverTimeout,
}

/// Solve block ordering constraints using z3
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

    // Build ID to index map
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

    // Create z3 solver
    let solver = Solver::new();

    // Create z3 variables for each block
    let mut positions: HashMap<BlockId, Int> = HashMap::new();

    for block in &with_ids {
        if let Some(id) = &block.id {
            let var = Int::fresh_const(id.as_str());
            positions.insert(id.clone(), var);
        }
    }

    let num_blocks = with_ids.len() as i64;

    // All positions must be in range [0, num_blocks)
    for var in positions.values() {
        solver.assert(var.ge(Int::from_i64(0)));
        solver.assert(var.lt(Int::from_i64(num_blocks)));
    }

    // All positions must be unique (distinct)
    let all_vars: Vec<_> = positions.values().collect();
    solver.assert(Int::distinct(&all_vars));

    // Add constraints from each block
    for block in &with_ids {
        let id = block.id.as_ref().unwrap();
        let this_pos = &positions[id];

        for constraint in &block.constraints {
            match constraint {
                Constraint::First => {
                    solver.assert(this_pos.eq(Int::from_i64(0)));
                }
                Constraint::Last => {
                    solver.assert(this_pos.eq(Int::from_i64(num_blocks - 1)));
                }
                Constraint::After(ids) => {
                    for other_id in ids {
                        let other_pos = positions
                            .get(other_id)
                            .ok_or_else(|| BlockError::UnknownBlockId(other_id.clone()))?;
                        solver.assert(this_pos.gt(other_pos));
                    }
                }
                Constraint::Before(ids) => {
                    for other_id in ids {
                        let other_pos = positions
                            .get(other_id)
                            .ok_or_else(|| BlockError::UnknownBlockId(other_id.clone()))?;
                        solver.assert(this_pos.lt(other_pos));
                    }
                }
            }
        }
    }

    // Solve
    match solver.check() {
        SatResult::Sat => {
            let model = solver.get_model().ok_or(BlockError::SolverTimeout)?;

            // Extract solution and sort
            let mut block_positions: Vec<(Block, i64)> = with_ids
                .iter()
                .map(|block| {
                    let id = block.id.as_ref().unwrap();
                    let pos = &positions[id];
                    let value = model.eval(pos, true).unwrap().as_i64().unwrap();
                    ((*block).clone(), value)
                })
                .collect();

            block_positions.sort_by_key(|(_, pos)| *pos);

            // Apply surround relationships
            let mut sorted_blocks =
                apply_surrounds(block_positions.into_iter().map(|(b, _)| b).collect())?;

            // Add blocks without IDs at the end
            sorted_blocks.extend(without_ids);

            Ok(sorted_blocks)
        }
        SatResult::Unsat => Err(BlockError::UnsatisfiableConstraints.into()),
        SatResult::Unknown => Err(BlockError::SolverTimeout.into()),
    }
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
        if let Some(id) = &block.id {
            if let Some(children) = surrounded.get(id) {
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
            } else {
                result.push(block);
            }
        } else {
            result.push(block);
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
