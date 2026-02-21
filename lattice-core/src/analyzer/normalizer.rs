//! Internal text normalization for fuzzy search pipelines.
//!
//! This module provides a deterministic normalization pipeline used internally
//! by the fuzzy search indexer. It is **not** part of the public API and may
//! change without notice.
//!
//! # Pipeline
//!
//! 1. ASCII fast‑path – direct lowercasing and whitespace handling for ASCII inputs.
//! 2. Unicode NFC – ensures consistent codepoint sequences.
//! 3. Lowercase folding – full Unicode case folding to lowercase.
//! 4. Diacritic stripping – optional removal of combining marks via NFD decomposition.
//! 5. Whitespace collapsing – optional replacement of consecutive whitespace runs
//!    with a single ASCII space. When enabled, a trailing space is trimmed.
//!
//! # Internal Notes
//!
//! - The output buffer is reused to avoid allocations; callers must clear it
//!   appropriately (done by `normalize_into`).
//! - The ASCII path is branch‑predictable and avoids Unicode overhead.
//! - The Unicode path uses lazy iterators; no intermediate allocations occur.
//! - for more performance this can be later implemented in-house instead of using crates

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

#[rustfmt::skip]
const LOWERCASE_TABLE: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f,
    0x40, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f,
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f,
    0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f,
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f,
    0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f,
    0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0x9b, 0x9c, 0x9d, 0x9e, 0x9f,
    0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf,
    0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf,
    0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf,
    0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf,
    0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef,
    0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe, 0xff,
];

const fn is_ascii_whitespace(b: u8) -> bool {
    b == b' ' || b == b'\n' || b == b'\t' || b == b'\r'
}

/// Internal configuration flags for the normalization pipeline.
#[derive(Debug, Clone, Copy)]
pub struct NormalizationConfig {
    /// Remove combining diacritical marks after NFD decomposition.
    /// e.g., `é` → `e`, `ü` → `u`.
    pub strip_diacritics: bool,
    /// Collapse runs of whitespace (space, tab, newline, carriage return)
    /// into a single ASCII space. If enabled, a trailing space is trimmed.
    pub collapse_whitespaces: bool,
}

impl Default for NormalizationConfig {
    #[inline]
    fn default() -> Self {
        Self {
            strip_diacritics: true,
            collapse_whitespaces: true,
        }
    }
}

/// Internal normalizer implementing the full pipeline.
///
/// This struct holds configuration and can be reused across many inputs.
/// It does not retain per‑call state, making it safe to share across threads.
pub struct TextNormalizer {
    config: NormalizationConfig,
}

impl Default for TextNormalizer {
    #[inline]
    fn default() -> Self {
        Self::new(NormalizationConfig::default())
    }
}

impl TextNormalizer {
    /// Creates a new normalizer with the given configuration.
    #[inline]
    pub fn new(config: NormalizationConfig) -> Self {
        Self { config }
    }

    /// Normalizes `input` and writes the result into `out`.
    ///
    /// Clears `out` before writing. This method reuses the buffer's allocation
    /// and should be preferred in hot paths.
    ///
    /// # Internal behavior
    /// - If `collapse_whitespaces` is enabled, trailing space is trimmed.
    /// - The input is processed either via ASCII fast‑path or full Unicode path.
    #[inline]
    pub fn normalize_into(&self, input: &str, out: &mut String) {
        out.clear();

        if input.is_ascii() {
            self.normalize_ascii_into(input, out);
        } else {
            self.normalize_unicode_into(input, out);
        }
    }

    /// Normalizes `input` and returns the result as a new [`String`].
    ///
    /// For repeated use, prefer [`normalize_into`] to avoid allocations.
    #[inline]
    pub fn normalize(&self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        self.normalize_into(input, &mut out);
        out
    }

    #[inline]
    fn normalize_ascii_into(&self, input: &str, out: &mut String) {
        let bytes = input.as_bytes();
        if bytes.is_empty() {
            return;
        }

        out.reserve(bytes.len());

        if self.config.collapse_whitespaces {
            self.normalize_ascii_collapse(bytes, out);
        } else {
            self.normalize_ascii_no_collapse(bytes, out);
        }
    }

