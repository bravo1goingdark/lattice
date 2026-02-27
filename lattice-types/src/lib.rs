//! Core types and traits for the Lattice search engine.
//!
//! This crate provides the fundamental types that are shared across
//! the Lattice ecosystem. Keeping types separate ensures:
//!
//! - **Zero-cost abstractions**: Types are sized for cache efficiency
//! - **Cross-crate compatibility**: Core and CLI share the same types
//! - **Clean boundaries**: No circular dependencies between crates

#![warn(missing_docs)]

use core::fmt;

/// Unique document identifier.
///
/// Documents are identified by a 32-bit unsigned integer.
/// With u32::MAX (~4 billion) documents, this provides sufficient
/// capacity for most use cases while keeping memory overhead low.
pub type DocId = u32;

/// Search result containing a document ID and relevance score.
///
/// Results are ordered by score (descending), then by doc_id (ascending).
/// Higher scores indicate better matches.
#[derive(Debug, Clone, Copy)]
pub struct SearchResult {
    /// Document identifier
    pub doc_id: DocId,
    /// Relevance score (higher is better)
    pub score: f32,
}

impl PartialEq for SearchResult {
    fn eq(&self, other: &Self) -> bool {
        // Two results are equal if both doc_id AND score are equal
        self.doc_id == other.doc_id && self.score == other.score
    }
}

impl Eq for SearchResult {}

impl PartialOrd for SearchResult {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchResult {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Primary: score (higher = greater for intuitive comparison)
        // Secondary: doc_id (for deterministic ordering when scores are equal)
        match self.score.total_cmp(&other.score) {
            core::cmp::Ordering::Equal => self.doc_id.cmp(&other.doc_id),
            ord => ord,
        }
    }
}

impl SearchResult {
    /// Creates a new search result.
    #[inline(always)]
    pub const fn new(doc_id: DocId, score: f32) -> Self {
        Self { doc_id, score }
    }
}

impl fmt::Display for SearchResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "doc={} score={:.3}", self.doc_id, self.score)
    }
}

/// A trigram (3-character sequence) represented as a 24-bit integer.
///
/// Trigrams are packed as: `(b0 << 16) | (b1 << 8) | b2`
/// This representation:
/// - Fits in 3 bytes (u24 would be ideal, but u32 is used)
/// - Enables fast equality comparison
/// - Works as a hash map key without allocation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Trigram(pub u32);

impl Trigram {
    /// Maximum possible trigram value (0xFFFFFF).
    pub const MAX: u32 = 0xFFFFFF;

    /// Creates a trigram from three bytes.
    #[inline(always)]
    pub const fn from_bytes(b0: u8, b1: u8, b2: u8) -> Self {
        Self(((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32))
    }

    /// Creates a trigram from a string slice.
    /// Panics if the slice is shorter than 3 bytes.
    #[inline(always)]
    pub fn from_str(s: &str) -> Self {
        let bytes = s.as_bytes();
        debug_assert!(bytes.len() >= 3, "trigram requires at least 3 bytes");
        Self::from_bytes(bytes[0], bytes[1], bytes[2])
    }

    /// Returns the three bytes of this trigram.
    #[inline(always)]
    pub const fn to_bytes(self) -> [u8; 3] {
        [
            ((self.0 >> 16) & 0xFF) as u8,
            ((self.0 >> 8) & 0xFF) as u8,
            (self.0 & 0xFF) as u8,
        ]
    }

    /// Returns the underlying u32 value.
    #[inline(always)]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl From<u32> for Trigram {
    #[inline(always)]
    fn from(value: u32) -> Self {
        Self(value & Self::MAX)
    }
}

impl From<Trigram> for u32 {
    #[inline(always)]
    fn from(t: Trigram) -> Self {
        t.0
    }
}

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

/// Search configuration options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchConfig {
    /// Minimum trigram overlap ratio for a document to be considered (0.0-1.0).
    /// Default: 0.3 (30% of query trigrams must match)
    pub min_overlap_ratio: f32,
    /// Whether to enable fuzzy reranking with edit distance.
    pub enable_fuzzy: bool,
    /// Maximum edit distance for fuzzy matching (0 = exact only).
    pub max_edit_distance: u8,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            min_overlap_ratio: 0.3,
            enable_fuzzy: true,
            max_edit_distance: 2,
        }
    }
}

