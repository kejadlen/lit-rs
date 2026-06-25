# lit

A literate-programming tool that tangles source code out of Markdown.

Lit lets you write a program as prose. You describe the code in `.md`
files, marking each code block with a `tangle://` URL that says where it
belongs and how it should be ordered. Lit reads your Markdown, extracts
those blocks, resolves their ordering, and writes the assembled source
files.

Lit is written in itself: everything under `src/` is tangled from the
Markdown sources in `lit/`.

## Installation

```sh
cargo install --locked --path .
```

This builds the `lit` binary from the current `src/`.

## Usage

```sh
lit <INPUT> [OUTPUT]
```

Lit walks `INPUT` for `.md` files, tangles every `tangle://` code block it
finds, and writes the results under `OUTPUT` (defaulting to `INPUT/out`).
For example, lit tangles its own sources with:

```sh
lit lit .
```

which reads `lit/*.md` and writes `src/lib.rs` and `src/main.rs`.

Logging is controlled with `RUST_LOG` (e.g. `RUST_LOG=debug lit lit .`).

## Tangle blocks

A code block becomes part of a tangled file when its info string is a
`tangle://` URL. The URL's path is the destination file, relative to the
output directory:

````markdown
```tangle:///src/main.rs
fn main() {
    println!("Hello");
}
```
````

Only top-level code blocks are tangled. Blocks nested inside blockquotes
or lists are ignored, so you can show example code without it leaking into
the output.

### Ordering

Blocks for the same destination can appear in any reading order across
your Markdown. Query parameters declare how they should be assembled, and
lit resolves the order with a topological sort:

| Parameter | Meaning |
|---|---|
| `id=<name>` | Give the block a name other blocks can refer to |
| `first` | Place the block at the very start of the file |
| `last` | Place the block at the very end of the file |
| `after=<id>[,<id>…]` | Place after the named block(s) |
| `before=<id>[,<id>…]` | Place before the named block(s) |
| `inside=<id>` | Nest the block inside the named block's `{{}}` placeholder |

`````markdown
# Imports (go first)
```tangle:///app.rs?id=imports&first
…
```

# Helper (after imports, before main)
```tangle:///app.rs?id=greet&after=imports&before=main
…
```

# Main function (goes last)
```tangle:///app.rs?id=main&last
…
```
`````

### Nesting

A block can wrap other blocks. The parent declares a `{{}}` placeholder;
children targeting it with `inside=` are concatenated into that spot:

`````markdown
```tangle:///app.rs?id=impl-wrapper
impl Wrapper {
    {{}}
}
```

```tangle:///app.rs?id=method-new&inside=impl-wrapper
    pub fn new() -> Self { … }
```
`````

If two blocks declare conflicting constraints (a cycle, or a reference to
an `id` that does not exist), lit reports a diagnostic instead of
producing output.

## Project layout

| Path | Contents |
|---|---|
| `lit/lit.md` | Core tangler: parsing, reading input, writing output |
| `lit/constraints.md` | Constraint solving, `Block`, and error types |
| `lit/cli.md` | The `lit` binary |
| `lit/dependencies.md` | Shared imports tangled to the top of `src/lib.rs` |
| `src/` | **Generated** — never edit by hand |

## Development

`src/` is generated from `lit/`. Edit the Markdown, then regenerate:

```sh
just tangle     # regenerate src/ from lit/, then cargo fmt
just clippy     # tangle, then clippy with -D warnings
just coverage   # tangle, then test with a 100% line-coverage gate
just all        # clippy + coverage
cargo test      # run tests against the current src/
```

Commit the `lit/*.md` change and the regenerated `src/` together so the
two never drift; CI fails if `src/` does not match a fresh tangle.

See [AGENTS.md](AGENTS.md) for the full working guide, including the
bootstrap gotcha when an edit changes the tangler's own dependencies.
</content>
</invoke>
