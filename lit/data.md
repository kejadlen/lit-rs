# Data Blocks

Most tangle blocks are text: lit concatenates them in order and writes the
result. Data blocks are different. Their content is *parsed* as structured
data, every data block for a destination is merged into one value, and that
value is serialized in whatever format the destination's extension names.

This lets the prose drive the shape of a config file. A `Cargo.toml` can be
written in YAML, a `package.json` can be assembled from fragments scattered
across the chapters that explain each dependency, and the fragments never
need to agree on punctuation or nesting.

## Writing a data block

A block becomes a data block when its URL carries `as=<format>`, naming the
syntax its *content* is written in. The destination's extension names the
*output* format. The two are independent:

````markdown
```tangle:///config.toml?as=yaml
server:
  host: localhost
  port: 8080
```
````

`at=<dotted.path>` grafts the block's value under a key path, so a fragment
doesn't have to repeat the nesting it lives in:

````markdown
```tangle:///config.toml?as=yaml&at=server.tls
cert: /etc/ssl/cert.pem
```
````

Both blocks together produce:

```toml
[server]
host = "localhost"
port = 8080

[server.tls]
cert = "/etc/ssl/cert.pem"
```

Data blocks still take part in the usual ordering constraints (`id`,
`after`, `before`, `first`, `last`). Order matters in two places: it decides
the order keys appear in the output, and it decides the order arrays are
concatenated in.

## Formats

`DataFormat` names the formats lit can read and write. Every format goes
through `serde_json::Value` as the common in-memory shape, built with
`preserve_order` so keys come out in the order they were written.

```tangle:///src/lib.rs
/// A structured data syntax a block can be written in or rendered to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Json,
    Toml,
    Yaml,
}

impl DataFormat {
    /// The format named by an `as=` parameter
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "json" => Some(DataFormat::Json),
            "toml" => Some(DataFormat::Toml),
            "yaml" | "yml" => Some(DataFormat::Yaml),
            _ => None,
        }
    }

    /// The format a destination path should be rendered in
    pub fn from_path(path: &Utf8Path) -> Option<Self> {
        path.extension().and_then(Self::from_name)
    }

    pub fn parse(self, text: &str) -> std::result::Result<Value, DataError> {
        let parsed = match self {
            DataFormat::Json => serde_json::from_str(text).map_err(|e| e.to_string()),
            DataFormat::Toml => toml::from_str(text).map_err(|e| e.to_string()),
            DataFormat::Yaml => serde_norway::from_str(text).map_err(|e| e.to_string()),
        };
        parsed.map_err(|message| DataError::Parse {
            format: self,
            message,
        })
    }

    pub fn render(self, value: &Value) -> std::result::Result<String, DataError> {
        let rendered = match self {
            DataFormat::Json => serde_json::to_string_pretty(value).map_err(|e| e.to_string()), // cov-excl-line: a Value always serializes to JSON
            DataFormat::Toml => toml::to_string(value).map_err(|e| e.to_string()),
            DataFormat::Yaml => serde_norway::to_string(value).map_err(|e| e.to_string()),
        }
        .map_err(|message| DataError::Render {
            format: self,
            message,
        })?;
        Ok(format!("{}\n", rendered.trim_end()))
    }
}

impl std::fmt::Display for DataFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            DataFormat::Json => "JSON",
            DataFormat::Toml => "TOML",
            DataFormat::Yaml => "YAML",
        };
        write!(f, "{name}")
    }
}
```

TOML has no `null` and needs a table at the root, so rendering a YAML or
JSON value that breaks either rule fails with a render error rather than
silently dropping data. TOML datetimes don't survive the round trip through
`Value` cleanly; that's a known limitation of this prototype.

## Block options

`DataOptions` holds the two data parameters. A block whose `syntax` is
`None` is an ordinary text block.

