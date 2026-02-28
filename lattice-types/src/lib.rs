//! Core types and traits for the Lattice search engine.
//!
//! This crate provides the fundamental types that are shared across
//! the Lattice ecosystem. Keeping types separate ensures:
//!
//! - **Zero-cost abstractions**: Types are sized for cache efficiency
//! - **Cross-crate compatibility**: Core and CLI share the same types
//! - **Clean boundaries**: No circular dependencies between crates

#![warn(missing_docs)]

pub mod compression;
pub mod doc;
pub mod search;
pub mod trigram;

pub use doc::{DocId, DocumentError};
pub use search::{SearchConfig, SearchResult};
pub use trigram::Trigram;

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
