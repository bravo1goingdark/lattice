//! Statistics and IndexStats.

use crate::index::types::Lattice;
use lattice_types::DocId;

/// A snapshot of index statistics.
#[derive(Debug, Clone, Copy)]
pub struct IndexStats {
    /// Number of documents in the index.
    pub num_documents: usize,
    /// Number of unique trigrams.
    pub num_trigrams: usize,
    /// Total number of postings.
    pub total_postings: usize,
    /// Compressed size in bytes, if computed.
    pub compressed_postings_bytes: Option<usize>,
    /// Compression ratio, if computed.
    pub compression_ratio: Option<f32>,
}

impl Lattice {
    /// Returns index statistics.
    pub fn stats(&self) -> IndexStats {
        IndexStats {
            num_documents: self.documents.len(),
            num_trigrams: self.blocks.len(),
            total_postings: self.postings.len(),
            compressed_postings_bytes: None,
            compression_ratio: None,
        }
    }

    /// Returns index statistics including compression analysis.
    pub fn stats_with_compression(&self) -> IndexStats {
        let (compressed, ratio) = self.compress_postings();
        IndexStats {
            num_documents: self.documents.len(),
            num_trigrams: self.blocks.len(),
            total_postings: self.postings.len(),
            compressed_postings_bytes: Some(compressed),
            compression_ratio: Some(ratio),
        }
    }

    /// Estimates compressed size of posting lists.
    pub fn compress_postings(&self) -> (usize, f32) {
        use lattice_types::compression::compress_sorted;

        if self.postings.is_empty() {
            return (0, 1.0);
        }

        let mut total_compressed = 0usize;
        let mut buf = Vec::new();

        for block in &self.blocks {
            buf.clear();
            if let Ok(bytes) =
                compress_sorted(Self::block_postings(block, &self.postings), &mut buf)
            {
                total_compressed += bytes;
            }
        }

        let original_bytes = self.postings.len() * std::mem::size_of::<DocId>();
        let ratio = if original_bytes > 0 {
            total_compressed as f32 / original_bytes as f32
        } else {
            1.0
        };

        (total_compressed, ratio)
    }
}

impl IndexStats {
    /// Constructs stats from an engine.
    pub fn from_engine(engine: &Lattice, compute_compression: bool) -> Self {
        let (compressed, ratio) = if compute_compression {
            let (b, r) = engine.compress_postings();
            (Some(b), Some(r))
        } else {
            (None, None)
        };

        Self {
            num_documents: engine.documents.len(),
            num_trigrams: engine.blocks.len(),
            total_postings: engine.postings.len(),
            compressed_postings_bytes: compressed,
            compression_ratio: ratio,
        }
    }

    /// Returns approximate memory usage in bytes.
    pub fn memory_usage_bytes(&self) -> usize {
        let blocks_size = self.num_trigrams * std::mem::size_of::<u32>() * 3;
        let postings_size = self.total_postings * std::mem::size_of::<u32>();
        blocks_size + postings_size
    }
}

impl core::fmt::Display for IndexStats {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} docs, {} trigrams, {} postings",
            self.num_documents, self.num_trigrams, self.total_postings
        )?;

        if let (Some(compressed), Some(ratio)) =
            (self.compressed_postings_bytes, self.compression_ratio)
        {
            let original = self.total_postings * 4;
            let savings = original.saturating_sub(compressed);
            write!(
                f,
                ", compressed: {} bytes ({:.1}%, saved {} bytes)",
                compressed,
                ratio * 100.0,
                savings
            )?;
        }

        Ok(())
    }
}
