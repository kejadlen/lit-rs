# Constraint-Based Ordering

This document extends Lit with constraint-based block ordering using the z3
SMT solver. Instead of manually specifying lexicographic positions with `?at=`,
blocks can use semantic IDs with declarative constraints.

## Overview

The constraint system allows blocks to specify:
- **ID**: Semantic identifier (e.g., `?id=imports` instead of `?at=a`)
- **Ordering**: Constraints like `?first`, `?last`, `?after=other`, `?before=other`
- **Nesting**: Blocks that nest inside other blocks with placeholders

Example:
````markdown
# Main function (goes last)
```tangle:///app.rs?id=main&last
fn main() {
    greet();
}
```

# Imports (go first)
```tangle:///app.rs?id=imports&first
use std::io;
```

# Helper function (after imports, before main)
```tangle:///app.rs?id=greet&after=imports&before=main
fn greet() {
    println!("Hello!");
}
```
````

The z3 solver automatically determines a valid ordering that satisfies all
constraints.

## Dependencies

The constraint system requires z3. The necessary imports are already included in
`lit/dependencies.md`.

## Constraint Types

### Block ID

A `BlockId` uniquely identifies a block within a file. IDs must start with a
lowercase letter, and can contain lowercase letters, digits, and single hyphens
(no leading/trailing dashes or consecutive dashes).

```tangle:///src/lib.rs
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
    #[error("Block ID '{0}' is invalid (must start with lowercase letter, contain only lowercase letters/digits/hyphens, no leading/trailing/consecutive dashes)")]
    #[diagnostic(
        code(lit::block_id::invalid_characters),
        help("use a lowercase letter followed by letters, digits, or single hyphens")
    )]
    InvalidCharacters(String),
}
```

### Constraint Enum

Constraints express ordering relationships between blocks:

```tangle:///src/lib.rs
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
```


### Nesting with Inside

The `inside` parameter allows a block to be nested inside another block. The parent
block's content must include the `{{}}` placeholder that will be replaced with
the nested blocks' content.

Example:
````markdown
# Implementation wrapper
```tangle:///app.rs?id=impl-wrapper
impl MyStruct {
    {{}}
}
```

# Methods nested inside
```tangle:///app.rs?id=method-new&inside=impl-wrapper
    pub fn new() -> Self {
        MyStruct { }
    }
```

```tangle:///app.rs?id=method-do-work&inside=impl-wrapper
    pub fn do_work(&self) {
        println!("Working!");
    }
```
````

This produces:
```rust
impl MyStruct {
    pub fn new() -> Self {
        MyStruct { }
    }

    pub fn do_work(&self) {
        println!("Working!");
    }
}
```

## Block Type

The `Block` struct represents a single tangle block with constraint-based ordering:

```tangle:///src/lib.rs
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
```

## Parsing Constraints

Parse constraints from the markdown AST node:

```tangle:///src/lib.rs
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
```

### Error Types

Error types for constraint-based ordering:

```tangle:///src/lib.rs
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
```

## Constraint Solver

The solver uses z3 to find a valid ordering that satisfies all constraints:

```tangle:///src/lib.rs
/// Solve block ordering constraints using z3
pub fn solve_block_order(blocks: &[Block]) -> Result<Vec<Block>> {
    if blocks.is_empty() {
        return Ok(Vec::new());
    }

    // Collect blocks with IDs (for constraint solving)
    let with_ids: Vec<_> = blocks
        .iter()
        .filter(|b| b.id.is_some())
        .collect();

    // Collect blocks without IDs (will be placed at default position)
    let without_ids: Vec<_> = blocks
        .iter()
        .filter(|b| b.id.is_none())
        .cloned()
        .collect();

    if with_ids.is_empty() {
        // No constraints, return as-is
        return Ok(blocks.to_vec());
    }

    // Check for duplicate IDs
    let mut seen = HashSet::new();
    for block in &with_ids {
        if let Some(id) = &block.id && !seen.insert(id.as_str()) {
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
        if let Some(surrounding_id) = &block.inside && !id_to_idx.contains_key(surrounding_id) {
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

    // Add constraints from each block. `with_ids` only holds blocks with an id,
    // and every such id was inserted into `positions` above, so the unwrap and
    // index below are infallible.
    #[allow(clippy::unwrap_used, clippy::indexing_slicing)]
    for block in &with_ids {
        let id = block.id.as_ref().unwrap();
        let this_pos = &positions[id];

        for constraint in &block.constraints {
            match constraint {
                Constraint::First => {
                    solver.assert(this_pos.eq(Int::from_i64(0)));
                }
                Constraint::Last => {
                    solver.assert(this_pos.eq(Int::from_i64(num_blocks.saturating_sub(1))));
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

            // Extract solution and sort. Every id is present (filtered above and
            // inserted into `positions`), and the solver guarantees a concrete
            // value for each asserted variable.
            #[allow(clippy::unwrap_used, clippy::indexing_slicing)]
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
            let mut sorted_blocks = apply_surrounds(block_positions.into_iter().map(|(b, _)| b).collect())?;

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
```