    #[inline]
    fn normalize_ascii_collapse(&self, bytes: &[u8], out: &mut String) {
        let mut prev_space = false;
        let mut wrote = 0usize;

        unsafe {
            // SAFETY: `out` was cleared and `reserve(bytes.len())` in the caller
            // guarantees capacity >= bytes.len(). We write at most one byte per
            // input byte, so `wrote` never exceeds capacity. `set_len(wrote)` is
            // called only after all writes complete, so no uninitialized bytes
            // are ever exposed.

            let buf = out.as_mut_vec();

            for &b in bytes {
                if is_ascii_whitespace(b) {
                    if !prev_space {
                        *buf.as_mut_ptr().add(wrote) = b' ';
                        wrote += 1;
                        prev_space = true;
                    }
                } else {
                    *buf.as_mut_ptr().add(wrote) = *LOWERCASE_TABLE.get_unchecked(b as usize);
                    wrote += 1;
                    prev_space = false;
                }
            }

            if prev_space {
                wrote -= 1;
            }
            buf.set_len(wrote);
        }
    }

    #[inline]
    fn normalize_ascii_no_collapse(&self, bytes: &[u8], out: &mut String) {
        unsafe {
            let buf = out.as_mut_vec();

            for (i, &b) in bytes.iter().enumerate() {
                *buf.as_mut_ptr().add(i) = *LOWERCASE_TABLE.get_unchecked(b as usize);
            }

            buf.set_len(bytes.len());
        }
    }

    fn normalize_unicode_into(&self, input: &str, out: &mut String) {
        let mut prev_space = false;

        let folded = input.nfc().flat_map(|c| c.to_lowercase());

        if self.config.strip_diacritics {
            for ch in folded.nfd() {
                if is_combining_mark(ch) {
                    continue;
                }
                self.push_char(ch, out, &mut prev_space);
            }
        } else {
            for ch in folded {
                self.push_char(ch, out, &mut prev_space);
            }
        }

        if self.config.collapse_whitespaces && prev_space {
            out.pop();
        }
    }

