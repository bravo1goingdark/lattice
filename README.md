# lattice

> A lightweight, high-performance fuzzy search engine in Rust using trigram indexing and inverted indexes.

lattice is a fast, embeddable fuzzy search library designed for typo-tolerant search, autocomplete, and low-latency retrieval in backend systems.

---

##  Features

-  Fast trigram-based indexing
-  Fuzzy matching without full edit-distance scans
-  Lightweight and dependency-minimal
-  Easy to embed in any Rust application
-  Designed for high-performance systems

---

##  Example

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
