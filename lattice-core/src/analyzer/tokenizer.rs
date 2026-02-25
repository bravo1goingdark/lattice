//! Streaming Tokenizer Module
//!
//! This module provides a zero-allocation tokenizer that splits normalized text into
//! individual tokens for indexing. It's the second stage in our text processing pipeline,
//! taking clean, normalized text and breaking it into searchable units.
//!
//! ## What It Does
//!
//! Given normalized input like `"hello world foo bar"`, it emits each word as a token
//! with its position in the document:
//!
//! ```ignore
//! ("hello", Field::Body, 0)
//! ("world", Field::Body, 1)
//! ("foo", Field::Body, 2)
//! ("bar", Field::Body, 3)
//! ```
//!
//! ## Key Features
//!
//! - **Zero Allocation**: Tokens are slices of the original string, not new allocations
//! - **Streaming**: Uses a callback to emit tokens, no intermediate collection
//! - **Fast**: Simple byte-scan for ASCII space (0x20) splitting
//! - **Field-Aware**: Can specify which document field (title, body, tag) tokens belong to
//!
//! ## Usage
//!
//! ```rust
//! use lattice_core::analyzer::tokenizer::{Tokenizer, Field};
//!
//! let tokenizer = Tokenizer::new(Field::Body);
//!
//! // Tokens are emitted via callback - no allocation!
//! // Callback receives: text (&str), field (Field), position (u32)
//! tokenizer.tokenize("hello world", |text, field, position| {
//!     // Process token: text="hello"/"world", field=Body, position=0/1
//! });
//! ```
//!
//! ## The Input Contract
//!
//! The tokenizer expects **pre-normalized** input. This means:
//! - ASCII-only text (or the tokenizer may not split correctly)
//! - All lowercase
//! - No leading or trailing whitespace
//! - No consecutive spaces between words
//!
//! If you violate this contract, the tokenizer will panic in debug mode with a helpful message.
//!
//! ## Field Weights
//!
//! Different fields have different relevance for search scoring:
//! - **Title**: 3.0x weight (most important)
//! - **Tag**: 2.0x weight
//! - **Body**: 1.0x weight (baseline)
//!
//! The weights are available via `Field::weight()` and are used during relevance scoring.

use core::str;
use memchr::memchr_iter;

/// Logical document field.
///
/// In a typical search system, documents have different parts with different
/// importance. A title word is usually more important than the same word in
/// the body text. This enum represents those different fields.
///
/// `#[repr(u8)]` guarantees stable 1-byte layout for compact storage
/// in posting lists or other packed index structures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Field {
    /// Document title - highest importance
    Title = 0,
    /// Document body - baseline importance
    Body = 1,
    /// Tags/categories - medium importance
    Tag = 2,
}

impl Field {
    /// Static scoring weight for this field.
    ///
    /// Not stored per token; derived during scoring.
    #[must_use]
    #[inline(always)]
    pub const fn weight(self) -> f32 {
        match self {
            Field::Title => 3.0,
            Field::Body => 1.0,
            Field::Tag => 2.0,
        }
    }
}

/// Streaming tokenizer - splits normalized text into tokens.
///
/// A lightweight, zero-allocation tokenizer that takes normalized text and
/// emits tokens one by one via a callback. Think of it as a simple word splitter
/// that also tracks where each word appears (position) and which field it came from.
///
/// ## Zero Allocation
///
/// Tokens are not copied—they're slices (`&str`) into the original input string.
/// This means no heap allocations during tokenization, just a fast byte scan.
///
/// ## The Contract
///
/// ⚠️ This tokenizer expects **pre-normalized** input! Specifically:
/// - ASCII-only text (for reliable space-splitting)
/// - All lowercase
/// - No leading/trailing whitespace
/// - No consecutive spaces
///
/// If you violate this, you'll get a helpful panic in debug builds.
///
/// ## Example
///
/// ```
/// use lattice_core::analyzer::tokenizer::{Tokenizer, Field};
///
/// let tokenizer = Tokenizer::new(Field::Body);
/// let mut count = 0;
///
/// tokenizer.tokenize("hello world foo", |text, field, pos| {
///     count += 1;
///     // Each token is a slice of the original - no allocation!
/// });
///
/// assert_eq!(count, 3);
/// ```
///
/// ## How It Works
///
/// It does a single forward scan looking for ASCII space bytes (0x20).
/// Each non-space run between spaces becomes a token. Simple and fast.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct Tokenizer {
    field: Field,
}

impl Tokenizer {
    /// Creates a new tokenizer for the specified field.
    #[inline]
    pub const fn new(field: Field) -> Self {
        Self { field }
    }

