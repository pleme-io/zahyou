//! **Zahyou (座標) — text coordinates.**
//!
//! One point in a document has at least three addresses, and every tool in a
//! toolchain picks a different one:
//!
//! | Address | Type here | Who uses it |
//! |---|---|---|
//! | byte offset | `usize` | Rust `&str`, `rnix` spans, `sui-ir::Span`, most parsers |
//! | line + UTF-16 code unit | [`Position`] | **LSP** — the protocol was designed around a JavaScript editor |
//! | line + `char` | [`CharPosition`] | most editors internally, `escriba-core::Position` among them |
//!
//! All three are real and none is more correct than the others; what is wrong
//! is *converting between them by accident*. [`Position`] and [`CharPosition`]
//! are deliberately distinct types for that reason — they hold the same numbers
//! on every ASCII document and differ by one per astral-plane character, which
//! is the signature of a bug that ships.
//!
//! Converting between them is trivial to describe and unusually easy to get
//! subtly wrong, because **the wrong answer is correct for ASCII**. Nothing
//! misbehaves until a file contains an accented identifier, at which point
//! every column on that line shifts by one — or an emoji, at which point they
//! shift by two, since a character outside the Basic Multilingual Plane is ONE
//! Rust `char`, up to FOUR UTF-8 bytes, and **TWO** UTF-16 code units.
//!
//! The symptom is not a crash. It is a diagnostic underlining the wrong text,
//! or a goto-definition landing a couple of columns off, on other people's
//! files, reported as "the editor is a bit broken".
//!
//! # Why this is a crate and not a helper
//!
//! It was written twice in one afternoon — once in an editor's LSP client, once
//! in a language server — before it was extracted. Two copies of one idea is a
//! defect even when each copy is correct, because they drift: the first one to
//! learn about CRLF or clamping teaches only its own callers.
//!
//! The `chars()` trap is the reason a shared, tested primitive earns its keep
//! here specifically. It is the *obvious* fix for the byte bug, it passes every
//! test involving `café` or `日本語`, and it is still wrong. A per-call-site
//! implementation gets that right at most once.
//!
//! ```
//! use zahyou::Lines;
//! let src = "# 🎉\nx = 1";
//! let lines = Lines::new(src);
//! // "# " is two columns; the emoji adds TWO more, not one.
//! assert_eq!(lines.position(src, 6).character, 4);
//! ```

#![forbid(unsafe_code)]

/// A position: zero-based line, and an offset within that line measured in
/// **UTF-16 code units** (LSP's definition, not bytes and not `char`s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    #[must_use]
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// A position whose column counts **Unicode scalar values** (Rust `char`s) —
/// the third of the three addresses, and the one most editors use internally.
///
/// This is a **separate type from [`Position`] on purpose.** The two agree on
/// every document until one contains a character outside the Basic Multilingual
/// Plane, at which point they differ by one per such character — and a bug of
/// that shape is invisible in testing and permanent in production. Making them
/// distinct types means passing an LSP column where an editor column is wanted
/// is a compile error rather than an off-by-one on other people's files.
///
/// Convert deliberately, via [`Lines::to_char`] / [`Lines::to_utf16`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CharPosition {
    pub line: u32,
    pub character: u32,
}

