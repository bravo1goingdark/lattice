//! Trigram extraction module.
//!
//! Provides efficient extraction of 3-character sequences from text.
//! Trigrams are the foundation of Lattice's fuzzy search capability.

use lattice_types::Trigram;

/// Extracts trigrams from text using a sliding window.
///
/// For text shorter than 3 characters, no trigrams are emitted.
/// For text of length N, exactly N-2 trigrams are emitted.
///
/// # Example
///
/// ```
/// use lattice_core::analyzer::trigram::extract_trigrams;
/// use lattice_types::Trigram;
///
/// let mut trigrams = Vec::new();
/// extract_trigrams("hello", |t| trigrams.push(t));
///
/// assert_eq!(trigrams.len(), 3); // "hel", "ell", "llo"
/// ```
#[inline(always)]
pub fn extract_trigrams<F>(text: &str, mut callback: F)
where
    F: FnMut(Trigram),
{
    let bytes = text.as_bytes();
    if bytes.len() < 3 {
        return;
    }

    // Use iterator for potential auto-vectorization
    for window in bytes.windows(3) {
        callback(Trigram::from_bytes(window[0], window[1], window[2]));
    }
}

/// Counts trigrams without allocating.
///
/// Returns 0 for text shorter than 3 characters.
#[inline(always)]
pub fn count_trigrams(text: &str) -> usize {
    let len = text.len();
    if len < 3 {
        0
    } else {
        len - 2
    }
}

/// Extracts trigrams from text with position information.
///
/// The callback receives (trigram, byte_position) for each trigram.
/// Position is the starting byte index of the trigram in the original text.
#[inline(always)]
pub fn extract_trigrams_with_pos<F>(text: &str, mut callback: F)
where
    F: FnMut(Trigram, usize),
{
    let bytes = text.as_bytes();
    if bytes.len() < 3 {
        return;
    }

    for (i, window) in bytes.windows(3).enumerate() {
        callback(Trigram::from_bytes(window[0], window[1], window[2]), i);
    }
}

/// Trait for types that can extract trigrams.
///
/// This allows custom tokenization strategies while reusing
/// the same indexing infrastructure.
pub trait TrigramExtractor {
    /// Extracts all trigrams from text.
    fn extract<F>(&self, text: &str, callback: F)
    where
        F: FnMut(Trigram);
}

/// Standard sliding-window extractor.
pub struct SlidingWindowExtractor;

impl TrigramExtractor for SlidingWindowExtractor {
    #[inline(always)]
    fn extract<F>(&self, text: &str, callback: F)
    where
        F: FnMut(Trigram),
    {
        extract_trigrams(text, callback);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_basic() {
        let mut trigrams = Vec::new();
        extract_trigrams("hello", |t| trigrams.push(t));

        assert_eq!(trigrams.len(), 3);
        assert_eq!(
            trigrams[0].as_u32(),
            Trigram::from_bytes(b'h', b'e', b'l').as_u32()
        );
        assert_eq!(
            trigrams[1].as_u32(),
            Trigram::from_bytes(b'e', b'l', b'l').as_u32()
        );
        assert_eq!(
            trigrams[2].as_u32(),
            Trigram::from_bytes(b'l', b'l', b'o').as_u32()
        );
    }

    #[test]
    fn extract_short_text() {
        let mut trigrams = Vec::new();
        extract_trigrams("ab", |t| trigrams.push(t));
        assert!(trigrams.is_empty());

        extract_trigrams("", |t| trigrams.push(t));
        assert!(trigrams.is_empty());

        extract_trigrams("a", |t| trigrams.push(t));
        assert!(trigrams.is_empty());
    }

    #[test]
    fn extract_exactly_three() {
        let mut trigrams = Vec::new();
        extract_trigrams("abc", |t| trigrams.push(t));

        assert_eq!(trigrams.len(), 1);
        assert_eq!(
            trigrams[0].as_u32(),
            Trigram::from_bytes(b'a', b'b', b'c').as_u32()
        );
    }

    #[test]
    fn count_basic() {
        assert_eq!(count_trigrams("hello"), 3);
        assert_eq!(count_trigrams("ab"), 0);
        assert_eq!(count_trigrams("abc"), 1);
        assert_eq!(count_trigrams("abcd"), 2);
    }

    #[test]
    fn extract_with_pos() {
        let mut results = Vec::new();
        extract_trigrams_with_pos("hello", |t, pos| results.push((t, pos)));

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].1, 0); // "hel" at position 0
        assert_eq!(results[1].1, 1); // "ell" at position 1
        assert_eq!(results[2].1, 2); // "llo" at position 2
    }

    #[test]
    fn sliding_window_extractor() {
        let extractor = SlidingWindowExtractor;
        let mut trigrams = Vec::new();
        extractor.extract("test", |t| trigrams.push(t));

        assert_eq!(trigrams.len(), 2); // "tes", "est"
    }

    #[test]
    fn unicode_handling() {
        // Unicode characters are handled as UTF-8 bytes
        let mut trigrams = Vec::new();
        extract_trigrams("café", |t| trigrams.push(t));

        // "café" in UTF-8 is: c a f 0xC3 0xA9 (5 bytes total)
        // Trigrams: "caf" (bytes 0-2), "af<0xC3>" (bytes 1-3), "f<0xC3><0xA9>" (bytes 2-4)
        assert_eq!(trigrams.len(), 3);
    }
}
