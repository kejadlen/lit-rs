# Test Nested Blocks

This tests that only top-level tangle blocks are extracted.

## Top-level block

```tangle://top-level.txt
This is a top-level tangle block.
```

## Nested in blockquote

> Here's a code block in a blockquote:
>
> ```tangle://nested-blockquote.txt
> This should NOT be extracted.
> ```

## Nested in list

- Item 1
- Item 2 with code:

  ```tangle://nested-list.txt
  This should NOT be extracted.
  ```

## Another top-level block

```tangle://another-top-level.txt
This is another top-level tangle block.
```
