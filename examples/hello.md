# Hello World Example

This is a simple example demonstrating how to use `lit` for literate programming.

## Overview

Literate programming allows you to write documentation and code together, extracting the code into separate files. In `lit`, you use special code blocks with the `tangle://` prefix to specify where code should be extracted.

## Creating a Simple Program

Let's create a simple "Hello, World!" program in Rust. First, we'll set up our main function:

```tangle://src/hello.rs
fn main() {
    println!("Hello, World!");
    greet("Alice");
}
```

We also want to add a helper function to greet people by name:

```tangle://src/hello.rs
fn greet(name: &str) {
    println!("Hello, {}!", name);
}
```

## Configuration

We'll also create a simple configuration file for our project:

```tangle://config.txt
name=hello-world
version=1.0.0
```

## Usage

To extract these code blocks, run:

```bash
lit examples/
```

This will find all markdown files in the `examples/` directory and extract tangle blocks, showing you which files would be generated and how many lines of code each contains.

## Notes

- Code blocks using `tangle://path/to/file` will be extracted
- Regular code blocks (like the bash example above) are ignored
- Multiple tangle blocks for the same file are collected together
- Only top-level code blocks are extracted (nested blocks in lists or quotes are ignored)
