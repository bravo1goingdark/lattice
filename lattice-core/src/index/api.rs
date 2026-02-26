//! Public API for adding and retrieving documents.

use crate::analyzer::trigram::extract_trigrams;
use crate::index::types::{Lattice, TempTrigramEntry};
use lattice_types::DocId;

impl Lattice {
    /// Adds a document to the index.
    #[inline(never)]
    pub fn add(&mut self, content: &str) -> DocId {
        self.norm_buf.clear();
        self.normalizer.normalize_into(content, &mut self.norm_buf);

        let doc_len = self.norm_buf.len() as u32;
        let doc_id = self.documents.push(&self.norm_buf);
        self.doc_lengths.push(doc_len);

        if self.norm_buf.len() >= 3 {
            extract_trigrams(&self.norm_buf, |trigram| {
                self.temp_trigrams
                    .push(TempTrigramEntry { trigram, doc_id });
            });
            self.needs_rebuild = true;
        }

        doc_id
    }

    /// Retrieves a document by its ID.
    #[inline(always)]
    pub fn get(&self, doc_id: DocId) -> Option<&str> {
        self.documents.get(doc_id)
    }
}
