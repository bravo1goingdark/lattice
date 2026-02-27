//! Search algorithm logic.

use crate::index::types::{
    Candidate, Lattice, QueryTrigram, MAX_CANDIDATES, MAX_QUERY_LENGTH, MAX_QUERY_TRIGRAMS,
    MAX_SEED_POSTING_LIST, PREFIX_BONUS,
};
use lattice_types::{DocId, SearchResult, Trigram};
use smallvec::SmallVec;

impl Lattice {
    /// Searches for documents matching the query.
    ///
    /// Returns owned results - no lifetime coupling with the engine.
    #[inline(never)]
    pub fn search(&mut self, query: &str, limit: usize) -> Vec<SearchResult> {
        self.query_count += 1;

        if self.is_empty() || limit == 0 {
            return Vec::new();
        }

        if self.needs_rebuild {
            self.rebuild_index();
        }

        if query.len() > MAX_QUERY_LENGTH {
            return Vec::new();
        }

        // Use reusable buffer to avoid allocation per search
        self.query_buf.clear();
        self.normalizer.normalize_into(query, &mut self.query_buf);
        let query_bytes = self.query_buf.as_bytes();

        if query_bytes.len() < 3 {
            return Vec::new();
        }

        let max_trigrams = (query_bytes.len() - 2).min(MAX_QUERY_TRIGRAMS);
        let mut query_trigrams: SmallVec<[QueryTrigram; MAX_QUERY_TRIGRAMS]> =
            SmallVec::with_capacity(max_trigrams);

        for i in 0..max_trigrams {
            let trigram =
                Trigram::from_bytes(query_bytes[i], query_bytes[i + 1], query_bytes[i + 2]);
            let bonus = if i < 3 { PREFIX_BONUS } else { 1 };
            if let Some(idx) = self.find_block(trigram) {
                let b = &self.blocks[idx];
                query_trigrams.push(QueryTrigram {
                    offset: b.offset,
                    len: b.len,
                    bonus,
                });
            }
        }

        if query_trigrams.is_empty() {
            return Vec::new();
        }

        query_trigrams.sort_unstable_by_key(|qt| qt.len);

        if query_trigrams[0].len as usize > MAX_SEED_POSTING_LIST {
            return Vec::new();
        }

        let total = query_trigrams.len();
        let required_end = ((total as f32 * self.config.min_overlap_ratio)
            .ceil()
            .max(1.0) as usize)
            .min(total);

        self.candidates.clear();
        let qt0 = query_trigrams[0];

        if qt0.len > MAX_CANDIDATES {
            return Vec::new();
        }

        let seed = &self.postings[qt0.offset as usize..(qt0.offset + qt0.len) as usize];
        self.candidates.reserve(qt0.len as usize);
        for &doc_id in seed {
            self.candidates.push(Candidate {
                doc_id,
                matches: qt0.bonus as u16,
            });
        }

        for i in 1..required_end {
            let qt = query_trigrams[i];
            let postings = &self.postings[qt.offset as usize..(qt.offset + qt.len) as usize];
            Self::hard_intersect(&mut self.candidates, postings, qt.bonus);

            if self.candidates.is_empty() {
                return Vec::new();
            }
        }

        for i in required_end..total {
            let qt = query_trigrams[i];
            let postings = &self.postings[qt.offset as usize..(qt.offset + qt.len) as usize];
            Self::soft_merge(&mut self.candidates, postings, qt.bonus);
        }

        self.results.clear();
        self.results.reserve(self.candidates.len().min(limit));
        for candidate in &self.candidates {
            let score =
                self.compute_score_fast(candidate.doc_id, candidate.matches as usize, total);
            self.results
                .push(SearchResult::new(candidate.doc_id, score));
        }

        if self.results.len() > limit {
            self.results.select_nth_unstable_by(limit, |a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(core::cmp::Ordering::Equal)
            });
            self.results.truncate(limit);
        }
        self.results.sort_unstable_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(core::cmp::Ordering::Equal)
        });

        self.results.clone().into_vec()
    }

    #[inline(always)]
    fn hard_intersect(candidates: &mut SmallVec<[Candidate; 256]>, postings: &[DocId], bonus: u8) {
        let bonus_u16 = bonus as u16;
        let mut write_idx = 0usize;
        let mut posting_idx = 0usize;

        for read_idx in 0..candidates.len() {
            let candidate = candidates[read_idx];

            while posting_idx < postings.len() && postings[posting_idx] < candidate.doc_id {
                posting_idx += 1;
            }

            if posting_idx < postings.len() && postings[posting_idx] == candidate.doc_id {
                candidates[write_idx] = Candidate {
                    doc_id: candidate.doc_id,
                    matches: candidate.matches + bonus_u16,
                };
                write_idx += 1;
                posting_idx += 1;
            }
        }

        candidates.truncate(write_idx);
    }

    #[inline(always)]
    fn soft_merge(candidates: &mut SmallVec<[Candidate; 256]>, postings: &[DocId], bonus: u8) {
        let bonus_u16 = bonus as u16;
        let mut posting_idx = 0usize;

        for candidate in candidates.iter_mut() {
            while posting_idx < postings.len() && postings[posting_idx] < candidate.doc_id {
                posting_idx += 1;
            }
            if posting_idx < postings.len() && postings[posting_idx] == candidate.doc_id {
                candidate.matches += bonus_u16;
                posting_idx += 1;
            }
        }
    }

    #[inline(always)]
    pub(crate) fn find_block(&self, trigram: Trigram) -> Option<usize> {
        self.blocks
            .binary_search_by_key(&trigram.0, |b| b.trigram.0)
            .ok()
    }
}
