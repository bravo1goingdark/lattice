//! Search-related types and configuration.

use core::fmt;

use crate::DocId;

/// Search result containing a document ID and relevance score.
///
/// Results are ordered by score (descending), then by doc_id (ascending).
/// Higher scores indicate better matches.
#[derive(Debug, Clone, Copy)]
pub struct SearchResult {
    /// Document identifier
    pub doc_id: DocId,
    /// Relevance score (higher is better)
    pub score: f32,
}

impl PartialEq for SearchResult {
    fn eq(&self, other: &Self) -> bool {
        // Two results are equal if both doc_id AND score are equal
        self.doc_id == other.doc_id && self.score == other.score
    }
}

impl Eq for SearchResult {}

impl PartialOrd for SearchResult {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchResult {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Primary: score (higher = greater for intuitive comparison)
        // Secondary: doc_id (for deterministic ordering when scores are equal)
        match self.score.total_cmp(&other.score) {
            core::cmp::Ordering::Equal => self.doc_id.cmp(&other.doc_id),
            ord => ord,
        }
    }
}

impl SearchResult {
    /// Creates a new search result.
    #[inline(always)]
    pub const fn new(doc_id: DocId, score: f32) -> Self {
        Self { doc_id, score }
    }
}

impl fmt::Display for SearchResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "doc={} score={:.3}", self.doc_id, self.score)
    }
}

/// Search configuration options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchConfig {
    /// Minimum trigram overlap ratio for a document to be considered (0.0-1.0).
    /// Default: 0.3 (30% of query trigrams must match)
    pub min_overlap_ratio: f32,
    /// Whether to enable fuzzy reranking with edit distance.
    pub enable_fuzzy: bool,
    /// Maximum edit distance for fuzzy matching (0 = exact only).
    pub max_edit_distance: u8,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            min_overlap_ratio: 0.3,
            enable_fuzzy: true,
            max_edit_distance: 2,
        }
    }
}

impl SearchConfig {
    /// Creates a configuration for exact matching only (no fuzziness).
    pub const fn exact() -> Self {
        Self {
            min_overlap_ratio: 0.5,
            enable_fuzzy: false,
            max_edit_distance: 0,
        }
    }

    /// Creates a configuration for fuzzy matching.
    pub const fn fuzzy() -> Self {
        Self {
            min_overlap_ratio: 0.2,
            enable_fuzzy: true,
            max_edit_distance: 2,
        }
    }
}