## Tests

### Constraint Parsing Tests

```tangle:///src/lib.rs?id=test-parse-id-constraints&inside=test-mod
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

```

```tangle:///src/lib.rs?id=test-parse-after&inside=test-mod
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

```

```tangle:///src/lib.rs?id=test-parse-multiple-after&inside=test-mod
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
```

### Constraint Solver Tests

```tangle:///src/lib.rs?id=test-solve-simple&inside=test-mod
    #[test]
    fn test_solve_simple_constraint_ordering() {
        let blocks = vec![
            create_constrained_block("c", vec![Constraint::After(vec![BlockId::new("b".to_string()).unwrap()])], "Third"),
            create_constrained_block("a", vec![Constraint::First], "First"),
            create_constrained_block("b", vec![Constraint::After(vec![BlockId::new("a".to_string()).unwrap()])], "Second"),
        ];

        let sorted = solve_block_order(&blocks).unwrap();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].id.as_ref().unwrap().as_str(), "a");
        assert_eq!(sorted[1].id.as_ref().unwrap().as_str(), "b");
        assert_eq!(sorted[2].id.as_ref().unwrap().as_str(), "c");
    }

```

```tangle:///src/lib.rs?id=test-solve-circular&inside=test-mod
    #[test]
    fn test_solve_circular_dependency() {
        let blocks = vec![
            create_constrained_block("a", vec![Constraint::After(vec![BlockId::new("b".to_string()).unwrap()])], "A"),
            create_constrained_block("b", vec![Constraint::After(vec![BlockId::new("a".to_string()).unwrap()])], "B"),
        ];

        let result = solve_block_order(&blocks);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Constraints are unsatisfiable"));
    }

```

```tangle:///src/lib.rs?id=test-solve-unknown&inside=test-mod
    #[test]
    fn test_solve_unknown_block_id() {
        let blocks = vec![
            create_constrained_block("a", vec![Constraint::After(vec![BlockId::new("unknown".to_string()).unwrap()])], "A"),
        ];

        let result = solve_block_order(&blocks);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown block ID"));
    }

```

```tangle:///src/lib.rs?id=test-solve-first-last&inside=test-mod
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
```

```tangle:///src/lib.rs?id=test-helper&inside=test-mod
    fn create_constrained_block(id: &str, constraints: Vec<Constraint>, content: &str) -> Block {
        Block {
            path: PathBuf::from("test.txt"),
            id: Some(BlockId::new(id.to_string()).unwrap()),
            constraints,
            inside: None,
            content: content.to_string(),
        }
    }
```

### Nesting Tests

````tangle:///src/lib.rs?id=test-surround&inside=test-mod
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

````

````tangle:///src/lib.rs?id=test-surround-order&inside=test-mod
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
````

## Benefits

Constraint-based ordering provides several advantages over manual positions:

1. **Semantic IDs**: `?id=imports` is clearer than `?at=a`
2. **Declarative**: Express relationships (`after=types`) not positions
3. **Automatic**: z3 solver computes valid ordering
4. **Safe**: Circular dependencies detected automatically
5. **Flexible**: Easy to insert blocks without renumbering
6. **Powerful**: Inside parameter enables nested structures
