//! Index types and constants.

use crate::analyzer::normalizer::TextNormalizer;

use crate::arena::Arena;
use lattice_types::{DocId, SearchConfig, SearchResult, Trigram};

use smallvec::SmallVec;

pub const MAX_QUERY_TRIGRAMS: usize = 30;

pub const PREFIX_BONUS: u8 = 2;

pub const MAX_CANDIDATES: usize = 100_000;

pub const MAX_QUERY_LENGTH: usize = 1_000;

pub const MAX_SEED_POSTING_LIST: usize = 100_000;

pub const RADIX_SORT_THRESHOLD: usize = 512;

#[derive(Clone, Copy, Debug)]
pub struct PostingBlock {
    pub trigram: Trigram,
    pub offset: u32,
    pub len: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub doc_id: DocId,
    pub matches: u16,
}

#[derive(Clone, Copy)]
pub struct TempTrigramEntry {
    pub trigram: Trigram,
    pub doc_id: DocId,
}

#[derive(Clone, Copy)]
pub struct QueryTrigram {
    pub offset: u32,
    pub len: u32,
    pub bonus: u8,
}

/// High-performance fuzzy search engine.
pub struct Lattice {
    pub(crate) blocks: Vec<PostingBlock>,
    pub(crate) postings: Vec<DocId>,
    pub(crate) documents: Arena,
    pub(crate) doc_lengths: Vec<u32>,
    pub(crate) normalizer: TextNormalizer,
    pub(crate) config: SearchConfig,
    pub(crate) temp_trigrams: Vec<TempTrigramEntry>,
    pub(crate) needs_rebuild: bool,
    pub(crate) candidates: SmallVec<[Candidate; 256]>,
    pub(crate) results: SmallVec<[SearchResult; 64]>,
    pub(crate) norm_buf: String,
}

impl Default for Lattice {
    fn default() -> Self {
        Self::new()
    }
}

impl Lattice {
    /// Creates a new, empty search engine.
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            postings: Vec::new(),
            documents: Arena::with_capacity(1024 * 1024, 1024),
            doc_lengths: Vec::new(),
            normalizer: TextNormalizer::new(),
            config: SearchConfig::default(),
            temp_trigrams: Vec::new(),
            needs_rebuild: false,
            candidates: SmallVec::new(),
            results: SmallVec::new(),
            norm_buf: String::with_capacity(256),
        }
    }

    /// Creates a new engine with custom configuration.
    pub fn with_config(search_config: SearchConfig) -> Self {
        Self {
            config: search_config,
            ..Self::new()
        }
    }

    /// Returns the number of documents in the index.
    #[inline(always)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Returns `true` if the index contains no documents.
    #[inline(always)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Removes all documents and resets the index.
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.postings.clear();
        self.documents.clear();
        self.doc_lengths.clear();
        self.temp_trigrams.clear();
        self.needs_rebuild = false;
    }
}
