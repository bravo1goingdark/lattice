//! Index building logic.

use crate::index::types::{Lattice, PostingBlock, TempTrigramEntry, RADIX_SORT_THRESHOLD};
use lattice_types::{DocId, Trigram};

impl Lattice {
    /// Commits `temp_trigrams` into the main index.
    ///
    /// ## Cold-start fast path (first build — common case for bulk ingestion)
    ///
    /// When the committed index is empty, we skip the merge and build directly
    /// from `temp_trigrams` in one O(Δ sort + Δ scan) pass. This is the hot
    /// path for bulk-then-search workloads and is equivalent to the original
    /// non-incremental implementation.
    ///
    /// ## Incremental path (interleaved add/search)
    ///
    /// When a committed index already exists, the delta is sorted, converted
    /// to blocks, then merged with the committed index in O(N + Δ).
    pub(crate) fn rebuild_index(&mut self) {
        if self.temp_trigrams.is_empty() {
            self.needs_rebuild = false;
            return;
        }

        Self::sort_trigrams(&mut self.temp_trigrams);

        if self.blocks.is_empty() {
            let (blocks, postings) = Self::build_blocks_from_sorted(&self.temp_trigrams);
            self.blocks = blocks;
            self.postings = postings;
        } else {
            let (delta_blocks, delta_postings) =
                Self::build_blocks_from_sorted(&self.temp_trigrams);
            let (merged_blocks, merged_postings) =
                Self::merge_indexes(&self.blocks, &self.postings, &delta_blocks, &delta_postings);
            self.blocks = merged_blocks;
            self.postings = merged_postings;
        }

        self.temp_trigrams.clear();
        self.needs_rebuild = false;
    }

    pub(crate) fn sort_trigrams(entries: &mut [TempTrigramEntry]) {
        if entries.len() < RADIX_SORT_THRESHOLD {
            entries.sort_unstable_by(|a, b| {
                a.trigram
                    .0
                    .cmp(&b.trigram.0)
                    .then_with(|| a.doc_id.cmp(&b.doc_id))
            });
            return;
        }

        let len = entries.len();

        let dummy = TempTrigramEntry {
            trigram: Trigram(0),
            doc_id: 0,
        };
        let mut aux = vec![dummy; len];

        Self::radix_pass(entries, &mut aux, |e| e.doc_id as u8);
        Self::radix_pass(&aux, entries, |e| (e.doc_id >> 8) as u8);
        Self::radix_pass(entries, &mut aux, |e| (e.doc_id >> 16) as u8);
        Self::radix_pass(&aux, entries, |e| (e.doc_id >> 24) as u8);
        Self::radix_pass(entries, &mut aux, |e| e.trigram.0 as u8);
        Self::radix_pass(&aux, entries, |e| (e.trigram.0 >> 8) as u8);
        Self::radix_pass(entries, &mut aux, |e| (e.trigram.0 >> 16) as u8);

        entries.copy_from_slice(&aux);
    }

    #[inline(always)]
    fn radix_pass(
        src: &[TempTrigramEntry],
        dst: &mut [TempTrigramEntry],
        key_fn: impl Fn(&TempTrigramEntry) -> u8,
    ) {
        let mut hist = [0u32; 256];
        let mut offsets = [0u32; 256];

        for entry in src {
            hist[key_fn(entry) as usize] += 1;
        }

        let mut sum = 0u32;
        for (h, off) in hist.iter().zip(offsets.iter_mut()) {
            *off = sum;
            sum += h;
        }

        for entry in src {
            let k = key_fn(entry) as usize;
            dst[offsets[k] as usize] = *entry;
            offsets[k] += 1;
        }
    }

    pub(crate) fn build_blocks_from_sorted(
        entries: &[TempTrigramEntry],
    ) -> (Vec<PostingBlock>, Vec<DocId>) {
        if entries.is_empty() {
            return (Vec::new(), Vec::new());
        }

        let mut blocks: Vec<PostingBlock> = Vec::new();
        let mut postings: Vec<DocId> = Vec::with_capacity(entries.len());

        let mut current_trigram = entries[0].trigram.0;
        let mut current_offset = 0u32;
        let mut current_len = 0u32;
        let mut last_doc_id: Option<DocId> = None;

        for entry in entries {
            let trigram = entry.trigram.0;
            let doc_id = entry.doc_id;

            if trigram != current_trigram {
                blocks.push(PostingBlock {
                    trigram: Trigram(current_trigram),
                    offset: current_offset,
                    len: current_len,
                });
                current_offset += current_len;
                current_trigram = trigram;
                current_len = 0;
                last_doc_id = None;
            }

            if last_doc_id != Some(doc_id) {
                postings.push(doc_id);
                current_len += 1;
                last_doc_id = Some(doc_id);
            }
        }

        blocks.push(PostingBlock {
            trigram: Trigram(current_trigram),
            offset: current_offset,
            len: current_len,
        });

        (blocks, postings)
    }

