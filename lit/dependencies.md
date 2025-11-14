# Dependencies

Key dependencies include:
- `color_eyre` for error handling
- `markdown` for parsing markdown AST
- `url` for parsing `tangle://` URLs
- `walkdir` for traversing input directories
- `tracing` for logging

```tangle:///src/lib.rs?at=a
use color_eyre::{Result, eyre::bail};
use markdown::{ParseOptions, mdast::Node, to_mdast};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;
use tracing::info;
use url::Url;
use walkdir::WalkDir;
```