impl SearchConfig {
    /// Creates a configuration for exact matching only (no fuzziness).
    pub const fn exact() -> Self {
        Self {
            min_overlap_ratio: 0.5,
            enable_fuzzy: false,
            max_edit_distance: 0,
        }
    }

    /// Creates a configuration for fuzzy matching.
    pub const fn fuzzy() -> Self {
        Self {
            min_overlap_ratio: 0.2,
            enable_fuzzy: true,
            max_edit_distance: 2,
        }
    }
}

/// Compression utilities for integer sequences.
///
/// Provides delta encoding and variable-length integer compression
/// optimized for sorted sequences like document ID lists.
pub mod compression {
    /// Error type for compression/decompression operations.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CompressionError {
        /// Input buffer was too small for the operation.
        BufferTooSmall,
        /// Output buffer was too small for the result.
        OutputTooSmall,
        /// Invalid varint encoding encountered.
        InvalidVarint,
        /// Input sequence was not sorted (required for delta encoding).
        NotSorted,
    }

    impl core::fmt::Display for CompressionError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self {
                CompressionError::BufferTooSmall => write!(f, "input buffer too small"),
                CompressionError::OutputTooSmall => write!(f, "output buffer too small"),
                CompressionError::InvalidVarint => write!(f, "invalid varint encoding"),
                CompressionError::NotSorted => write!(f, "input sequence not sorted"),
            }
        }
    }

    /// Encodes a sorted sequence of u32 values using delta encoding.
    ///
    /// Delta encoding stores the difference between consecutive values rather
    /// than the absolute values. For sorted sequences, these deltas are much
    /// smaller, enabling better compression with varint.
    ///
    /// # Example
    /// ```
    /// use lattice_types::compression::delta_encode;
    ///
    /// let input = vec![100u32, 105, 110, 115];
    /// let mut deltas = Vec::new();
    /// delta_encode(&input, &mut deltas).unwrap();
    /// // deltas: [100, 5, 5, 5]
    /// ```
    ///
    /// # Errors
    /// Returns `CompressionError::NotSorted` if the input is not sorted in ascending order.
    pub fn delta_encode(input: &[u32], output: &mut Vec<u32>) -> Result<(), CompressionError> {
        if input.is_empty() {
            return Ok(());
        }

        // Verify input is sorted
        for i in 1..input.len() {
            if input[i] < input[i - 1] {
                return Err(CompressionError::NotSorted);
            }
        }

        output.clear();
        output.reserve(input.len());

        // First value is stored as-is (base)
        output.push(input[0]);

        // Subsequent values are deltas from previous
        for i in 1..input.len() {
            output.push(input[i] - input[i - 1]);
        }

        Ok(())
    }

    /// Decodes a delta-encoded sequence back to absolute values.
    ///
    /// Reconstructs the original sorted sequence from delta-encoded data.
    ///
    /// # Example
    /// ```
    /// use lattice_types::compression::delta_decode;
    ///
    /// let deltas = vec![100u32, 5, 5, 5];
    /// let mut output = Vec::new();
    /// delta_decode(&deltas, &mut output).unwrap();
    /// // output: [100, 105, 110, 115]
    /// ```
    pub fn delta_decode(input: &[u32], output: &mut Vec<u32>) -> Result<(), CompressionError> {
        if input.is_empty() {
            return Ok(());
        }

        output.clear();
        output.reserve(input.len());

        // First value is the base
        output.push(input[0]);

        // Reconstruct by accumulating deltas
        for i in 1..input.len() {
            let prev = output[i - 1];
            output.push(prev + input[i]);
        }

        Ok(())
    }

    /// Encodes a u32 value as a variable-length integer (varint).
    ///
    /// Uses Protocol Buffers varint encoding where 7 bits of data are stored
    /// per byte, with the MSB indicating continuation.
    ///
    /// # Encoding
    /// - Small values (0-127): 1 byte
    /// - Medium values (128-16383): 2 bytes
    /// - Large values: up to 5 bytes
    ///
    /// # Example
    /// ```
    /// use lattice_types::compression::encode_varint;
    ///
    /// let mut buf = [0u8; 5];
    /// let len = encode_varint(150u32, &mut buf);
    /// assert_eq!(&buf[..len], &[0x96, 0x01]);
    /// ```
    pub fn encode_varint(mut value: u32, buf: &mut [u8]) -> usize {
        let mut i = 0;

        while value >= 0x80 {
            buf[i] = (value as u8) | 0x80;
            value >>= 7;
            i += 1;
        }

        buf[i] = value as u8;
        i + 1
    }

    /// Decodes a varint from a byte buffer.
    ///
    /// Returns the decoded value and the number of bytes consumed.
    /// Returns an error if the buffer is too small or the varint is malformed.
    ///
    /// # Example
    /// ```
    /// use lattice_types::compression::decode_varint;
    ///
    /// let buf = [0x96, 0x01];
    /// let (value, bytes_read) = decode_varint(&buf).unwrap();
    /// assert_eq!(value, 150);
    /// assert_eq!(bytes_read, 2);
    /// ```
    pub fn decode_varint(buf: &[u8]) -> Result<(u32, usize), CompressionError> {
        let mut result: u32 = 0;
        let mut shift = 0;
        let mut i = 0;

        while i < buf.len() {
            let byte = buf[i];
            i += 1;

            // Extract 7 data bits
            let value = (byte & 0x7F) as u32;

            // Check for overflow
            if shift >= 32 {
                return Err(CompressionError::InvalidVarint);
            }

            result |= value << shift;

            // Check continuation bit
            if byte & 0x80 == 0 {
                return Ok((result, i));
            }

            shift += 7;
        }

        Err(CompressionError::BufferTooSmall)
    }

    /// Compresses a sorted sequence of u32 values using delta + varint encoding.
    ///
    /// This combines delta encoding (which makes values small) with varint
    /// encoding (which makes small values compact).
    ///
    /// # Example
    /// ```
    /// use lattice_types::compression::compress_sorted;
    ///
    /// let input = vec![100u32, 105, 110, 115];
    /// let mut output = Vec::new();
    /// let bytes_written = compress_sorted(&input, &mut output).unwrap();
    /// // Typically uses ~5 bytes instead of 16 bytes for raw u32 array
    /// ```
    pub fn compress_sorted(input: &[u32], output: &mut Vec<u8>) -> Result<usize, CompressionError> {
        if input.is_empty() {
            return Ok(0);
        }

        // Apply delta encoding
        let mut deltas = Vec::with_capacity(input.len());
        delta_encode(input, &mut deltas)?;

        // Estimate output size and reserve capacity
        output.clear();
        output.reserve(input.len() * 5); // Worst case: 5 bytes per value

        // Encode each delta as varint
        let mut buf = [0u8; 5];
        for &delta in &deltas {
            let len = encode_varint(delta, &mut buf);
            output.extend_from_slice(&buf[..len]);
        }

        Ok(output.len())
    }

    /// Decompresses a sequence encoded with `compress_sorted`.
    ///
    /// # Example
    /// ```
    /// use lattice_types::compression::{compress_sorted, decompress_sorted};
    ///
    /// let input = vec![100u32, 105, 110, 115];
    /// let mut compressed = Vec::new();
    /// compress_sorted(&input, &mut compressed).unwrap();
    ///
    /// let mut output = Vec::new();
    /// decompress_sorted(&compressed, &mut output).unwrap();
    /// assert_eq!(input, output);
    /// ```
    pub fn decompress_sorted(input: &[u8], output: &mut Vec<u32>) -> Result<(), CompressionError> {
        if input.is_empty() {
            return Ok(());
        }

        // Decode varints to get deltas
        let mut deltas = Vec::new();
        let mut i = 0;

        while i < input.len() {
            let (value, bytes_read) = decode_varint(&input[i..])?;
            deltas.push(value);
            i += bytes_read;
        }

        // Apply delta decoding
        delta_decode(&deltas, output)?;

        Ok(())
    }

    /// Returns the maximum bytes needed to encode a u32 as varint.
    pub const fn max_varint_len() -> usize {
        5 // u32::MAX requires 5 bytes in varint encoding
    }

    /// Estimates the compressed size of a sorted sequence.
    ///
    /// This is a rough estimate based on average delta size.
    /// Actual size depends on the data distribution.
    pub fn estimate_compressed_size(values: &[u32]) -> usize {
        if values.len() <= 1 {
            return values.len() * max_varint_len();
        }

        // Calculate average gap between consecutive values
        let total_gap: u64 = values.windows(2).map(|w| (w[1] - w[0]) as u64).sum();
        let avg_gap = total_gap / (values.len() - 1) as u64;

        // Estimate bytes per value based on average gap
        let bytes_per_value = if avg_gap < 128 {
            1
        } else if avg_gap < 16384 {
            2
        } else {
            3
        };

        // First value is always 5 bytes (worst case)
        5 + (values.len() - 1) * bytes_per_value
    }
}

