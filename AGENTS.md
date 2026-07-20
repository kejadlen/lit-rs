# Working in this repository

Lit is a literate-programming tool that tangles code out of Markdown. It
is written in itself: the files under `src/` are **generated**, not
hand-written. This document explains how to make changes without fighting
the tooling.

## The one rule that matters

Never edit `src/` directly. The contents of `src/lib.rs` and `src/main.rs`
are tangled from the Markdown sources in `lit/`. Any hand edit to `src/`
is overwritten the next time the project is tangled.

Edit the literate sources instead, then regenerate:

```sh
just tangle   # runs `cargo run -- lit .` and `cargo fmt`
```

Commit both the `lit/*.md` change and the regenerated `src/` output in the
same change, so the two never drift. CI fails if `src/` does not match a
fresh tangle of `lit/`.

## Where things live

| File | Contents |
|---|---|
| `lit/lit.md` | Core tangler: parsing, reading input, writing output, `TangledFile` |
| `lit/constraints.md` | Constraint solver (topological sort), `Block`, `BlockId`, and all error types |
| `lit/cli.md` | The `lit` binary (`src/main.rs`) |
| `lit/dependencies.md` | The shared `use` block tangled to the top of `src/lib.rs` |

A `tangle:///path?...` fenced block names its destination file and its
ordering constraints (`first`, `last`, `after=`, `before=`, `inside=`).
A topological sort resolves the order, so blocks can appear in any
reading order within the Markdown.

## Commands

```sh
just tangle     # regenerate src/ from lit/
just clippy     # tangle, then clippy with -D warnings
just coverage   # tangle, then test with a 100% line-coverage gate
just mutants    # tangle, then mutation testing
just all        # clippy + coverage
cargo test      # run tests against the current src/
```

## Conventions

These match the project's Rust preferences and are enforced by clippy:

- Errors derive `thiserror::Error` and `miette::Diagnostic`. Library
  functions return `Result<T>` (aliased to `LitError`); `main` returns
  `miette::Result<()>`.
- Use `fs_err` (imported as `fs`), never `std::fs`.
- Panic-discipline lints (`unwrap_used`, `expect_used`, `panic`,
  `indexing_slicing`, `arithmetic_side_effects`) are warnings that CI
  promotes to errors. Opt out only at a specific call site with
  `#[allow(...)]` and a comment explaining why the invariant holds. Test
  modules are exempt via an inner `#![allow(...)]`.

## The bootstrap gotcha

Lit tangles itself, so changing a dependency can break the cycle: if an
edit makes the *current* `src/` stop compiling (for example, removing a
crate it imports), `cargo run -- lit .` can no longer build the tangler
to regenerate `src/`.

When that happens, tangle with the last-good compiled binary instead of
rebuilding:

```sh
./target/debug/lit lit .   # regenerate src/ without recompiling
cargo build                # now the new src/ compiles
```
