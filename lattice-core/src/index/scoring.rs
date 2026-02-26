//! Scoring functions.

use crate::index::types::Lattice;
use lattice_types::DocId;

impl Lattice {
    #[inline(always)]
    pub(crate) fn compute_score_fast(
        &self,
        doc_id: DocId,
        matches: usize,
        query_trigrams: usize,
    ) -> f32 {
        let doc_len = self.doc_lengths.get(doc_id as usize).copied().unwrap_or(0) as usize;

        let len_factor = if doc_len > 0 {
            100.0 / (1.0 + (doc_len as f32).sqrt())
        } else {
            100.0
        };

        let match_ratio = matches as f32 / query_trigrams.max(1) as f32;
        match_ratio * match_ratio * len_factor
    }
}
