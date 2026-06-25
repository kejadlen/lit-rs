# Lit - A Literate Programming Tool

Lit extracts code from Markdown files and tangles them into source files. This
literate programming tool, written in Rust, enables a documentation-first
development approach.

## Overview

Code blocks marked with `tangle:///` URLs tell Lit where to write them. Each
URL specifies a destination file and optional ordering constraints. Lit reads
markdown files, extracts these blocks, groups them by destination, solves
ordering constraints, and writes each group to its target file.

See `lit/constraints.md` for details on the constraint-based ordering system.

## Usage

Lit takes an input directory and an output directory.

```tangle:///src/lib.rs?id=lit-struct
#[derive(Debug)]
pub struct Lit {
    pub input: PathBuf,
    pub output: PathBuf,
}
```

`tangle` is the main entry point. It reads blocks, renders each file, creates
directories, and writes output.

```tangle:///src/lib.rs?id=tangle&inside=impl-lit
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
```

### Parsing Markdown

`parse_markdown` converts markdown text into blocks. It builds an AST using the
`markdown` crate, then extracts top-level code blocks only (ignoring nested
blocks in quotes or lists).

````tangle:///src/lib.rs?id=parse-markdown&inside=impl-lit
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
````

### Reading Input Files

`read_blocks` walks the input directory, parses all `.md` files, and groups blocks by destination.

````tangle:///src/lib.rs?id=read-blocks&inside=impl-lit
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
````

### Lit Setup

```tangle:///src/lib.rs?id=impl-lit&after=lit-struct
impl Lit {
    pub fn new(input: PathBuf, output: PathBuf) -> Self {
        Lit { input, output }
    }

    {{}}
}
```


### Tests

The tests verify parsing, end-to-end tangling, and file-writing behavior.

````tangle:///src/lib.rs?id=test-parse-single&inside=test-mod
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
````

````tangle:///src/lib.rs?id=test-parse-multiple&inside=test-mod
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
````

````tangle:///src/lib.rs?id=test-parse-ignore-regular&inside=test-mod
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
````

````tangle:///src/lib.rs?id=test-parse-ignore-blockquote&inside=test-mod
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
````

````tangle:///src/lib.rs?id=test-parse-ignore-list&inside=test-mod
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
````

````tangle:///src/lib.rs?id=test-parse-empty&inside=test-mod
    #[test]
    fn test_parse_empty_markdown() {
        let markdown = "";
        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 0);
    }
````

````tangle:///src/lib.rs?id=test-parse-no-tangle&inside=test-mod
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
````

````tangle:///src/lib.rs?id=test-parse-subdirectory&inside=test-mod
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
````

````tangle:///src/lib.rs?id=test-parse-empty-block&inside=test-mod
    #[test]
    fn test_parse_empty_tangle_block() {
        let markdown = r#"```tangle:///empty.txt
```"#;

        let blocks = Lit::parse_markdown(markdown).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, PathBuf::from("empty.txt"));
        assert_eq!(blocks[0].content, "");
    }
````

````tangle:///src/lib.rs?id=test-tangle-end-to-end&inside=test-mod
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
````

````tangle:///src/lib.rs?id=test-files-end-newline&inside=test-mod
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
````

## Tangled Files

`TangledFile` groups blocks destined for the same output file. The constructor
receives blocks already sorted by the constraint solver. When rendering, it
concatenates blocks with double newlines.

```tangle:///src/lib.rs
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
```

### Tests


## Blocks

`Block` represents a single code snippet extracted from markdown. The actual
struct definition and ordering logic is defined in `lit/constraints.md`.

### Parsing from Markdown

Block parsing from markdown AST nodes is defined in `lit/constraints.md`.  This includes:
- URL validation
- Constraint parameter parsing
- Error handling

### Tests

Block tests are defined in `lit/constraints.md`.


## Test Setup

```tangle:///src/lib.rs?id=test-mod
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

    {{}}
}
```