impl CharPosition {
    #[must_use]
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// A half-open range: `end` is exclusive, as LSP requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// Precomputed line starts for one document.
///
/// Build once per document version; each lookup is then a binary search plus a
/// scan of one line, rather than a scan from the top of the file.
#[derive(Debug, Clone)]
pub struct Lines {
    starts: Vec<usize>,
    len: usize,
}

impl Lines {
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0usize];
        // Split on '\n' ONLY. In a CRLF document the '\r' belongs to the line
        // it terminates; treating "\r\n" as two breaks inserts a phantom empty
        // line and shifts every subsequent line number by one.
        starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        Self { starts, len: text.len() }
    }

    /// Byte offset → position.
    ///
    /// An offset past the end **clamps** rather than panicking. Offsets cross
    /// process boundaries — a server sends them to a client and vice versa —
    /// and one party being a byte optimistic must not take the other down.
    #[must_use]
    pub fn position(&self, text: &str, offset: usize) -> Position {
        let offset = offset.min(self.len);
        let idx = self.starts.partition_point(|&s| s <= offset).saturating_sub(1);
        let start = self.starts[idx];
        let character = text
            .get(start..offset)
            .map_or(0, |s| s.chars().map(|c| u32::try_from(c.len_utf16()).unwrap_or(1)).sum());
        Position { line: u32::try_from(idx).unwrap_or(u32::MAX), character }
    }

    /// Position → byte offset. The inverse of [`Self::position`].
    ///
    /// A character offset past the end of its line clamps to that line's end
    /// rather than spilling onto the next: a peer asking for column 999 means
    /// "end of line", never "somewhere below".
    #[must_use]
    pub fn offset(&self, text: &str, pos: Position) -> usize {
        let Some(&start) = self.starts.get(pos.line as usize) else {
            return self.len;
        };
        let line_end =
            self.starts.get(pos.line as usize + 1).map_or(self.len, |&n| n.saturating_sub(1));
        let mut utf16 = 0u32;
        let mut byte = start;
        for c in text.get(start..line_end).unwrap_or_default().chars() {
            if utf16 >= pos.character {
                break;
            }
            utf16 += u32::try_from(c.len_utf16()).unwrap_or(1);
            byte += c.len_utf8();
        }
        byte
    }

    /// Byte offset → char-counted position.
    ///
    /// Same clamping rules as [`Self::position`]; only the column unit differs.
    #[must_use]
    pub fn char_position(&self, text: &str, offset: usize) -> CharPosition {
        let offset = offset.min(self.len);
        let idx = self.starts.partition_point(|&s| s <= offset).saturating_sub(1);
        let start = self.starts[idx];
        let character = text
            .get(start..offset)
            .map_or(0, |s| u32::try_from(s.chars().count()).unwrap_or(u32::MAX));
        CharPosition { line: u32::try_from(idx).unwrap_or(u32::MAX), character }
    }

    /// Char-counted position → byte offset. The inverse of
    /// [`Self::char_position`].
    #[must_use]
    pub fn offset_of_char(&self, text: &str, pos: CharPosition) -> usize {
        let Some(&start) = self.starts.get(pos.line as usize) else {
            return self.len;
        };
        let line_end =
            self.starts.get(pos.line as usize + 1).map_or(self.len, |&n| n.saturating_sub(1));
        text.get(start..line_end)
            .unwrap_or_default()
            .char_indices()
            .nth(pos.character as usize)
            .map_or(line_end, |(i, _)| start + i)
    }

    /// LSP column → editor column.
    ///
    /// The conversion that has to happen at the boundary between a language
    /// server and an editor that counts `char`s. Composed through the byte
    /// offset rather than counted directly, so there is exactly one place in
    /// this crate that decides what a UTF-16 column means.
    #[must_use]
    pub fn to_char(&self, text: &str, pos: Position) -> CharPosition {
        self.char_position(text, self.offset(text, pos))
    }

    /// Editor column → LSP column. The inverse of [`Self::to_char`].
    #[must_use]
    pub fn to_utf16(&self, text: &str, pos: CharPosition) -> Position {
        self.position(text, self.offset_of_char(text, pos))
    }

    /// A byte span as a range.
    #[must_use]
    pub fn range(&self, text: &str, start: usize, end: usize) -> Range {
        Range { start: self.position(text, start), end: self.position(text, end) }
    }

    /// Number of lines. A document with no trailing newline still counts its
    /// last line; an empty document has one.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    #[must_use]
    pub fn len_bytes(&self) -> usize {
        self.len
    }
}

#[cfg(test)]
mod tests {
    use super::{Lines, Position};

    fn at(text: &str, offset: usize) -> Position {
        Lines::new(text).position(text, offset)
    }

    #[test]
    fn ascii_is_the_obvious_mapping() {
        let t = "let x = 1;\nin x\n";
        assert_eq!(at(t, 0), Position::new(0, 0));
        assert_eq!(at(t, 4), Position::new(0, 4));
        assert_eq!(at(t, 11), Position::new(1, 0), "just past a newline starts the next line");
        assert_eq!(at(t, 13), Position::new(1, 2));
    }

    /// Two UTF-8 bytes, one UTF-16 unit. Counting bytes puts every later column
    /// on the line one too far right.
    #[test]
    fn a_two_byte_character_is_one_column() {
        let t = "café = 1";
        assert_eq!("café".len(), 5, "fixture must be multibyte");
        assert_eq!(at(t, 5), Position::new(0, 4));
    }

    /// Three bytes, still one column.
    #[test]
    fn three_byte_characters_are_one_column_each() {
        let t = "# 日本語\n";
        assert_eq!("日本語".len(), 9);
        assert_eq!(at(t, 2 + 9), Position::new(0, 5));
    }

    /// **The trap this crate exists for.** `chars().count()` is the obvious fix
    /// for the byte bug. It handles café and 日本語 correctly and is STILL
    /// wrong here — one char, four bytes, two UTF-16 units — which is why a
    /// per-call-site implementation gets this right at most once.
    #[test]
    fn an_astral_plane_character_is_two_columns() {
        let t = "# 🎉 done";
        assert_eq!("🎉".len(), 4, "four UTF-8 bytes");
        assert_eq!("🎉".chars().count(), 1, "one char");
        assert_eq!("🎉".chars().next().unwrap().len_utf16(), 2, "TWO UTF-16 units");
        assert_eq!(at(t, 6), Position::new(0, 4), "not 3 (chars) and not 6 (bytes)");
    }

