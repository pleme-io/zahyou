# Zahyou (座標) — text coordinates

> **★★★ CSE / Knowable Construction.** This repo operates under **Constructive
> Substrate Engineering** — canonical spec at
> [`pleme-io/theory/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md`](https://github.com/pleme-io/theory/blob/main/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md).
> The Compounding Directive is in the org-level `pleme-io/CLAUDE.md`.

## Build & test

```bash
cargo test    # unit tests + the doc-test on the crate-level example
```

## What this owns

**One conversion, in one place: byte offset ↔ LSP `Position` (line + UTF-16 code
unit).** Nothing else. There is no parser here, no document store, no protocol —
those belong to the consumers.

| Path | Purpose |
|---|---|
| `src/lib.rs` | `Position`, `CharPosition`, `Range`, `Lines` — the whole crate (14 tests + 1 doc-test) |

**Three addresses, and `Position` ≠ `CharPosition` on purpose.** Byte offsets
(parsers), UTF-16 columns (LSP), and `char` columns (editors, including
`escriba-core::Position`) are all real. The third arrived the moment the escriba
bridge was written: LSP hands over UTF-16 and escriba's buffer wants chars, and
those differ by one per astral-plane character. Keeping them as separate types
makes the mix-up a compile error instead of an off-by-one on emoji-bearing
files. Convert deliberately via `to_char` / `to_utf16`.

`serde` is an **optional** feature, off by default: a language server already
depends on serde and wants the derives; a caller that only needs the arithmetic
should not pay for them.

## Why the crate exists — do not "simplify" this away

The conversion is ~30 lines and looks like a helper. It was written twice in one
afternoon (once in `escriba-lsp-client`, once in `sui-lsp`) before being
extracted, which is the ★★ PRIME DIRECTIVE's second-copy rule firing exactly as
intended.

**The wrong answer is correct for ASCII**, which is what makes this worth a
tested primitive rather than a per-call-site line:

- counting **bytes** shifts every column after the first accented character;
- counting **`char`s** — the obvious fix, and the one that passes every `café`
  and `日本語` test — is still wrong on any character outside the Basic
  Multilingual Plane, which is ONE `char`, FOUR UTF-8 bytes, and **TWO** UTF-16
  code units.

Measured, not asserted: swapping `len_utf16()` for a per-char `1` leaves 8 of the
10 tests green and reddens exactly two — the emoji case and the round-trip. A
per-call-site implementation gets that right at most once.

The symptom is never a crash; it is a diagnostic underlining the wrong text on
somebody else's file.

## Invariants the tests pin

| Illegal state | Mechanism | Tier |
|---|---|---|
| an LSP column assigned to an editor column | `Position` and `CharPosition` are distinct types with no `From` between them | truly-unrep (E0308) |
| a column counted in bytes or `char`s *within* one unit | `len_utf16()` is the only counter; the emoji test reddens on either substitute | only-mitigated (C1) — ceiling: a test, not a type; nothing stops a caller doing its own arithmetic |
| a phantom line per CRLF | split on `\n` only; `\r` belongs to the line it terminates | only-mitigated (C1) — ceiling: same |
| an out-of-range offset panicking | `min(len)` + `get()` on every slice; `#![forbid(unsafe_code)]` | truly-unrep (no panicking path exists) |
| the two directions disagreeing | round-trip asserted at **every** char boundary of 7 documents | only-mitigated (C1) — ceiling: seven fixtures, not a proptest |

The round-trip fixture list is the load-bearing one — extend it rather than
adding a new one-off test when a new width or line-ending case appears.

## Consumers

`escriba-lsp-client` (editor side, needs both directions) and `sui-lsp` (server
side, needs offset → position). Depend on it by version — `zahyou = "0.1"` — via
★★ AUTO-RELEASE, never a git pin: one git pin duplicates a crate in every
consumer's graph.
