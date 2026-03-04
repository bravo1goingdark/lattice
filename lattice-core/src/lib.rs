//! Lattice Core - High-performance fuzzy search engine
//!
//! This crate provides the core indexing and search functionality
//! for the Lattice search engine.

#![warn(missing_docs)]

pub mod analyzer;
pub mod arena;
pub mod index;

pub use analyzer::{Field, TextNormalizer, Tokenizer, TrigramExtractor};
pub use arena::Arena;
pub use index::{EngineMetrics, IndexStats, Lattice};