#[cfg(test)]
mod tests {
    use super::compression::*;
    use super::*;

    #[test]
    fn search_result_ordering() {
        let r1 = SearchResult::new(1, 0.9);
        let r2 = SearchResult::new(2, 0.5);
        let r3 = SearchResult::new(3, 0.9); // Same score as r1

        assert!(r1 > r2); // Higher score is "greater"
        assert_ne!(r1, r3); // Different doc_id = not equal

        // When scores are equal, doc_id breaks the tie
        assert_eq!(r1.cmp(&r3), core::cmp::Ordering::Less); // doc 1 < doc 3
    }

    #[test]
    fn trigram_from_bytes() {
        let t = Trigram::from_bytes(b'a', b'b', b'c');
        assert_eq!(t.as_u32(), 0x00616263);
        assert_eq!(t.to_bytes(), [b'a', b'b', b'c']);
    }

    #[test]
    fn trigram_from_str() {
        let t = Trigram::from_str("abc");
        assert_eq!(t.as_u32(), 0x00616263);
    }

    // Delta encoding tests
    #[test]
    fn delta_encode_basic() {
        let input = vec![100u32, 105, 110, 115];
        let mut output = Vec::new();
        delta_encode(&input, &mut output).unwrap();
        assert_eq!(output, vec![100, 5, 5, 5]);
    }

