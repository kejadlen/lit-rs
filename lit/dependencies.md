# Dependencies

Key dependencies include:
- `miette` and `thiserror` for diagnostic error handling
- `fs-err` for filesystem operations that name the failing path
- `markdown` for parsing markdown AST
- `regex` for validating block IDs
- `url` for parsing `tangle://` URLs
- `walkdir` for traversing input directories
- `tracing` for logging
- `z3` for constraint solving

```tangle:///src/lib.rs?id=imports&first
use fs_err as fs;
use markdown::ParseOptions;
use markdown::mdast::Node;
use markdown::to_mdast;
use miette::Diagnostic;
use regex::Regex;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;
use thiserror::Error;
use tracing::info;
use url::Url;
use walkdir::WalkDir;
use z3::SatResult;
use z3::Solver;
use z3::ast::Ast as _;
use z3::ast::Int;
```
