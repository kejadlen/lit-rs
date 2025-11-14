# Hello World Example

This is a simple example demonstrating how to use `lit` for literate programming.

## Overview

Literate programming allows you to write documentation and code together, extracting the code into separate files. In `lit`, you use special code blocks with the `tangle:///` prefix to specify where code should be extracted.

## Creating a Shell Script

Let's create a simple "Hello, World!" shell script. First, we'll add the shebang and main greeting:

```tangle:///hello.sh
#!/bin/bash

echo "Hello, World!"
```

We can also add a function to greet people by name:

```tangle:///hello.sh
greet() {
    local name="$1"
    echo "Hello, $name!"
}

greet "Alice"
greet "Bob"
```

## Configuration

We'll also create a simple configuration file:

```tangle:///config.txt
name=hello-world
version=1.0.0
```

## Usage

To extract these code blocks, run:

```bash
lit examples/ output/
```

This will find all markdown files in the `examples/` directory and extract tangle blocks to the `output/` directory.

To run the generated script:

```bash
chmod +x output/hello.sh
output/hello.sh
```

## Notes

- Code blocks using `tangle:///path/to/file` will be extracted
- Regular code blocks (like the bash example above) are ignored
- Multiple tangle blocks for the same file are collected together
- Only top-level code blocks are extracted (nested blocks in lists or quotes are ignored)

## Block Positioning

You can optionally specify the position of blocks using the `at` parameter. This is useful when you want to control the order of code blocks across different sections of documentation.

Blocks are sorted lexicographically by their position key. Blocks without a position are implicitly placed at position "m", allowing you to place blocks before or after them.

### Example: Building a Program Structure

Here's how to build a program with headers, main content, and footer in the correct order:

```tangle:///program.txt?at=a
# Header Section
# This appears first
```

```tangle:///program.txt?at=z
# Footer Section
# This appears last
```

```tangle:///program.txt
# Main Content
# This appears in the middle (implicitly at position "m")
```

The resulting `program.txt` will be ordered as:
1. Header (at=a, which comes before "m")
2. Main Content (no position, defaults to "m")
3. Footer (at=z, which comes after "m")

### Position Key Rules

- Position keys must contain only lowercase letters (a-z)
- Position keys cannot start with 'm' (reserved for unpositioned blocks)
- Position keys must be unique within the same file
- Sorting is lexicographic: "a" < "b" < "c" etc.
