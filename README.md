# Zahyou (座標)

**Text coordinates: byte offsets ↔ LSP line/UTF-16 positions, converted once and correctly.**

One point in a document has at least three addresses, and every tool in a
toolchain picks a different one:

| Address | Type | Who uses it |
|---|---|---|
| byte offset | `usize` | Rust `&str`, `rnix` spans, most parsers |
| line + UTF-16 code unit | `Position` | **LSP** |
| line + `char` | `CharPosition` | most editors internally |

All three are real; what is wrong is converting between them by accident.
`Position` and `CharPosition` are distinct types for exactly that reason — they
hold the same numbers on every ASCII document and differ by one per
astral-plane character, so assigning one to the other is a compile error rather
than an off-by-one that only appears in other people's files.

## Why this is a crate

The wrong answer is correct for ASCII. Nothing misbehaves until a file contains
an accented identifier — every column on that line shifts by one — or an emoji,
which shifts them by two, because a character outside the Basic Multilingual
Plane is ONE Rust `char`, up to FOUR UTF-8 bytes, and **TWO** UTF-16 code units.

The symptom is never a crash. It is a diagnostic underlining the wrong text, or
a goto-definition landing two columns off, reported as "the editor is a bit
broken".

`chars().count()` is the *obvious* fix for the byte bug. It handles `café` and
`日本語` correctly and is still wrong on emoji — which is exactly why this
belongs in one tested place instead of at each call site.

```rust
use zahyou::Lines;

let src = "# 🎉\nx = 1";
let lines = Lines::new(src);
assert_eq!(lines.position(src, 6).character, 4); // not 3 (chars), not 6 (bytes)
```

## Also handled

- **CRLF** — `\r` belongs to the line it terminates, so no phantom empty lines.
- **Clamping** — offsets cross process boundaries; one party being a byte
  optimistic must not crash the other.
- **Round-trip** — asserted at every char boundary of seven documents,
  including empty, newline-only, and no-trailing-newline.

## Consumers

`escriba-lsp-client` (editor side) and `sui-lsp` (server side). Extracted after
being written twice in one afternoon.

MIT.