    #[inline(always)]
    fn push_char(&self, ch: char, out: &mut String, prev_space: &mut bool) {
        if ch.is_whitespace() {
            if self.config.collapse_whitespaces {
                if !*prev_space {
                    out.push(' ');
                    *prev_space = true;
                }
            } else {
                out.push(ch);
            }
        } else {
            out.push(ch);
            *prev_space = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm_default(input: &str) -> String {
        TextNormalizer::default().normalize(input)
    }

    fn norm_with(input: &str, config: NormalizationConfig) -> String {
        TextNormalizer::new(config).normalize(input)
    }

    #[test]
    fn test_ascii_lowercasing() {
        assert_eq!(norm_default("HELLO"), "hello");
        assert_eq!(norm_default("HeLlO"), "hello");
        assert_eq!(norm_default("hello"), "hello");
        assert_eq!(norm_default("123 ABC!"), "123 abc!");
    }

    #[test]
    fn test_ascii_whitespace_collapse_default() {
        assert_eq!(norm_default("hello   world"), "hello world");
        assert_eq!(norm_default("hello\t\nworld"), "hello world");
        assert_eq!(norm_default("  leading spaces"), " leading spaces");
        assert_eq!(norm_default("trailing spaces   "), "trailing spaces");
        assert_eq!(norm_default("   multiple   spaces   "), " multiple spaces");
        assert_eq!(norm_default("no\texcess\n\rspaces"), "no excess spaces");
    }

    #[test]
    fn test_ascii_no_collapse() {
        let config = NormalizationConfig {
            collapse_whitespaces: false,
            ..Default::default()
        };
        assert_eq!(norm_with("hello   world", config), "hello   world");
        assert_eq!(norm_with("hello\t\nworld", config), "hello\t\nworld");
        assert_eq!(norm_with("  leading  ", config), "  leading  ");
    }

    #[test]
    fn test_unicode_lowercasing() {
        assert_eq!(norm_default("HÉLLO"), "hello");
        // 'ß' is already lowercase — to_lowercase() is a no-op on it
        assert_eq!(norm_default("Straße"), "straße");
        assert_eq!(norm_default("İstanbul"), "istanbul");

        let config_no_strip = NormalizationConfig {
            strip_diacritics: false,
            ..Default::default()
        };
        assert_eq!(norm_with("İstanbul", config_no_strip), "i\u{307}stanbul");
    }

    #[test]
    fn test_diacritic_stripping() {
        assert_eq!(norm_default("café"), "cafe");
        assert_eq!(norm_default("cafe\u{301}"), "cafe");
        assert_eq!(norm_default("Müller"), "muller");
        assert_eq!(norm_default("άλφα"), "αλφα");
        assert_eq!(norm_default("a\u{304}"), "a");
    }

    #[test]
    fn test_unicode_whitespace_collapse() {
        assert_eq!(norm_default("hello\u{00A0}world"), "hello world");
        assert_eq!(norm_default("hello\u{2002}world"), "hello world");
        assert_eq!(norm_default("hello \u{00A0}\u{2002} world"), "hello world");
        assert_eq!(norm_default("hello world\u{00A0}"), "hello world");
    }

    #[test]
    fn test_unicode_no_collapse() {
        let config = NormalizationConfig {
            collapse_whitespaces: false,
            strip_diacritics: true,
        };
        assert_eq!(
            norm_with("hello\u{00A0}world", config),
            "hello\u{00A0}world"
        );
        assert_eq!(norm_with("hello \t\nworld", config), "hello \t\nworld");
    }

    #[test]
    fn test_no_diacritic_stripping() {
        let config = NormalizationConfig {
            strip_diacritics: false,
            ..Default::default()
        };
        assert_eq!(norm_with("café", config), "café");
        assert_eq!(norm_with("cafe\u{301}", config), "café");
    }

    #[test]
    fn test_mixed_scripts() {
        assert_eq!(norm_default("Hello κόσμε мир"), "hello κοσμε мир");
        assert_eq!(norm_default("ПРИВЕТ"), "привет");
    }

    #[test]
    fn test_edge_cases() {
        assert_eq!(norm_default(""), "");
        assert_eq!(norm_default("   "), "");
        assert_eq!(norm_default("\t\n "), "");
        assert_eq!(norm_default("\u{301}\u{302}"), "");

        let config_no_strip = NormalizationConfig {
            strip_diacritics: false,
            ..Default::default()
        };
        let s = "\u{301}\u{302}";
        let out = norm_with(s, config_no_strip);
        assert_eq!(out, s);
    }

    #[test]
    fn test_normalize_into_reuses_buffer() {
        let normalizer = TextNormalizer::default();
        let mut buf = String::with_capacity(100);
        let initial_capacity = buf.capacity();

        normalizer.normalize_into("hello", &mut buf);
        assert_eq!(buf, "hello");
        assert_eq!(buf.capacity(), initial_capacity);

        normalizer.normalize_into("world", &mut buf);
        assert_eq!(buf, "world");
        assert_eq!(buf.capacity(), initial_capacity);

        let long = "a".repeat(200);
        normalizer.normalize_into(&long, &mut buf);
        assert_eq!(buf, long);
        assert!(buf.capacity() >= 200);
    }

    #[test]
    fn test_trailing_space_trimmed_only_when_collapse_enabled() {
        let config_collapse = NormalizationConfig {
            collapse_whitespaces: true,
            ..Default::default()
        };
        let normalizer_collapse = TextNormalizer::new(config_collapse);
        let mut out = String::new();
        normalizer_collapse.normalize_into("hello ", &mut out);
        assert_eq!(out, "hello");

        let config_no_collapse = NormalizationConfig {
            collapse_whitespaces: false,
            ..Default::default()
        };
        let normalizer_no_collapse = TextNormalizer::new(config_no_collapse);
        normalizer_no_collapse.normalize_into("hello ", &mut out);
        assert_eq!(out, "hello ");
    }

    #[test]
    fn test_unicode_trailing_space_trimmed() {
        let normalizer = TextNormalizer::default();
        let mut out = String::new();
        normalizer.normalize_into("café \u{00A0} ", &mut out);
        assert_eq!(out, "cafe");
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn idempotency_default(s in any::<String>()) {
                let normalizer = TextNormalizer::default();
                let once = normalizer.normalize(&s);
                let twice = normalizer.normalize(&once);
                prop_assert_eq!(once, twice);
            }

            #[test]
            fn idempotency_strip_only(s in any::<String>()) {
                let config = NormalizationConfig {
                    strip_diacritics: true,
                    collapse_whitespaces: false,
                };
                let normalizer = TextNormalizer::new(config);
                let once = normalizer.normalize(&s);
                let twice = normalizer.normalize(&once);
                prop_assert_eq!(once, twice);
            }

            #[test]
            fn idempotency_collapse_only(s in any::<String>()) {
                let config = NormalizationConfig {
                    strip_diacritics: false,
                    collapse_whitespaces: true,
                };
                let normalizer = TextNormalizer::new(config);
                let once = normalizer.normalize(&s);
                let twice = normalizer.normalize(&once);
                prop_assert_eq!(once, twice);
            }

            #[test]
            fn idempotency_no_strip_no_collapse(s in any::<String>()) {
                let config = NormalizationConfig {
                    strip_diacritics: false,
                    collapse_whitespaces: false,
                };
                let normalizer = TextNormalizer::new(config);
                let once = normalizer.normalize(&s);
                let twice = normalizer.normalize(&once);
                prop_assert_eq!(once, twice);
            }
        }
    }
}
