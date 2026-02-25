//! Text analysis pipeline.
//!
//! This module provides the text processing components:
//! - **Normalizer**: Cleans and normalizes raw text
//! - **Tokenizer**: Splits normalized text into tokens
//! - **Trigram**: Extracts 3-character sequences for indexing

pub mod normalizer;
pub mod tokenizer;
pub mod trigram;

pub use normalizer::TextNormalizer;
pub use tokenizer::{Field, Tokenizer};
pub use trigram::TrigramExtractor;