    /// A '\r' belongs to the line it terminates. Otherwise every line number
    /// after the first CRLF is wrong.
    #[test]
    fn crlf_does_not_create_a_phantom_line() {
        let t = "a\r\nb\r\nc";
        let l = Lines::new(t);
        assert_eq!(l.line_count(), 3, "three lines, not five");
        assert_eq!(l.position(t, 3), Position::new(1, 0));
        assert_eq!(l.position(t, 6), Position::new(2, 0));
    }

    /// Offsets cross process boundaries; one party being optimistic must not
    /// crash the other.
    #[test]
    fn out_of_range_clamps_instead_of_panicking() {
        assert_eq!(at("abc", 3), Position::new(0, 3), "exactly at the end is valid");
        assert_eq!(at("abc", 99), Position::new(0, 3));
        assert_eq!(at("", 5), Position::new(0, 0));
    }

    #[test]
    fn an_overlong_character_clamps_to_the_line_end() {
        let t = "ab\ncd\n";
        let l = Lines::new(t);
        assert_eq!(l.offset(t, Position::new(0, 999)), 2, "before the newline, not after");
        assert_eq!(l.offset(t, Position::new(99, 0)), t.len());
    }

    /// Both directions must agree at EVERY char boundary of every awkward
    /// document, not at a sampled few.
    #[test]
    fn the_two_directions_round_trip() {
        for t in [
            "let x = 1;\nin x\n",
            "café = 1\nnaïve = 2\n",
            "# 🎉 done\nx = 1\n",
            "a\r\nb\r\nc",
            "",
            "\n\n\n",
            "no trailing newline",
        ] {
            let l = Lines::new(t);
            for (b, _) in t.char_indices().chain(std::iter::once((t.len(), ' '))) {
                assert_eq!(l.offset(t, l.position(t, b)), b, "round trip failed at {b} of {t:?}");
            }
        }
    }

    /// **The case that makes `CharPosition` a separate type.** On a line with
    /// an astral-plane character, the LSP column and the editor column are
    /// different numbers for the same place. An editor that assigns one to the
    /// other is off by one for the rest of the line — silently, and only for
    /// the users whose files contain emoji.
    #[test]
    fn utf16_and_char_columns_disagree_on_an_astral_character() {
        let t = "# 🎉 x";
        let l = super::Lines::new(t);
        let after_emoji = 2 + 4; // byte offset just past the emoji
        assert_eq!(l.position(t, after_emoji).character, 4, "UTF-16: emoji is 2");
        assert_eq!(l.char_position(t, after_emoji).character, 3, "chars: emoji is 1");
    }

    /// They must agree everywhere else, or the conversion would be noise.
    #[test]
    fn the_two_column_units_agree_on_the_basic_multilingual_plane() {
        for t in ["let x = 1;\nin x", "café = 1", "# 日本語\nx"] {
            let l = super::Lines::new(t);
            for (b, _) in t.char_indices() {
                assert_eq!(
                    l.position(t, b).character,
                    l.char_position(t, b).character,
                    "disagreed at {b} of {t:?}"
                );
            }
        }
    }

    #[test]
    fn the_boundary_conversions_round_trip() {
        for t in ["# 🎉 x\ny = 2", "café\nnaïve", "a\r\nb", "", "plain"] {
            let l = super::Lines::new(t);
            for (b, _) in t.char_indices().chain(std::iter::once((t.len(), ' '))) {
                let utf16 = l.position(t, b);
                let ch = l.to_char(t, utf16);
                assert_eq!(l.to_utf16(t, ch), utf16, "round trip failed at {b} of {t:?}");
                assert_eq!(l.offset_of_char(t, ch), b, "char offset wrong at {b} of {t:?}");
            }
        }
    }

    #[test]
    fn char_positions_clamp_like_utf16_ones() {
        let t = "ab\ncd\n";
        let l = super::Lines::new(t);
        assert_eq!(l.offset_of_char(t, super::CharPosition::new(0, 999)), 2);
        assert_eq!(l.offset_of_char(t, super::CharPosition::new(99, 0)), t.len());
        assert_eq!(l.char_position(t, 999), l.char_position(t, t.len()));
    }

    #[test]
    fn a_span_becomes_a_range() {
        let t = "let x = 1;\n";
        let l = Lines::new(t);
        let r = l.range(t, 4, 5);
        assert_eq!(r.start, Position::new(0, 4));
        assert_eq!(r.end, Position::new(0, 5));
    }

    #[test]
    fn an_empty_document_has_one_line() {
        assert_eq!(Lines::new("").line_count(), 1);
    }
}