```tangle:///src/lib.rs
/// Data-block parameters parsed from a tangle URL
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataOptions {
    /// The syntax the block's content is written in (`as=`)
    pub syntax: Option<DataFormat>,
    /// The key path to graft the block's value under (`at=`)
    pub at: Vec<String>,
}

fn parse_data_options(
    params: &HashMap<std::borrow::Cow<str>, std::borrow::Cow<str>>,
) -> std::result::Result<DataOptions, BlockError> {
    let syntax = params
        .get("as")
        .map(|name| {
            DataFormat::from_name(name).ok_or_else(|| BlockError::UnknownDataFormat(name.to_string()))
        })
        .transpose()?;

    let at = match params.get("at") {
        None => Vec::new(),
        Some(path) => {
            let keys: Vec<String> = path.split('.').map(str::to_string).collect();
            if keys.iter().any(String::is_empty) {
                return Err(BlockError::InvalidDataPath(path.to_string()));
            }
            keys
        }
    };

    if syntax.is_none() && !at.is_empty() {
        return Err(BlockError::AtWithoutAs);
    }

    Ok(DataOptions { syntax, at })
}
```

## Merging

`render_data` turns an ordered list of data blocks into the output text.
Each block's value is wrapped in objects for its `at=` path, then merged
into the running result.

The merge is deliberately strict, because a silent override in a literate
document hides which paragraph actually decided a value:

- Two objects merge key by key, recursively.
- Two arrays concatenate, in block order.
- An empty block (`null`) contributes nothing.
- Anything else meeting an existing value is a conflict, reported with the
  key path where the two blocks disagree.

A destination can't mix text and data blocks: there's no sensible way to
splice raw text into a value.

```tangle:///src/lib.rs
/// Merge an ordered list of data blocks and render them for `path`
pub fn render_data(path: &Utf8Path, blocks: &[Block]) -> std::result::Result<String, DataError> {
    let output =
        DataFormat::from_path(path).ok_or_else(|| DataError::NoOutputFormat(path.to_owned()))?;

    let mut root = Value::Null;
    for block in blocks {
        let syntax = block
            .data
            .syntax
            .ok_or_else(|| DataError::MixedBlocks(path.to_owned()))?;
        let value = block
            .data
            .at
            .iter()
            .rev()
            .fold(syntax.parse(&block.content)?, |inner, key| {
                Value::Object([(key.clone(), inner)].into_iter().collect())
            });
        merge_value(&mut root, value, &mut Vec::new()).map_err(|key_path| DataError::Conflict {
            path: path.to_owned(),
            key_path,
        })?;
    }

    output.render(&root)
}

/// Merge `incoming` into `existing`, returning the dotted key path of any conflict
fn merge_value(
    existing: &mut Value,
    incoming: Value,
    key_path: &mut Vec<String>,
) -> std::result::Result<(), String> {
    match (existing, incoming) {
        (_, Value::Null) => Ok(()),
        (existing @ Value::Null, incoming) => {
            *existing = incoming;
            Ok(())
        }
        (Value::Object(existing), Value::Object(incoming)) => {
            for (key, value) in incoming {
                key_path.push(key.clone());
                merge_value(existing.entry(key).or_insert(Value::Null), value, key_path)?;
                key_path.pop();
            }
            Ok(())
        }
        (Value::Array(existing), Value::Array(incoming)) => {
            existing.extend(incoming);
            Ok(())
        }
        _ => Err(if key_path.is_empty() {
            "<root>".to_string()
        } else {
            key_path.join(".")
        }),
    }
}
```

## Errors

