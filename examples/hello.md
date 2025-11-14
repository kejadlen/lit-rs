# Hello World Example

This is a simple example demonstrating how to use `lit` for literate programming.

## Overview

Literate programming allows you to write documentation and code together, extracting the code into separate files. In `lit`, you use special code blocks with the `tangle://` prefix to specify where code should be extracted.

## Creating a Shell Script

Let's create a simple "Hello, World!" shell script. First, we'll add the shebang and main greeting:

```tangle://hello.sh
#!/bin/bash

echo "Hello, World!"
```

We can also add a function to greet people by name:

```tangle://hello.sh

greet() {
    local name="$1"
    echo "Hello, $name!"
}

greet "Alice"
greet "Bob"
```

## Configuration

We'll also create a simple configuration file:

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

To run the generated script:

```bash
chmod +x hello.sh
./hello.sh
```

## Notes

- Code blocks using `tangle://path/to/file` will be extracted
- Regular code blocks (like the bash example above) are ignored
- Multiple tangle blocks for the same file are collected together
- Only top-level code blocks are extracted (nested blocks in lists or quotes are ignored)
