# Dependencies

Key dependencies include:
- `color_eyre` for error handling
- `markdown` for parsing markdown AST
- `regex` for validating position keys
- `url` for parsing `tangle://` URLs
- `walkdir` for traversing input directories
- `tracing` for logging

```tangle:///src/lib.rs?at=a
use color_eyre::{Result, eyre::bail};
use markdown::{ParseOptions, mdast::Node, to_mdast};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;
use thiserror::Error;
use tracing::info;
use url::Url;
use walkdir::WalkDir;

/// Regex pattern for valid position keys: lowercase letters with optional dashes
/// Pattern: one or more lowercase letters, followed by zero or more groups of (dash + lowercase letters)
static POSITION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z]+(?:-[a-z]+)*$").expect("Invalid position regex pattern")
});
```
