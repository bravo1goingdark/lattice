//! Public API for adding and retrieving documents.

use crate::analyzer::trigram::extract_trigrams;
use crate::index::types::{Lattice, TempTrigramEntry, MAX_DOCUMENT_LENGTH};
use lattice_types::{DocId, DocumentError};

/// Checks if input contains invalid control characters (other than whitespace).
fn contains_invalid_controls(input: &str) -> bool {
    input
        .bytes()
        .any(|b| matches!(b, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F))
}

impl Lattice {
    /// Adds a document to the index.
    ///
    /// # Errors
    ///
    /// Returns `DocumentError::TooLarge` if the document exceeds 64KB.
    /// Returns `DocumentError::InvalidInput` if the document contains control characters.
    #[inline(never)]
    pub fn add(&mut self, content: &str) -> Result<DocId, DocumentError> {
        // Validate document length before processing
        if content.len() > MAX_DOCUMENT_LENGTH {
            return Err(DocumentError::TooLarge {
                size: content.len(),
                max_size: MAX_DOCUMENT_LENGTH,
            });
        }

        // Check for control characters (null bytes, bells, etc.)
        if contains_invalid_controls(content) {
            return Err(DocumentError::InvalidInput {
                reason: "control characters (0x00-0x1F excluding whitespace) are not allowed",
            });
        }

        self.norm_buf.clear();
        self.normalizer.normalize_into(content, &mut self.norm_buf);

        let doc_len = self.norm_buf.len() as u32;
        let doc_id = self
            .documents
            .push(&self.norm_buf)
            .ok_or(DocumentError::TooLarge {
                size: self.norm_buf.len(),
                max_size: MAX_DOCUMENT_LENGTH,
            })?;
        self.doc_lengths.push(doc_len);
        self.documents_added += 1;

        if self.norm_buf.len() >= 3 {
            extract_trigrams(&self.norm_buf, |trigram| {
                self.temp_trigrams
                    .push(TempTrigramEntry { trigram, doc_id });
            });
            self.needs_rebuild = true;
        }

        Ok(doc_id)
    }

    /// Adds multiple documents in batch for better performance.
    ///
    /// Returns a tuple of (success_count, error_count) and the last error encountered.
    pub fn add_batch(&mut self, contents: &[&str]) -> (usize, usize, Option<DocumentError>) {
        let mut added = 0;
        let mut failed = 0;
        let mut last_error = None;

        for content in contents {
            match self.add(content) {
                Ok(_) => added += 1,
                Err(e) => {
                    failed += 1;
                    last_error = Some(e);
                }
            }
        }
        (added, failed, last_error)
    }

    /// Retrieves a document by its ID.
    #[inline(always)]
    pub fn get(&self, doc_id: DocId) -> Option<&str> {
        self.documents.get(doc_id)
    }
}