    /// Tokenizes normalized input and emits `(text, field, position)`.
    ///
    /// Position is `u32`. After emitting a token at position `u32::MAX`,
    /// further emissions stop (overflow protection).
    #[inline(always)]
    #[allow(clippy::needless_lifetimes)]
    pub fn tokenize<'n, F>(&self, normalized: &'n str, mut emit: F)
    where
        F: FnMut(&'n str, Field, u32),
    {
        let bytes = normalized.as_bytes();

        debug_assert!(
            bytes.first().is_none_or(|&b| b != b' '),
            "tokenizer: leading whitespace — normalizer contract violated"
        );

        debug_assert!(
            bytes.last().is_none_or(|&b| b != b' '),
            "tokenizer: trailing whitespace — normalizer contract violated"
        );

        debug_assert!(
            {
                let mut prev_space = false;
                let mut ok = true;
                for &b in bytes {
                    if b == b' ' {
                        if prev_space {
                            ok = false;
                            break;
                        }
                        prev_space = true;
                    } else {
                        prev_space = false;
                    }
                }
                ok
            },
            "tokenizer: consecutive spaces — normalizer contract violated"
        );

        if bytes.is_empty() {
            return;
        }

        let field = self.field;
        let mut start = 0usize;
        let mut pos = 0u32;

        for i in memchr_iter(b' ', bytes) {
            if start < i {
                // SAFETY: `normalized` is valid UTF-8. We split only on ASCII space (0x20),
                // which is never a continuation byte, so `bytes[start..i]` is always a
                // valid UTF-8 subslice.
                let text = unsafe { str::from_utf8_unchecked(&bytes[start..i]) };
                emit(text, field, pos);
                if pos == u32::MAX {
                    return;
                }
                pos += 1;
            }
            start = i + 1;
        }

        if start < bytes.len() {
            // SAFETY: same invariants as above — `bytes[start..]` is a valid UTF-8
            // subslice since `start` was set to `i + 1` after an ASCII space byte.
            let text = unsafe { str::from_utf8_unchecked(&bytes[start..]) };
            emit(text, field, pos);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn collect(input: &str, field: Field) -> Vec<(&str, Field, u32)> {
        let mut out = Vec::new();
        Tokenizer::new(field).tokenize(input, |text, f, pos| {
            out.push((text, f, pos));
        });
        out
    }

    #[test]
    fn field_size_is_1_byte() {
        assert_eq!(size_of::<Field>(), 1);
    }

    #[test]
    fn single_word() {
        let out = collect("hello", Field::Body);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "hello");
        assert_eq!(out[0].2, 0);
    }

    #[test]
    fn two_words() {
        let out = collect("hello world", Field::Body);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "hello");
        assert_eq!(out[1].0, "world");
    }

    #[test]
    fn positions_are_sequential() {
        let out = collect("the quick brown fox", Field::Body);
        assert_eq!(out.len(), 4);
        for (i, (_, _, pos)) in out.iter().enumerate() {
            assert_eq!(*pos, i as u32);
        }
    }

    #[test]
    fn empty_emits_nothing() {
        let out = collect("", Field::Body);
        assert!(out.is_empty());
    }

    #[test]
    fn single_char_token() {
        let out = collect("a", Field::Body);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "a");
    }

    #[test]
    fn field_propagated_to_all_tokens() {
        let out = collect("hello world foo", Field::Title);
        for (_, field, _) in &out {
            assert_eq!(*field, Field::Title);
        }
    }

    #[test]
    fn weight_derivable_from_field() {
        assert_eq!(Field::Title.weight(), 3.0);
        assert_eq!(Field::Body.weight(), 1.0);
        assert_eq!(Field::Tag.weight(), 2.0);
    }

    #[test]
    fn tokens_are_slices_of_input() {
        let input = String::from("hello world");
        let base = input.as_ptr() as usize;
        let end = base + input.len();

        Tokenizer::new(Field::Body).tokenize(&input, |text, _, _| {
            let ptr = text.as_ptr() as usize;
            assert!(ptr >= base && ptr < end);
        });
    }

    #[test]
    fn emit_order_is_left_to_right() {
        let words = ["one", "two", "three", "four"];
        let input = words.join(" ");
        let mut i = 0usize;

        Tokenizer::new(Field::Body).tokenize(&input, |text, _, pos| {
            assert_eq!(text, words[i]);
            assert_eq!(pos, i as u32);
            i += 1;
        });

        assert_eq!(i, words.len());
    }

    #[test]
    fn tokenizer_is_reusable() {
        let t = Tokenizer::new(Field::Title);

        let mut n = 0usize;
        t.tokenize("hello world", |_, _, _| n += 1);
        assert_eq!(n, 2);

        n = 0;
        t.tokenize("one two three", |_, _, _| n += 1);
        assert_eq!(n, 3);
    }

    #[test]
    fn composes_with_ngram_layer() {
        let mut gram_count = 0usize;

        Tokenizer::new(Field::Title).tokenize("hello world", |text, _, _| {
            let len = text.len();
            if len >= 3 {
                gram_count += len - 2;
            }
        });

        assert_eq!(gram_count, 6);
    }
}
