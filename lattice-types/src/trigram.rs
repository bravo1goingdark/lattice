//! Trigram type for substring indexing.

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
