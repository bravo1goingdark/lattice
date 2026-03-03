//! Search algorithm logic.

use crate::index::types::{
    Candidate, Lattice, QueryTrigram, MAX_CANDIDATES, MAX_QUERY_LENGTH, MAX_QUERY_TRIGRAMS,
    MAX_SEED_POSTING_LIST, PREFIX_BONUS,
};
use lattice_types::{DocId, SearchResult, Trigram};
use rustc_hash::FxHashMap;
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
        // Store trigram values alongside for uncommitted search
        let mut query_trigram_values: SmallVec<[(Trigram, u8); MAX_QUERY_TRIGRAMS]> =
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
            // Always track for uncommitted search
            query_trigram_values.push((trigram, bonus));
        }

        // Check if we have any trigrams to search (committed or uncommitted)
        let has_committed = !query_trigrams.is_empty();
        let has_uncommitted = !self.temp_trigrams.is_empty();

        if !has_committed && !has_uncommitted {
            return Vec::new();
        }

        // Calculate required_end based on total trigrams
        let total_trigrams = query_trigram_values.len();
        let required_end = ((total_trigrams as f32 * self.config.min_overlap_ratio)
            .ceil()
            .max(1.0) as usize)
            .min(total_trigrams);

        // If only uncommitted data, build candidates from uncommitted only
        if !has_committed {
            let uncommitted = self.scan_uncommitted_trigrams(&query_trigram_values, required_end);
            self.candidates.clear();
            self.candidates.reserve(uncommitted.len());
            for (doc_id, matches) in uncommitted {
                self.candidates.push(Candidate { doc_id, matches });
            }

            // Score and return results
            self.results.clear();
            self.results.reserve(self.candidates.len().min(limit));
            for candidate in &self.candidates {
                let score = self.compute_score_fast(
                    candidate.doc_id,
                    candidate.matches as usize,
                    total_trigrams,
                );
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

            return std::mem::take(&mut self.results).into_vec();
        }

        query_trigrams.sort_unstable_by_key(|qt| qt.len);

        if query_trigrams[0].len as usize > MAX_SEED_POSTING_LIST {
            return Vec::new();
        }

        // total_trigrams and required_end already calculated above

        self.candidates.clear();
        let qt0 = query_trigrams[0];

        if qt0.len > MAX_CANDIDATES {
            return Vec::new();
        }

        let seed = &self.postings[qt0.offset as usize..(qt0.offset + qt0.len) as usize];
        self.candidates.reserve(qt0.len as usize);
        self.candidates.extend(seed.iter().map(|&doc_id| Candidate {
            doc_id,
            matches: qt0.bonus as u16,
        }));

        for i in 1..required_end {
            let qt = query_trigrams[i];
            let postings = &self.postings[qt.offset as usize..(qt.offset + qt.len) as usize];
            Self::hard_intersect(&mut self.candidates, postings, qt.bonus);

            if self.candidates.is_empty() {
                return Vec::new();
            }
        }

        for i in required_end..query_trigrams.len() {
            let qt = query_trigrams[i];
            let postings = &self.postings[qt.offset as usize..(qt.offset + qt.len) as usize];
            Self::soft_merge(&mut self.candidates, postings, qt.bonus);
        }

        // Merge in uncommitted trigrams (lazy rebuild optimization)
        // This is O(threshold) which is bounded and small
        if !self.temp_trigrams.is_empty() {
            let uncommitted = self.scan_uncommitted_trigrams(&query_trigram_values, required_end);
            self.merge_uncommitted_into_candidates(uncommitted, required_end);
        }

        self.results.clear();
        self.results.reserve(self.candidates.len().min(limit));
        for candidate in &self.candidates {
            let score = self.compute_score_fast(
                candidate.doc_id,
                candidate.matches as usize,
                total_trigrams,
            );
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

        std::mem::take(&mut self.results).into_vec()
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

    /// Scans uncommitted trigrams and builds posting lists for query trigrams.
    ///
    /// This is called when we have uncommitted trigrams (lazy rebuild mode).
    /// Returns a map of doc_id -> match count for uncommitted data.
    /// Time complexity: O(num_query_trigrams * log(temp_trigrams)) due to binary search.
    fn scan_uncommitted_trigrams(
        &self,
        query_trigrams: &[(Trigram, u8)],
        required_end: usize,
    ) -> FxHashMap<DocId, u16> {
        let mut uncommitted_matches: FxHashMap<DocId, u16> =
            FxHashMap::with_capacity_and_hasher(64, Default::default());

        for (i, (trigram, bonus)) in query_trigrams.iter().enumerate() {
            let is_required = i < required_end;
            let bonus_u16 = *bonus as u16;

            // Binary search for the trigram in temp_trigrams
            // temp_trigrams is sorted by trigram, then doc_id
            let pos = self
                .temp_trigrams
                .binary_search_by_key(&trigram.0, |e| e.trigram.0);

            if let Ok(mut idx) = pos {
                // Scan backward to find the first occurrence
                while idx > 0 && self.temp_trigrams[idx - 1].trigram.0 == trigram.0 {
                    idx -= 1;
                }

                // Scan forward through all matches
                while idx < self.temp_trigrams.len()
                    && self.temp_trigrams[idx].trigram.0 == trigram.0
                {
                    let doc_id = self.temp_trigrams[idx].doc_id;

                    if is_required {
                        // For required trigrams, mark them but we'll filter later
                        uncommitted_matches
                            .entry(doc_id)
                            .and_modify(|c| *c += bonus_u16)
                            .or_insert(bonus_u16);
                    } else {
                        // For soft merge, just add the bonus
                        uncommitted_matches
                            .entry(doc_id)
                            .and_modify(|c| *c += bonus_u16)
                            .or_insert(bonus_u16);
                    }

                    idx += 1;
                }
            }
        }

        uncommitted_matches
    }

    /// Merges uncommitted matches into the candidate list.
    fn merge_uncommitted_into_candidates(
        &mut self,
        uncommitted: FxHashMap<DocId, u16>,
        required_query_trigrams: usize,
    ) {
        if required_query_trigrams == 0 {
            // No required trigrams - just add all uncommitted matches
            for (doc_id, matches) in uncommitted {
                self.candidates.push(Candidate { doc_id, matches });
            }
            return;
        }

        // We need to handle the case where some docs in uncommitted have required trigrams
        // but weren't in the committed candidates (because they didn't have the seed trigram)
        // For simplicity, we add all uncommitted matches and rely on the fact that
        // the scoring will be lower for docs with fewer matches
        let mut existing: FxHashMap<DocId, usize> =
            FxHashMap::with_capacity_and_hasher(self.candidates.len(), Default::default());
        for (idx, c) in self.candidates.iter().enumerate() {
            existing.insert(c.doc_id, idx);
        }

        for (doc_id, matches) in uncommitted {
            if let Some(&idx) = existing.get(&doc_id) {
                self.candidates[idx].matches += matches;
            } else {
                self.candidates.push(Candidate { doc_id, matches });
            }
        }
    }
}