    #[test]
    fn delta_encode_with_duplicates() {
        let input = vec![1u32, 1, 2, 2, 3];
        let mut output = Vec::new();
        delta_encode(&input, &mut output).unwrap();
        assert_eq!(output, vec![1, 0, 1, 0, 1]);
    }

    #[test]
    fn delta_encode_empty() {
        let input: Vec<u32> = vec![];
        let mut output = Vec::new();
        delta_encode(&input, &mut output).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn delta_encode_single() {
        let input = vec![42u32];
        let mut output = Vec::new();
        delta_encode(&input, &mut output).unwrap();
        assert_eq!(output, vec![42]);
    }

    #[test]
    fn delta_encode_not_sorted() {
        let input = vec![10u32, 5, 15];
        let mut output = Vec::new();
        assert_eq!(
            delta_encode(&input, &mut output),
            Err(CompressionError::NotSorted)
        );
    }

    #[test]
    fn delta_decode_basic() {
        let input = vec![100u32, 5, 5, 5];
        let mut output = Vec::new();
        delta_decode(&input, &mut output).unwrap();
        assert_eq!(output, vec![100, 105, 110, 115]);
    }

    #[test]
    fn delta_roundtrip() {
        let original = vec![1u32, 2, 5, 10, 20, 50, 100];
        let mut encoded = Vec::new();
        let mut decoded = Vec::new();

        delta_encode(&original, &mut encoded).unwrap();
        delta_decode(&encoded, &mut decoded).unwrap();

        assert_eq!(original, decoded);
    }

    // Varint tests
    #[test]
    fn varint_encode_single_byte() {
        let mut buf = [0u8; 5];
        let len = encode_varint(0, &mut buf);
        assert_eq!(&buf[..len], &[0x00]);

        let len = encode_varint(127, &mut buf);
        assert_eq!(&buf[..len], &[0x7F]);
    }

    #[test]
    fn varint_encode_two_bytes() {
        let mut buf = [0u8; 5];
        let len = encode_varint(128, &mut buf);
        assert_eq!(&buf[..len], &[0x80, 0x01]);

        let len = encode_varint(150, &mut buf);
        assert_eq!(&buf[..len], &[0x96, 0x01]);

        let len = encode_varint(16383, &mut buf);
        assert_eq!(&buf[..len], &[0xFF, 0x7F]);
    }

    #[test]
    fn varint_encode_max_u32() {
        let mut buf = [0u8; 5];
        let len = encode_varint(u32::MAX, &mut buf);
        assert_eq!(&buf[..len], &[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
    }

    #[test]
    fn varint_decode_single_byte() {
        let buf = [0x00];
        let (val, len) = decode_varint(&buf).unwrap();
        assert_eq!(val, 0);
        assert_eq!(len, 1);

        let buf = [0x7F];
        let (val, len) = decode_varint(&buf).unwrap();
        assert_eq!(val, 127);
        assert_eq!(len, 1);
    }

    #[test]
    fn varint_decode_two_bytes() {
        let buf = [0x80, 0x01];
        let (val, len) = decode_varint(&buf).unwrap();
        assert_eq!(val, 128);
        assert_eq!(len, 2);

        let buf = [0x96, 0x01];
        let (val, len) = decode_varint(&buf).unwrap();
        assert_eq!(val, 150);
        assert_eq!(len, 2);
    }

    #[test]
    fn varint_roundtrip() {
        let test_values = [0u32, 1, 127, 128, 150, 16383, 16384, 100000, u32::MAX];
        let mut buf = [0u8; 5];

        for &original in &test_values {
            let len = encode_varint(original, &mut buf);
            let (decoded, decoded_len) = decode_varint(&buf[..len]).unwrap();
            assert_eq!(original, decoded, "Roundtrip failed for value {}", original);
            assert_eq!(len, decoded_len);
        }
    }

    #[test]
    fn varint_decode_incomplete() {
        let buf = [0xFF, 0xFF]; // Continuation bit set but no more data
        assert_eq!(decode_varint(&buf), Err(CompressionError::BufferTooSmall));
    }

    // Combined compression tests
    #[test]
    fn compress_decompress_sorted() {
        let original = vec![100u32, 105, 110, 115, 200, 250, 300];
        let mut compressed = Vec::new();
        let mut decompressed = Vec::new();

        compress_sorted(&original, &mut compressed).unwrap();
        decompress_sorted(&compressed, &mut decompressed).unwrap();

        assert_eq!(original, decompressed);
    }

    #[test]
    fn compress_empty() {
        let original: Vec<u32> = vec![];
        let mut compressed = Vec::new();
        let bytes = compress_sorted(&original, &mut compressed).unwrap();
        assert_eq!(bytes, 0);
    }

    #[test]
    fn compress_not_sorted() {
        let original = vec![10u32, 5, 15];
        let mut compressed = Vec::new();
        assert_eq!(
            compress_sorted(&original, &mut compressed),
            Err(CompressionError::NotSorted)
        );
    }

    #[test]
    fn compression_efficiency() {
        // Sorted sequence with small gaps compresses very well
        let original: Vec<u32> = (0..1000).map(|i| i * 2).collect();
        let mut compressed = Vec::new();

        compress_sorted(&original, &mut compressed).unwrap();

        // Raw size: 1000 * 4 = 4000 bytes
        // Compressed should be much smaller since all deltas are 2 (1 byte each in varint)
        // Plus first value (0-4 bytes) = ~1004 bytes
        let ratio = compressed.len() as f64 / (original.len() * 4) as f64;
        assert!(
            ratio < 0.3,
            "Compression ratio should be < 30%, got {:.1}%",
            ratio * 100.0
        );
    }
}
