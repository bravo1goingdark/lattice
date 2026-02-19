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

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

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
        let mut prev_space = false;

        for &b in input.as_bytes() {
            match b {
                b'A'..=b'Z' => {
                    out.push((b + 32) as char); // to lowercase
                    prev_space = false;
                }
                b' ' | b'\n' | b'\t' | b'\r' => {
                    if self.config.collapse_whitespaces {
                        if !prev_space {
                            out.push(' ');
                            prev_space = true;
                        }
                    } else {
                        out.push(b as char);
                    }
                }
                _ => {
                    out.push(b as char);
                    prev_space = false;
                }
            }
        }

        if self.config.collapse_whitespaces && out.ends_with(' ') {
            out.pop();
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

        if self.config.collapse_whitespaces && out.ends_with(' ') {
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
