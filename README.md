# Lattice

> A lightweight, high-performance fuzzy search engine in Rust using trigram indexing and inverted indexes.


---

## Features

| Feature | What it does |
|---------|-------------|
| Trigram indexing | Indexes text by 3-character sequences for fast fuzzy matching |
| ASCII fast-path | Skips Unicode overhead when input is plain ASCII |
| Unicode support | Handles accented characters, normalizes text consistently |
| Smart preprocessing | Optionally strips accents and cleans up extra spaces |
| Field weights | Give titles more importance than body text |
| Positional scoring | Matches at the start of words score higher |
| Efficient reranking | Expensive edit distance only runs on best candidates |
| Lightweight | Few dependencies, compiles quickly

---

## Quick Start

```rust
use lattice_core::Lattice;

fn main() {
    let mut engine = Lattice::new();

    engine.add(1, "hello world");
    engine.add(2, "hallo werld");
    engine.add(3, "rust programming");

    let results = engine.search("helo world", 5);

    for r in results {
        println!("doc={} score={}", r.doc_id, r.score);
    }
}
```

---

## Architecture Overview

**Preprocessing** — Normalization, tokenization, and n-gram generation:
- Normalization: ASCII fast-path (direct lowercase) or Unicode NFC cold-path → lowercase folding → optional diacritic stripping and whitespace collapsing
- Tokenization: Whitespace split → field tagging → weight assignment
- N-gram generation: Boundary padding (optional) → sliding window → trigram extraction

**Indexing** — Batch insertions update the document store and create posting lists that populate the inverted index.

**Retrieval** — Query trigrams lookup posting lists, then candidate retrieval intersects lists and filters by overlap threshold. Scoring applies trigram overlap, positional boost, and field weight multiplication. Reranking (optional) runs edit distance and Jaro-Winkler on top-K candidates.

Documents and queries share the same preprocessing pipeline.

![Architecture](lattice.png)

---

## Configuration

```rust
use lattice_core::NormalizationConfig;

let config = NormalizationConfig {
    strip_diacritics: true,      // "café" → "cafe"
    collapse_whitespaces: true,  // "a  b" → "a b"
};
```

Both default to `true`.

---


