# Lattice

> A lightweight, production-grade fuzzy search engine in Rust using trigram indexing.

---

## Features

| Feature | What it does |
|---------|-------------|
| **Trigram indexing** | Indexes text by 3-character sequences for fast fuzzy matching |
| **ASCII-optimized** | SIMD-accelerated normalizer (~4-8 GiB/s on x86_64) |
| **Typo tolerance** | Finds matches despite spelling errors via trigram overlap |
| **Zero-allocation search** | Stack-allocated buffers for queries and results |
| **Fast hash maps** | `FxHashMap` for trigram → posting list lookups |
| **Smallvec optimization** | Avoids heap allocation for common cases |
| **Memory efficient** | Compact inverted index, reusable query buffers |
| **Minimal dependencies** | `memchr`, `rustc-hash`, `smallvec` only |

---

## ⚠️ Production Disclaimer: ASCII Only

The `TextNormalizer` is optimized for **ASCII text processing**:
- Non-ASCII bytes (≥0x80) pass through unchanged
- No Unicode lowercasing or diacritic stripping
- Designed for English/ASCII-heavy content

For search engines indexing primarily non-Unicode content, this provides maximum throughput. Unicode content will be indexed as-is without normalization.

---

## Quick Start

```rust
use lattice_core::Lattice;

fn main() {
    let mut engine = Lattice::new();

    // Index documents
    engine.add(1, "hello world");
    engine.add(2, "hallo werld");
    engine.add(3, "rust programming");

    // Search with typo tolerance
    let results = engine.search("helo world", 5);

    // Process results
    for r in results {
        // r.doc_id: u32, r.score: f32
    }
}
```

---

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
lattice-core = { path = "lattice-core" }
```

---

## Architecture

```
┌─────────────────┐     ┌─────────────┐     ┌─────────────────┐
│  Input Text     │────▶│ Normalizer  │────▶│  Tokenizer      │
│  (ASCII/UTF-8)  │     │ (SIMD)      │     │  (zero-alloc)   │
└─────────────────┘     └─────────────┘     └─────────────────┘
                                                      │
                                                      ▼
┌─────────────────┐     ┌─────────────┐     ┌─────────────────┐
│  Search Results │◀────│   Scorer    │◀────│ Trigram Extract │
│  (ranked)       │     │             │     │                 │
└─────────────────┘     └─────────────┘     └─────────────────┘
                              │
                              ▼
┌─────────────────┐     ┌─────────────┐
│  Inverted Index │◀────│ Posting     │
│  (FxHashMap)    │     │ Lists       │
└─────────────────┘     └─────────────┘
```

---

## Performance

### Text Normalization

| Input Type | Throughput | Implementation |
|------------|-----------|----------------|
| Pure ASCII | ~4-8 GiB/s | AVX2 (32B) / SSE2 (16B) |
| Mixed ASCII | ~2-4 GiB/s | Hybrid SIMD + scalar |
| Non-ASCII | ~1-2 GiB/s | Scalar pass-through |

### Search Performance

Typical throughput on modern x86_64:
- **Indexing**: ~1.5M docs/sec (short documents)
- **Querying**: ~200 queries/sec (10K document index)
- **Memory**: ~17 bytes per trigram occurrence

---

## Project Structure

```
lattice/
├── Cargo.toml                 # Workspace configuration
├── lattice-types/             # Core types (DocId, Trigram, SearchResult)
│   └── src/lib.rs
├── lattice-core/              # Search engine library
│   └── src/
│       ├── lib.rs
│       ├── analyzer/
│       │   ├── normalizer.rs  # SIMD ASCII normalizer
│       │   ├── tokenizer.rs   # Zero-alloc tokenization
│       │   └── trigram.rs     # Trigram extraction
│       ├── index/
│       │   └── mod.rs         # Lattice search engine
│       ├── search/
│       │   └── mod.rs         # Edit distance, Jaro-Winkler
│       └── bin/
│           └── wiki_bench.rs  # Benchmarking tool
├── lattice-demo/              # Demo application
│   └── src/main.rs
└── README.md                  # This file
```

---

## API Reference

### `Lattice` - Main Search Engine

```rust
use lattice_core::Lattice;
use lattice_types::SearchConfig;

// Create with default config
let mut engine = Lattice::new();

// Or with custom search config
let config = SearchConfig::fuzzy();  // or SearchConfig::exact()
let mut engine = Lattice::with_config(config);

// Add documents
engine.add(1, "hello world");

// Search (returns SmallVec for stack efficiency)
let results = engine.search("helo wrld", 10);

// Get document content
if let Some(content) = engine.get_document(1) {
    // content is &str
}

// Statistics
let stats = engine.stats();  // documents, trigrams, postings
```

### `TextNormalizer` - SIMD ASCII Normalizer

```rust
use lattice_core::analyzer::normalizer::TextNormalizer;

let normalizer = TextNormalizer::new();

// Normalize to new String
let result = normalizer.normalize("HELLO   WORLD");  // "hello world"

// Or reuse buffer (zero-allocation path)
let mut buf = String::with_capacity(256);
normalizer.normalize_into("HELLO   WORLD", &mut buf);
```

### `Tokenizer` - Zero-Allocation Tokenizer

```rust
use lattice_core::analyzer::tokenizer::{Tokenizer, Field};

let tokenizer = Tokenizer::new(Field::Body);

// Callback-based (no allocations)
tokenizer.tokenize("hello world", |text, field, position| {
    // text: &str, field: Field, position: u32
});
```

**Field Weights:**
| Field | Weight | Use Case |
|-------|--------|----------|
| `Title` | 3.0x | Document titles |
| `Tag` | 2.0x | Tags, categories |
| `Body` | 1.0x | Main content |

---

## Configuration

### Search Configuration

```rust
use lattice_types::SearchConfig;

// Fuzzy matching (default)
let fuzzy = SearchConfig::fuzzy();
// min_overlap_ratio: 0.2
// enable_fuzzy: true
// max_edit_distance: 2

// Exact matching
let exact = SearchConfig::exact();
// min_overlap_ratio: 0.5
// enable_fuzzy: false
// max_edit_distance: 0
```

---

## Implementation Details

### Memory Efficiency

- **Query trigrams**: `SmallVec<[Trigram; 32]>` - stack allocated for typical queries
- **Results**: `SmallVec<[SearchResult; 64]>` - no heap allocation for common result sets
- **Posting lists**: `SmallVec<[DocId; 4]>` - rare trigrams stay on stack
- **Scoring**: Linear search in SmallVec instead of HashMap (faster for n<64)
- **Query buffer**: Reusable `String` in `Lattice` struct amortizes allocations

### SIMD Normalization

The ASCII normalizer uses a three-tier approach on x86_64:

1. **AVX2** (32 bytes at a time): Check high bit with `_mm256_movemask_epi8`, process if all ASCII
2. **SSE2** (16 bytes at a time): Same approach with 128-bit registers
3. **Scalar**: Process remaining bytes, non-ASCII pass through unchanged

---

## Testing

```bash
# Run all tests
cargo test

# Run with release optimizations
cargo test --release

# Run demo
cargo run --release -p lattice-demo
```

---

## License

MIT

## Acknowledgments

- Uses [`memchr`](https://docs.rs/memchr) for SIMD-accelerated byte searching
- Uses [`rustc-hash`](https://docs.rs/rustc-hash) for fast hash maps
- Uses [`smallvec`](https://docs.rs/smallvec) for stack-allocated collections
- Inspired by trigram indexing techniques used in PostgreSQL's `pg_trgm`
