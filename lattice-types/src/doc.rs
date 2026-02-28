//! Document-related types and errors.

use core::fmt;

/// Unique document identifier.
///
/// Documents are identified by a 32-bit unsigned integer.
/// With u32::MAX (~4 billion) documents, this provides sufficient
/// capacity for most use cases while keeping memory overhead low.
pub type DocId = u32;

/// Errors that can occur when adding a document to the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentError {
    /// Document exceeds the maximum allowed size (64KB).
    TooLarge {
        /// The actual size of the document in bytes.
        size: usize,
        /// The maximum allowed size in bytes.
        max_size: usize,
    },
    /// Document is too short to extract trigrams (minimum 3 characters).
    TooShort {
        /// The actual length of the document.
        length: usize,
        /// The minimum required length.
        min_length: usize,
    },
    /// Document contains invalid control characters.
    InvalidInput {
        /// Description of the invalid content.
        reason: &'static str,
    },
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentError::TooLarge { size, max_size } => {
                write!(
                    f,
                    "document too large: {} bytes (max: {} bytes)",
                    size, max_size
                )
            }
            DocumentError::TooShort { length, min_length } => {
                write!(
                    f,
                    "document too short: {} bytes (min: {} bytes)",
                    length, min_length
                )
            }
            DocumentError::InvalidInput { reason } => {
                write!(f, "document contains invalid input: {}", reason)
            }
        }
    }
}

impl core::error::Error for DocumentError {}
