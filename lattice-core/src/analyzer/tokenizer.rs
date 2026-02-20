use core::str;

/// Logical document field.
///
/// `#[repr(u8)]` guarantees stable 1-byte layout for compact storage
/// in posting lists or other packed index structures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Field {
    Title = 0,
    Body = 1,
    Tag = 2,
}

impl Field {
    /// Static scoring weight for this field.
    ///
    /// Not stored per token; derived during scoring.
    #[must_use]
    #[inline(always)]
    pub const fn weight(self) -> f32 {
        match self {
            Field::Title => 3.0,
            Field::Body => 1.0,
            Field::Tag => 2.0,
        }
    }
}

/// Streaming tokenizer.
///
/// Zero allocation. Emits `(text, field, position)` directly to caller.
///
/// Contract:
/// - Input must be normalized:
///   - ASCII only
///   - Lowercase
///   - No leading/trailing spaces
///   - No consecutive spaces
///
/// Splitting is a single forward byte scan on ASCII space (`0x20`).
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct Tokenizer {
    field: Field,
}

impl Tokenizer {
    #[inline]
    pub const fn new(field: Field) -> Self {
        Self { field }
    }

    /// Tokenizes normalized input and emits `(text, field, position)`.
    ///
    /// Position is `u32`. If overflow occurs, emission stops.
    #[inline(always)]
    #[allow(clippy::needless_lifetimes)]
    pub fn tokenize<'n, F>(&self, normalized: &'n str, mut emit: F)
    where
        F: FnMut(&'n str, Field, u32),
    {
        let bytes = normalized.as_bytes();

        debug_assert!(
            bytes.first().is_none_or(|&b| b != b' '),
            "tokenizer: leading whitespace — normalizer contract violated"
        );

        debug_assert!(
            bytes.last().is_none_or(|&b| b != b' '),
            "tokenizer: trailing whitespace — normalizer contract violated"
        );

        debug_assert!(
            {
                let mut prev_space = false;
                let mut ok = true;
                for &b in bytes {
                    if b == b' ' {
                        if prev_space {
                            ok = false;
                            break;
                        }
                        prev_space = true;
                    } else {
                        prev_space = false;
                    }
                }
                ok
            },
            "tokenizer: consecutive spaces — normalizer contract violated"
        );

        if bytes.is_empty() {
            return;
        }

        let len = bytes.len();
        let field = self.field;

        let mut start = 0usize;
        let mut pos = 0u32;

        for i in 0..len {
            if bytes[i] == b' ' {
                if start < i {
                    // SAFETY:
                    // - `normalized` is valid UTF-8
                    // - we split only on ASCII space (0x20)
                    let text = unsafe { str::from_utf8_unchecked(&bytes[start..i]) };

                    emit(text, field, pos);

                    debug_assert!(pos != u32::MAX, "tokenizer: position overflow");
                    if pos == u32::MAX {
                        return;
                    }

                    pos += 1;
                }
                start = i + 1;
            }
        }

        // final token
        if start < len {
            let text = unsafe { str::from_utf8_unchecked(&bytes[start..len]) };
            emit(text, field, pos);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn collect(input: &str, field: Field) -> Vec<(&str, Field, u32)> {
        let mut out = Vec::new();
        Tokenizer::new(field).tokenize(input, |text, f, pos| {
            out.push((text, f, pos));
        });
        out
    }

    #[test]
    fn field_size_is_1_byte() {
        assert_eq!(size_of::<Field>(), 1);
    }

    #[test]
    fn single_word() {
        let out = collect("hello", Field::Body);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "hello");
        assert_eq!(out[0].2, 0);
    }

    #[test]
    fn two_words() {
        let out = collect("hello world", Field::Body);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "hello");
        assert_eq!(out[1].0, "world");
    }

    #[test]
    fn positions_are_sequential() {
        let out = collect("the quick brown fox", Field::Body);
        assert_eq!(out.len(), 4);
        for (i, (_, _, pos)) in out.iter().enumerate() {
            assert_eq!(*pos, i as u32);
        }
    }

    #[test]
    fn empty_emits_nothing() {
        let out = collect("", Field::Body);
        assert!(out.is_empty());
    }

    #[test]
    fn single_char_token() {
        let out = collect("a", Field::Body);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "a");
    }

    #[test]
    fn field_propagated_to_all_tokens() {
        let out = collect("hello world foo", Field::Title);
        for (_, field, _) in &out {
            assert_eq!(*field, Field::Title);
        }
    }

    #[test]
    fn weight_derivable_from_field() {
        assert_eq!(Field::Title.weight(), 3.0);
        assert_eq!(Field::Body.weight(), 1.0);
        assert_eq!(Field::Tag.weight(), 2.0);
    }

    #[test]
    fn tokens_are_slices_of_input() {
        let input = String::from("hello world");
        let base = input.as_ptr() as usize;
        let end = base + input.len();

        Tokenizer::new(Field::Body).tokenize(&input, |text, _, _| {
            let ptr = text.as_ptr() as usize;
            assert!(ptr >= base && ptr < end);
        });
    }

    #[test]
    fn emit_order_is_left_to_right() {
        let words = ["one", "two", "three", "four"];
        let input = words.join(" ");
        let mut i = 0usize;

        Tokenizer::new(Field::Body).tokenize(&input, |text, _, pos| {
            assert_eq!(text, words[i]);
            assert_eq!(pos, i as u32);
            i += 1;
        });

        assert_eq!(i, words.len());
    }

    #[test]
    fn tokenizer_is_reusable() {
        let t = Tokenizer::new(Field::Title);

        let mut n = 0usize;
        t.tokenize("hello world", |_, _, _| n += 1);
        assert_eq!(n, 2);

        n = 0;
        t.tokenize("one two three", |_, _, _| n += 1);
        assert_eq!(n, 3);
    }

    #[test]
    fn composes_with_ngram_layer() {
        let mut gram_count = 0usize;

        Tokenizer::new(Field::Title).tokenize("hello world", |text, _, _| {
            let len = text.len();
            if len >= 3 {
                gram_count += len - 2;
            }
        });

        assert_eq!(gram_count, 6);
    }
}