    pub(crate) fn merge_indexes(
        a_blocks: &[PostingBlock],
        a_postings: &[DocId],
        b_blocks: &[PostingBlock],
        b_postings: &[DocId],
    ) -> (Vec<PostingBlock>, Vec<DocId>) {
        let mut out_blocks: Vec<PostingBlock> = Vec::with_capacity(a_blocks.len() + b_blocks.len());
        let mut out_postings: Vec<DocId> = Vec::with_capacity(a_postings.len() + b_postings.len());

        let mut ai = 0usize;
        let mut bi = 0usize;

        while ai < a_blocks.len() && bi < b_blocks.len() {
            let at = a_blocks[ai].trigram.0;
            let bt = b_blocks[bi].trigram.0;

            match at.cmp(&bt) {
                std::cmp::Ordering::Less => {
                    Self::copy_block(
                        &a_blocks[ai],
                        a_postings,
                        &mut out_blocks,
                        &mut out_postings,
                    );
                    ai += 1;
                }
                std::cmp::Ordering::Greater => {
                    Self::copy_block(
                        &b_blocks[bi],
                        b_postings,
                        &mut out_blocks,
                        &mut out_postings,
                    );
                    bi += 1;
                }
                std::cmp::Ordering::Equal => {
                    let a_list = Self::block_postings(&a_blocks[ai], a_postings);
                    let b_list = Self::block_postings(&b_blocks[bi], b_postings);
                    let merged_offset = out_postings.len() as u32;
                    Self::merge_sorted_dedup(a_list, b_list, &mut out_postings);
                    let merged_len = out_postings.len() as u32 - merged_offset;
                    out_blocks.push(PostingBlock {
                        trigram: a_blocks[ai].trigram,
                        offset: merged_offset,
                        len: merged_len,
                    });
                    ai += 1;
                    bi += 1;
                }
            }
        }

        while ai < a_blocks.len() {
            Self::copy_block(
                &a_blocks[ai],
                a_postings,
                &mut out_blocks,
                &mut out_postings,
            );
            ai += 1;
        }
        while bi < b_blocks.len() {
            Self::copy_block(
                &b_blocks[bi],
                b_postings,
                &mut out_blocks,
                &mut out_postings,
            );
            bi += 1;
        }

        (out_blocks, out_postings)
    }

    #[inline(always)]
    fn copy_block(
        block: &PostingBlock,
        source_postings: &[DocId],
        out_blocks: &mut Vec<PostingBlock>,
        out_postings: &mut Vec<DocId>,
    ) {
        let new_offset = out_postings.len() as u32;
        out_postings.extend_from_slice(Self::block_postings(block, source_postings));
        out_blocks.push(PostingBlock {
            trigram: block.trigram,
            offset: new_offset,
            len: block.len,
        });
    }

    #[inline(always)]
    pub(crate) fn block_postings<'a>(block: &PostingBlock, postings: &'a [DocId]) -> &'a [DocId] {
        let start = block.offset as usize;
        &postings[start..start + block.len as usize]
    }

    pub(crate) fn merge_sorted_dedup(a: &[DocId], b: &[DocId], out: &mut Vec<DocId>) {
        let mut ai = 0usize;
        let mut bi = 0usize;

        while ai < a.len() && bi < b.len() {
            match a[ai].cmp(&b[bi]) {
                std::cmp::Ordering::Less => {
                    out.push(a[ai]);
                    ai += 1;
                }
                std::cmp::Ordering::Greater => {
                    out.push(b[bi]);
                    bi += 1;
                }
                std::cmp::Ordering::Equal => {
                    out.push(a[ai]);
                    ai += 1;
                    bi += 1;
                }
            }
        }

        out.extend_from_slice(&a[ai..]);
        out.extend_from_slice(&b[bi..]);
    }
}
