# Dependencies

Key dependencies include:
- `color_eyre` for error handling
- `markdown` for parsing markdown AST
- `regex` for validating block IDs
- `url` for parsing `tangle://` URLs
- `walkdir` for traversing input directories
- `tracing` for logging
- `z3` for constraint solving

```tangle:///src/lib.rs?id=imports&first
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
```
