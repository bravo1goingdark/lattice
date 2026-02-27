//! Index types and constants.

use crate::analyzer::normalizer::TextNormalizer;

use crate::arena::Arena;
use lattice_types::{DocId, SearchConfig, SearchResult, Trigram};

use smallvec::SmallVec;

pub const MAX_QUERY_TRIGRAMS: usize = 30;

pub const PREFIX_BONUS: u8 = 2;

pub const MAX_CANDIDATES: u32 = 100_000;

pub const MAX_QUERY_LENGTH: usize = 1_000;

/// Maximum document length (64KB - matches Arena u16 limit)
pub const MAX_DOCUMENT_LENGTH: usize = 65535;

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
    /// Reusable buffer for query normalization (avoids allocation per search)
    pub(crate) query_buf: String,
    /// Total number of queries executed
    pub(crate) query_count: u64,
    /// Total number of documents added
    pub(crate) documents_added: u64,
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
            query_buf: String::with_capacity(256),
            query_count: 0,
            documents_added: 0,
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
        self.query_count = 0;
        self.documents_added = 0;
    }

    /// Returns basic metrics about the engine's operation.
    #[inline(always)]
    #[must_use]
    pub fn metrics(&self) -> EngineMetrics {
        EngineMetrics {
            documents_indexed: self.documents_added,
            queries_executed: self.query_count,
            current_doc_count: self.documents.len() as u64,
        }
    }
}

/// Basic operational metrics for the search engine.
#[derive(Debug, Clone, Copy)]
pub struct EngineMetrics {
    /// Total number of documents added (including those that may have been cleared).
    pub documents_indexed: u64,
    /// Total number of search queries executed.
    pub queries_executed: u64,
    /// Current number of documents in the index.
    pub current_doc_count: u64,
}
