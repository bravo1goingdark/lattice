//! High-performance indexing infrastructure for the search engine.
//!
//! Optimized for ASCII-only, single-threaded, low-latency search workloads.
//! Uses merge-join intersection for cache-efficient posting list traversal.
//!
//! Memory Layout:
//! - Posting lists are stored in a single contiguous array for cache efficiency
//! - Metadata is stored in sorted blocks for binary search lookup
//! - Eliminates HashMap overhead and SmallVec heap allocations
//!
//! Threading:
//! - [`Lattice`] is intentionally not `Send`/`Sync`. It uses reusable mutable
//!   buffers that are not safe to share across threads.

mod api;
mod builder;
mod scoring;
mod search;
mod stats;
mod types;

pub use stats::IndexStats;
pub use types::Lattice;

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_types::DocId;

    #[test]
    fn basic_add_and_search() {
        let mut engine = Lattice::new();

        let id1 = engine.add("hello world").expect("should add doc");
        let id2 = engine.add("hello rust").expect("should add doc");
        let id3 = engine.add("goodbye world").expect("should add doc");

        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(id3, 2);

        let results = engine.search("hello", 10);
        assert_eq!(results.len(), 2);

        let results = engine.search("world", 10);
        assert_eq!(results.len(), 2);

        let results = engine.search("hello world", 10);
        assert!(!results.is_empty(), "Should find at least document 0");
        assert!(
            results.iter().any(|r| r.doc_id == 0),
            "Should find document 0"
        );
    }

    #[test]
    fn fuzzy_search() {
        let mut engine = Lattice::new();
        engine.add("hello world").expect("should add doc");
        engine.add("hallo werld").expect("should add doc");
        engine.add("helo wrld").expect("should add doc");
        let results = engine.search("hello world", 10);
        assert!(!results.is_empty());
    }

    #[test]
    fn empty_query() {
        let mut engine = Lattice::new();
        engine.add("hello world").expect("should add doc");
        assert!(engine.search("", 10).is_empty());
        assert!(engine.search("a", 10).is_empty());
    }

    #[test]
    fn large_scale() {
        let mut engine = Lattice::new();
        for i in 0..1000 {
            engine
                .add(&format!("document number {}", i))
                .expect("should add doc");
        }
        assert!(!engine.search("document", 10).is_empty());
    }

    #[test]
    fn arena_storage() {
        let mut engine = Lattice::new();
        engine.add("first document").expect("should add doc");
        engine.add("second document").expect("should add doc");
        engine.add("third document").expect("should add doc");
        assert_eq!(engine.get(0), Some("first document"));
        assert_eq!(engine.get(1), Some("second document"));
        assert_eq!(engine.get(2), Some("third document"));
        assert_eq!(engine.get(3), None);
    }

    #[test]
    fn doc_lengths_cached() {
        let mut engine = Lattice::new();
        engine.add("hello").expect("should add doc");
        engine.add("hello world").expect("should add doc");
        assert_eq!(engine.doc_lengths.get(0).copied(), Some(5));
        assert_eq!(engine.doc_lengths.get(1).copied(), Some(11));
    }

    #[test]
    fn clear_resets() {
        let mut engine = Lattice::new();
        engine.add("test").expect("should add doc");
        engine.add("document").expect("should add doc");
        assert_eq!(engine.len(), 2);

        engine.clear();

        assert_eq!(engine.len(), 0);
        assert!(engine.is_empty());
        assert!(engine.search("test", 10).is_empty());
        assert!(engine.doc_lengths.is_empty());
    }

    #[test]
    fn merge_intersect_basic() {
        let mut engine = Lattice::new();
        engine.add("hello world foo").expect("should add doc");
        engine.add("hello world bar").expect("should add doc");
        engine.add("hello baz foo").expect("should add doc");
        engine.add("other text here").expect("should add doc");

        let results = engine.search("hello world", 10);
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|r| r.doc_id == 0));
        assert!(results.iter().any(|r| r.doc_id == 1));
    }

    #[test]
    fn soft_merge_does_not_drop_partial_matches() {
        use lattice_types::SearchConfig;
        let mut permissive = Lattice::with_config(SearchConfig {
            min_overlap_ratio: 0.1,
            ..Default::default()
        });
        permissive.add("hello world").expect("should add doc");
        permissive
            .add("hello there friend")
            .expect("should add doc");

        let results = permissive.search("hello world", 10);
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.doc_id == 0));
    }

    #[test]
    fn posting_lists_sorted() {
        let mut engine = Lattice::new();
        for i in 0..100 {
            engine
                .add(&format!("document {}", i))
                .expect("should add doc");
        }
        let _ = engine.search("test", 1);

        for block in &engine.blocks {
            let postings = Lattice::block_postings(block, &engine.postings);
            for w in postings.windows(2) {
                assert!(w[0] < w[1], "Posting list must be strictly sorted");
            }
        }
    }

    #[test]
    fn blocks_sorted_by_trigram() {
        use lattice_types::Trigram;
        let mut engine = Lattice::new();
        engine.add("abc").expect("should add doc");
        engine.add("abd").expect("should add doc");
        engine.add("xyz").expect("should add doc");
        let _ = engine.search("test", 1);

        for i in 1..engine.blocks.len() {
            assert!(
                engine.blocks[i - 1].trigram.0 < engine.blocks[i].trigram.0,
                "Blocks must be sorted by trigram"
            );
        }

        // Verify posting lists exist for actual trigrams
        let abc_idx = engine.find_block(Trigram::from_str("abc"));
        assert!(abc_idx.is_some());
        let abc_block = &engine.blocks[abc_idx.unwrap()];
        let abc_postings = Lattice::block_postings(abc_block, &engine.postings);
        assert!(!abc_postings.is_empty());

        // Verify no posting list for non-existent trigram
        let zzz_idx = engine.find_block(Trigram::from_str("zzz"));
        assert!(zzz_idx.is_none());
    }

    #[test]
    fn incremental_indexing() {
        let mut engine = Lattice::new();
        for i in 0..10 {
            engine
                .add(&format!("document {}", i))
                .expect("should add doc");
        }
        assert_eq!(engine.search("document", 10).len(), 10);

        for i in 10..20 {
            engine
                .add(&format!("document {}", i))
                .expect("should add doc");
        }
        assert_eq!(engine.search("document", 20).len(), 20);
    }

    #[test]
    fn incremental_indexing_correctness() {
        let mut incremental = Lattice::new();
        for i in 0..5 {
            incremental
                .add(&format!("word{} doc", i))
                .expect("should add doc");
        }
        let _ = incremental.search("word", 1);
        for i in 5..10 {
            incremental
                .add(&format!("word{} doc", i))
                .expect("should add doc");
        }
        let inc = incremental.search("doc", 20);

        let mut fresh = Lattice::new();
        for i in 0..10 {
            fresh
                .add(&format!("word{} doc", i))
                .expect("should add doc");
        }
        let fr = fresh.search("doc", 20);

        assert_eq!(inc.len(), fr.len());
    }

    #[test]
    fn radix_sort_correctness() {
        use crate::index::types::TempTrigramEntry;
        use crate::index::types::RADIX_SORT_THRESHOLD;
        use lattice_types::Trigram;
        let n = RADIX_SORT_THRESHOLD * 4;
        let mut entries: Vec<TempTrigramEntry> = (0..n as u32)
            .map(|i| TempTrigramEntry {
                trigram: Trigram(((i.wrapping_mul(7919)) % 0x00FF_FFFF) as u32),
                doc_id: n as u32 - 1 - i,
            })
            .collect();

        let mut reference = entries.clone();
        reference.sort_unstable_by(|a, b| {
            a.trigram
                .0
                .cmp(&b.trigram.0)
                .then_with(|| a.doc_id.cmp(&b.doc_id))
        });

        Lattice::sort_trigrams(&mut entries);

        for (i, (got, want)) in entries.iter().zip(reference.iter()).enumerate() {
            assert_eq!(
                (got.trigram.0, got.doc_id),
                (want.trigram.0, want.doc_id),
                "Mismatch at index {i}"
            );
        }
    }

    #[test]
    fn sort_small_input_correctness() {
        use crate::index::types::TempTrigramEntry;
        use lattice_types::Trigram;
        let mut entries: Vec<TempTrigramEntry> = (0..10u32)
            .map(|i| TempTrigramEntry {
                trigram: Trigram(10 - i),
                doc_id: i,
            })
            .collect();

        Lattice::sort_trigrams(&mut entries);

        for w in entries.windows(2) {
            assert!(
                w[0].trigram.0 < w[1].trigram.0
                    || (w[0].trigram.0 == w[1].trigram.0 && w[0].doc_id <= w[1].doc_id),
                "Small input not sorted correctly"
            );
        }
    }

    #[test]
    fn compression_saves_space() {
        let mut engine = Lattice::new();
        for i in 0..100 {
            engine
                .add(&format!("word{} document {}", i % 10, i))
                .expect("should add doc");
        }
        let _ = engine.search("test", 1);

        let (compressed_bytes, ratio) = engine.compress_postings();
        let original = engine.postings.len() * std::mem::size_of::<DocId>();
        assert!(compressed_bytes < original);
        assert!(ratio < 1.0 && ratio > 0.0);

        let stats = engine.stats_with_compression();
        assert!(stats.compressed_postings_bytes.is_some());
        assert!(stats.compression_ratio.is_some());
        assert!(stats.memory_usage_bytes() > 0);
        assert!(format!("{stats}").contains("compressed"));
    }

    #[test]
    fn compression_empty_index() {
        let engine = Lattice::new();
        let (bytes, ratio) = engine.compress_postings();
        assert_eq!(bytes, 0);
        assert_eq!(ratio, 1.0);
    }

    #[test]
    fn memory_usage_bytes_matches_sizes() {
        let mut engine = Lattice::new();
        for i in 0..10 {
            engine
                .add(&format!("document {}", i))
                .expect("should add doc");
        }
        let _ = engine.search("doc", 1);

        let stats = engine.stats();
        let expected = engine.blocks.len() * 12 + engine.postings.len() * 4;
        assert_eq!(stats.memory_usage_bytes(), expected);
    }

    #[test]
    fn add_batch_works() {
        let mut engine = Lattice::new();
        let docs = ["hello world", "rust programming", "fuzzy search"];
        let (added, failed, err) = engine.add_batch(&docs);
        assert_eq!(added, 3);
        assert_eq!(failed, 0);
        assert!(err.is_none());
        assert_eq!(engine.len(), 3);

        let results = engine.search("hello", 10);
        assert!(!results.is_empty());
    }

    #[test]
    fn rejects_oversized_documents() {
        use lattice_types::DocumentError;
        let mut engine = Lattice::new();
        let oversized = "x".repeat(65536); // Just over 64KB limit
        let result = engine.add(&oversized);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DocumentError::TooLarge { .. }
        ));

        let exact_size = "x".repeat(65535); // Exactly at limit
        assert!(engine.add(&exact_size).is_ok());
    }

    #[test]
    fn rejects_control_characters() {
        use lattice_types::DocumentError;
        let mut engine = Lattice::new();

        // Null byte
        let result = engine.add("hello\x00world");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DocumentError::InvalidInput { .. }
        ));

        // Bell character
        let result = engine.add("hello\x07world");
        assert!(result.is_err());

        // DEL character
        let result = engine.add("hello\x7fworld");
        assert!(result.is_err());

        // Valid: whitespace is allowed
        let result = engine.add("hello world\t\n");
        assert!(result.is_ok());
    }

    #[test]
    fn metrics_tracks_operations() {
        let mut engine = Lattice::new();

        // Initially zero
        let metrics = engine.metrics();
        assert_eq!(metrics.documents_indexed, 0);
        assert_eq!(metrics.queries_executed, 0);
        assert_eq!(metrics.current_doc_count, 0);

        // Add some documents
        engine.add("doc one").unwrap();
        engine.add("doc two").unwrap();
        engine.add("doc three").unwrap();

        let metrics = engine.metrics();
        assert_eq!(metrics.documents_indexed, 3);
        assert_eq!(metrics.current_doc_count, 3);

        // Execute some queries
        engine.search("doc", 10);
        engine.search("one", 10);
        engine.search("two", 10);

        let metrics = engine.metrics();
        assert_eq!(metrics.queries_executed, 3);
        assert_eq!(metrics.current_doc_count, 3);

        // Clear resets current count but keeps totals
        engine.clear();
        let metrics = engine.metrics();
        assert_eq!(metrics.documents_indexed, 0);
        assert_eq!(metrics.queries_executed, 0);
        assert_eq!(metrics.current_doc_count, 0);
    }
}