```tangle:///src/lib.rs
/// Errors that can occur while merging and rendering data blocks
#[derive(Debug, Error, Diagnostic)]
pub enum DataError {
    #[error("failed to parse {format} data block: {message}")]
    #[diagnostic(code(lit::data::parse))]
    Parse { format: DataFormat, message: String },

    #[error("failed to render {format}: {message}")]
    #[diagnostic(
        code(lit::data::render),
        help("TOML needs a table at the root and has no null")
    )]
    Render { format: DataFormat, message: String },

    #[error("data blocks for {path} disagree at `{key_path}`")]
    #[diagnostic(
        code(lit::data::conflict),
        help("only objects merge and arrays concatenate; any other value can be set by one block")
    )]
    Conflict { path: Utf8PathBuf, key_path: String },

    #[error("{0} mixes text blocks and data blocks")]
    #[diagnostic(
        code(lit::data::mixed),
        help("give every block for this file an `as=` format, or none of them")
    )]
    MixedBlocks(Utf8PathBuf),

    #[error("{0} has data blocks but no data extension")]
    #[diagnostic(
        code(lit::data::no_output_format),
        help("data destinations must end in .json, .toml, .yaml, or .yml")
    )]
    NoOutputFormat(Utf8PathBuf),
}
```

## Tests

```tangle:///src/lib.rs?id=test-data-helper&inside=test-mod
    fn render_md(path: &str, markdown: &str) -> std::result::Result<String, DataError> {
        let blocks = Lit::parse_markdown(markdown).unwrap();
        let sorted = solve_block_order(&blocks).unwrap();
        render_data(Utf8Path::new(path), &sorted)
    }
```

The worked example from the top of this chapter:

````tangle:///src/lib.rs?id=test-data-yaml-to-toml&inside=test-mod
    #[test]
    fn test_data_yaml_to_toml_with_at() {
        let markdown = r#"
```tangle:///config.toml?as=yaml
server:
  host: localhost
  port: 8080
```

```tangle:///config.toml?as=yaml&at=server.tls
cert: /etc/ssl/cert.pem
```
"#;
        assert_eq!(
            render_md("config.toml", markdown).unwrap(),
            "[server]\nhost = \"localhost\"\nport = 8080\n\n[server.tls]\ncert = \"/etc/ssl/cert.pem\"\n"
        );
    }
````

Ordering constraints decide key and array order, and arrays concatenate:

````tangle:///src/lib.rs?id=test-data-order-and-arrays&inside=test-mod
    #[test]
    fn test_data_order_and_array_concat() {
        let markdown = r#"
```tangle:///out.json?as=toml&id=second&after=first
tags = ["b"]
name = "x"
```

```tangle:///out.json?as=json&id=first
{"tags": ["a"]}
```
"#;
        assert_eq!(
            render_md("out.json", markdown).unwrap(),
            "{\n  \"tags\": [\n    \"a\",\n    \"b\"\n  ],\n  \"name\": \"x\"\n}\n"
        );
    }
````

````tangle:///src/lib.rs?id=test-data-yaml-output&inside=test-mod
    #[test]
    fn test_data_yaml_output_and_empty_block() {
        let markdown = r#"
```tangle:///out.yml?as=json&at=a
{"b": 1}
```

```tangle:///out.yml?as=yaml
```
"#;
        assert_eq!(render_md("out.yml", markdown).unwrap(), "a:\n  b: 1\n");
    }
````

````tangle:///src/lib.rs?id=test-data-conflict&inside=test-mod
    #[test]
    fn test_data_conflict_reports_key_path() {
        let markdown = r#"
```tangle:///out.json?as=yaml
a: {b: 1}
```

```tangle:///out.json?as=yaml
a: {b: 2}
```
"#;
        let err = render_md("out.json", markdown).unwrap_err();
        assert_eq!(err.to_string(), "data blocks for out.json disagree at `a.b`");
    }

    #[test]
    fn test_data_conflict_at_root() {
        let markdown = r#"
```tangle:///out.json?as=json
[1]
```

```tangle:///out.json?as=json
{"a": 1}
```
"#;
        let err = render_md("out.json", markdown).unwrap_err();
        assert!(err.to_string().contains("`<root>`"));
    }
````

