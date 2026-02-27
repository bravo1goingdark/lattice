//! Bump Allocator for Document Storage
//!
//! Eliminates per-document allocations by storing all text in a single
//! contiguous buffer. Documents are referenced by (offset, length) pairs.
//!
//! ## Memory Layout
//!
//! ```text
//! Arena Buffer: [doc0][doc1][doc2][doc3]...[free space]
//!               ^     ^     ^     ^
//!               |     |     |     |
//! Spans:       (0,5) (5,7) (12,4) (16,8) ...
//! ```
//!
//! ## Performance
//!
//! - Allocation: O(1) - just bump pointer
//! - Retrieval: O(1) - slice from buffer
//! - Memory overhead: 6 bytes per document (u32 offset + u16 len)
//! - Cache efficiency: Documents stored sequentially (good for iteration)

/// Document reference - 6 bytes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocSpan {
    offset: u32,
    len: u16,
}

impl DocSpan {
    /// Creates a new document span.
    #[inline(always)]
    pub const fn new(offset: u32, len: u16) -> Self {
        Self { offset, len }
    }

    /// Returns the byte offset in the arena.
    #[inline(always)]
    pub const fn offset(self) -> usize {
        self.offset as usize
    }

    /// Returns the byte length.
    #[inline(always)]
    pub const fn len(self) -> usize {
        self.len as usize
    }
}

/// Bump allocator for document text.
pub struct Arena {
    /// Contiguous storage buffer
    buffer: Vec<u8>,
    /// Document spans (offset, length pairs)
    spans: Vec<DocSpan>,
    /// Current write position (bump pointer)
    head: usize,
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Arena {
    /// Creates a new empty arena.
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(64 * 1024), // 64KB initial
            spans: Vec::with_capacity(1024),
            head: 0,
        }
    }

    /// Creates a new arena with pre-allocated capacity.
    pub fn with_capacity(buffer_cap: usize, doc_cap: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(buffer_cap),
            spans: Vec::with_capacity(doc_cap),
            head: 0,
        }
    }

    /// Returns the number of documents stored.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Returns true if no documents are stored.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Clears all documents (resets bump pointer but keeps capacity).
    pub fn clear(&mut self) {
        self.head = 0;
        self.spans.clear();
        // Note: we don't clear buffer to avoid re-zeroing
    }

    /// Adds a document to the arena.
    ///
    /// # Errors
    ///
    /// Returns `None` if the document length exceeds u16::MAX (65535 bytes).
    #[inline]
    pub fn push(&mut self, text: &str) -> Option<u32> {
        let bytes = text.as_bytes();
        let len = bytes.len();
        if len > u16::MAX as usize {
            return None;
        }

        let doc_id = self.spans.len() as u32;
        let offset = self.head;

        // Ensure capacity with 1.5x growth factor for better memory efficiency
        if offset + len > self.buffer.capacity() {
            let new_cap = (self.buffer.capacity() * 3 / 2).max(offset + len).max(4096);
            self.buffer.reserve(new_cap - self.buffer.capacity());
        }

        unsafe {
            // SAFETY: We reserved capacity for `offset + len` above.
            // `copy_nonoverlapping` is valid because:
            // - `bytes.as_ptr()` is valid for `len` bytes (it's a valid string slice)
            // - `self.buffer.as_mut_ptr().add(offset)` is valid for `len` bytes
            //   (we just ensured capacity and offset < capacity)
            // - Both pointers are properly aligned (u8 has align 1)
            // - The regions don't overlap (we're writing to arena buffer, reading from input)
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.buffer.as_mut_ptr().add(offset),
                len,
            );
            // SAFETY: `set_len` is valid because:
            // - We just wrote `len` bytes starting at `offset`
            // - `self.head = offset + len` is the new valid length
            // - All bytes in the buffer are initialized (we only ever append)
            self.head = offset + len;
            self.buffer.set_len(self.head);
        }

        self.spans.push(DocSpan::new(offset as u32, len as u16));
        Some(doc_id)
    }

    /// Gets a document by ID.
    #[inline(always)]
    pub fn get(&self, doc_id: u32) -> Option<&str> {
        let span = self.spans.get(doc_id as usize)?;
        let start = span.offset();
        let end = start + span.len();

        // SAFETY: `from_utf8_unchecked` is valid because:
        // - We only store valid UTF-8 data (verified `&str` input to `push`)
        // - The span offsets point to contiguous bytes within the buffer
        // - We never modify buffer contents after writing
        // - Bounds were validated above via `get(doc_id)`
        unsafe { Some(std::str::from_utf8_unchecked(&self.buffer[start..end])) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_push_get() {
        let mut arena = Arena::new();

        let id0 = arena.push("hello").expect("should push");
        let id1 = arena.push("world").expect("should push");
        let id2 = arena.push("foo bar baz").expect("should push");

        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);

        assert_eq!(arena.get(id0), Some("hello"));
        assert_eq!(arena.get(id1), Some("world"));
        assert_eq!(arena.get(id2), Some("foo bar baz"));
    }

    #[test]
    fn empty_document() {
        let mut arena = Arena::new();
        let id = arena.push("").expect("should push");
        assert_eq!(arena.get(id), Some(""));
        assert_eq!(arena.len(), 1);
    }

    #[test]
    fn large_document() {
        let mut arena = Arena::new();
        let text = "x".repeat(60000);
        let id = arena.push(&text).expect("should push");
        assert_eq!(arena.get(id), Some(text.as_str()));
    }

    #[test]
    fn document_too_long() {
        let mut arena = Arena::new();
        let text = "x".repeat(70000);
        assert!(arena.push(&text).is_none());
    }

    #[test]
    fn clear_resets() {
        let mut arena = Arena::with_capacity(1024 * 1024, 1000);
        for i in 0..100 {
            arena.push(&format!("doc{}", i)).expect("should push");
        }

        arena.clear();

        assert_eq!(arena.len(), 0);
        assert!(arena.is_empty());
    }

    #[test]
    fn many_documents() {
        let mut arena = Arena::with_capacity(10 * 1024 * 1024, 100_000);
        for i in 0..10_000 {
            arena
                .push(&format!("document number {} here", i))
                .expect("should push");
        }
        assert_eq!(arena.len(), 10_000);

        // Spot check
        assert!(arena.get(0).unwrap().contains("document number 0"));
        assert!(arena.get(9999).unwrap().contains("document number 9999"));
    }
}
