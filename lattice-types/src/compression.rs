//! Compression utilities for integer sequences.
//!
//! Provides delta encoding and variable-length integer compression
//! optimized for sorted sequences like document ID lists.

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
