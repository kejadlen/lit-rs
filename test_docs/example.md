# Example Literate Programming Document

This is an example of literate programming using the tangle syntax.

## Hello World Program

Let's create a simple hello world program:

```tangle://src/hello.rs
fn main() {
    println!("Hello, World!");
}
```

## Configuration File

We can also tangle configuration files:

```tangle://config.toml
[package]
name = "example"
version = "0.1.0"

[dependencies]
serde = "1.0"
```

## Regular Code Block

This is just a regular code block, not a tangle block:

```rust
// This won't be extracted
let x = 42;
```

## Another Module

```tangle://src/utils.rs
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}
```

That's the end of our example document.