````tangle:///src/lib.rs?id=test-data-errors&inside=test-mod
    #[test]
    fn test_data_parse_error() {
        let markdown = "```tangle:///out.json?as=json\n{nope\n```\n";
        let err = render_md("out.json", markdown).unwrap_err();
        assert!(err.to_string().starts_with("failed to parse JSON data block"));
    }

    #[test]
    fn test_data_toml_and_yaml_parse_errors() {
        assert!(DataFormat::Toml.parse("= 1").is_err());
        assert!(DataFormat::Yaml.parse("a: [").is_err());
    }

    #[test]
    fn test_data_render_error_toml_null_root() {
        let markdown = "```tangle:///out.toml?as=json\n[1, 2]\n```\n";
        let err = render_md("out.toml", markdown).unwrap_err();
        assert!(err.to_string().starts_with("failed to render TOML"));
    }

    #[test]
    fn test_data_format_display() {
        assert_eq!(DataFormat::Json.to_string(), "JSON");
        assert_eq!(DataFormat::Toml.to_string(), "TOML");
        assert_eq!(DataFormat::Yaml.to_string(), "YAML");
    }

    #[test]
    fn test_tangled_file_render_surfaces_data_error() {
        let blocks = Lit::parse_markdown("```tangle:///a.toml?as=json\n[1]\n```\n").unwrap();
        let file = TangledFile::new(Utf8PathBuf::from("a.toml"), blocks);
        assert!(matches!(file.render(), Err(LitError::Data(DataError::Render { .. }))));
    }

    #[test]
    fn test_data_yaml_renders_scalar_root() {
        assert_eq!(DataFormat::Yaml.render(&Value::Null).unwrap(), "null\n");
    }

    #[test]
    fn test_data_mixed_blocks() {
        let markdown = "```tangle:///out.json?as=json\n{}\n```\n\n```tangle:///out.json\nraw\n```\n";
        let err = render_md("out.json", markdown).unwrap_err();
        assert_eq!(err.to_string(), "out.json mixes text blocks and data blocks");
    }

    #[test]
    fn test_data_no_output_format() {
        let markdown = "```tangle:///out.txt?as=json\n{}\n```\n";
        let err = render_md("out.txt", markdown).unwrap_err();
        assert_eq!(err.to_string(), "out.txt has data blocks but no data extension");
    }
````

URL parameter parsing:

````tangle:///src/lib.rs?id=test-data-options&inside=test-mod
    #[test]
    fn test_data_options_parse() {
        let blocks =
            Lit::parse_markdown("```tangle:///a.json?as=yml&at=x.y\n```\n").unwrap();
        assert_eq!(
            blocks[0].data,
            DataOptions {
                syntax: Some(DataFormat::Yaml),
                at: vec!["x".to_string(), "y".to_string()],
            }
        );
    }

    #[test]
    fn test_data_options_errors() {
        let unknown = Lit::parse_markdown("```tangle:///a.json?as=xml\n```\n").unwrap_err();
        assert_eq!(unknown.to_string(), "Unknown data format: xml");

        let bad_path = Lit::parse_markdown("```tangle:///a.json?as=json&at=x..y\n```\n").unwrap_err();
        assert_eq!(bad_path.to_string(), "Invalid data key path: x..y");

        let no_as = Lit::parse_markdown("```tangle:///a.json?at=x\n```\n").unwrap_err();
        assert_eq!(no_as.to_string(), "`at=` requires `as=`");
    }
````

End to end, a data destination goes through `TangledFile::render` while a
text destination with the same extension is left alone:

````tangle:///src/lib.rs?id=test-data-tangled-file&inside=test-mod
    #[test]
    fn test_tangled_file_renders_data_and_text_json() {
        let data = Lit::parse_markdown("```tangle:///a.json?as=yaml\nk: v\n```\n").unwrap();
        let file = TangledFile::new(Utf8PathBuf::from("a.json"), data);
        assert_eq!(file.render().unwrap(), "{\n  \"k\": \"v\"\n}\n");

        let text = Lit::parse_markdown("```tangle:///b.json\n{ \"raw\":1 }\n```\n").unwrap();
        let file = TangledFile::new(Utf8PathBuf::from("b.json"), text);
        assert_eq!(file.render().unwrap(), "{ \"raw\":1 }\n");
    }
````
