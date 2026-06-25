# Dependencies

Key dependencies include:
- `miette` and `thiserror` for diagnostic error handling
- `fs-err` for filesystem operations that name the failing path
- `markdown` for parsing markdown AST
- `regex` for validating block IDs
- `url` for parsing `tangle://` URLs
- `walkdir` for traversing input directories
- `tracing` for logging
- `petgraph` for constraint solving via topological sort

```tangle:///src/lib.rs?id=imports&first
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
```
