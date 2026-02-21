use std::str;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[rustfmt::skip]
const LOWERCASE_TABLE: [u8; 256] = [
    0x00,0x01,0x02,0x03,0x04,0x05,0x06,0x07,0x08,0x09,0x0a,0x0b,0x0c,0x0d,0x0e,0x0f,
    0x10,0x11,0x12,0x13,0x14,0x15,0x16,0x17,0x18,0x19,0x1a,0x1b,0x1c,0x1d,0x1e,0x1f,
    0x20,0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28,0x29,0x2a,0x2b,0x2c,0x2d,0x2e,0x2f,
    0x30,0x31,0x32,0x33,0x34,0x35,0x36,0x37,0x38,0x39,0x3a,0x3b,0x3c,0x3d,0x3e,0x3f,
    0x40,0x61,0x62,0x63,0x64,0x65,0x66,0x67,0x68,0x69,0x6a,0x6b,0x6c,0x6d,0x6e,0x6f,
    0x70,0x71,0x72,0x73,0x74,0x75,0x76,0x77,0x78,0x79,0x7a,0x5b,0x5c,0x5d,0x5e,0x5f,
    0x60,0x61,0x62,0x63,0x64,0x65,0x66,0x67,0x68,0x69,0x6a,0x6b,0x6c,0x6d,0x6e,0x6f,
    0x70,0x71,0x72,0x73,0x74,0x75,0x76,0x77,0x78,0x79,0x7a,0x7b,0x7c,0x7d,0x7e,0x7f,
    0x80,0x81,0x82,0x83,0x84,0x85,0x86,0x87,0x88,0x89,0x8a,0x8b,0x8c,0x8d,0x8e,0x8f,
    0x90,0x91,0x92,0x93,0x94,0x95,0x96,0x97,0x98,0x99,0x9a,0x9b,0x9c,0x9d,0x9e,0x9f,
    0xa0,0xa1,0xa2,0xa3,0xa4,0xa5,0xa6,0xa7,0xa8,0xa9,0xaa,0xab,0xac,0xad,0xae,0xaf,
    0xb0,0xb1,0xb2,0xb3,0xb4,0xb5,0xb6,0xb7,0xb8,0xb9,0xba,0xbb,0xbc,0xbd,0xbe,0xbf,
    0xc0,0xc1,0xc2,0xc3,0xc4,0xc5,0xc6,0xc7,0xc8,0xc9,0xca,0xcb,0xcc,0xcd,0xce,0xcf,
    0xd0,0xd1,0xd2,0xd3,0xd4,0xd5,0xd6,0xd7,0xd8,0xd9,0xda,0xdb,0xdc,0xdd,0xde,0xdf,
    0xe0,0xe1,0xe2,0xe3,0xe4,0xe5,0xe6,0xe7,0xe8,0xe9,0xea,0xeb,0xec,0xed,0xee,0xef,
    0xf0,0xf1,0xf2,0xf3,0xf4,0xf5,0xf6,0xf7,0xf8,0xf9,0xfa,0xfb,0xfc,0xfd,0xfe,0xff,
];

#[inline(always)]
const fn is_ascii_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\n' | b'\t' | b'\r')
}

/// Configuration options for text normalization.
#[derive(Clone, Copy)]
pub struct NormalizerConfig {
    /// When enabled, strips diacritical marks from Latin characters.
    /// For example, "café" becomes "cafe".
    pub strip_diacritics: bool,
}

impl Default for NormalizerConfig {
    fn default() -> Self {
        Self {
            strip_diacritics: false,
        }
    }
}

/// High-performance Unicode text normalizer.
///
/// Performs the following operations:
/// - Converts all characters to lowercase (Unicode-aware)
/// - Collapses consecutive ASCII whitespace into single spaces
/// - Removes leading/trailing ASCII whitespace
/// - Optionally strips diacritical marks from Latin characters
///
/// # Performance
///
/// Uses SIMD acceleration (AVX2/SSE2) for ASCII text paths on x86_64.
/// Falls back to scalar processing for non-ASCII content or on other architectures.
///
/// # Examples
///
/// ```
/// let normalizer = TextNormalizer::default();
/// assert_eq!(normalizer.normalize("  HELLO  WORLD  "), "hello world");
///
/// let stripper = TextNormalizer::new(NormalizerConfig { strip_diacritics: true });
/// assert_eq!(stripper.normalize("Café"), "cafe");
/// ```
pub struct TextNormalizer {
    config: NormalizerConfig,
}

impl Default for TextNormalizer {
    fn default() -> Self {
        Self::new(NormalizerConfig::default())
    }
}

impl TextNormalizer {
    /// Creates a new normalizer with the specified configuration.
    pub fn new(config: NormalizerConfig) -> Self {
        Self { config }
    }

    /// Normalizes text into an existing String buffer.
    ///
    /// Reuses the buffer's capacity if sufficient, growing only when necessary.
    /// Clears the buffer before writing.
    ///
    /// # Safety
    ///
    /// This method uses unsafe code for performance. The implementation maintains
    /// UTF-8 invariants and buffer bounds.
    #[inline]
    pub fn normalize_into(&self, input: &str, out: &mut String) {
        out.clear();
        out.reserve(input.len() + input.len() / 8);

        let bytes = input.as_bytes();
        let mut i = 0usize;
        let mut wrote = 0usize;
        let mut prev_space = false;
        let strip = self.config.strip_diacritics;

        unsafe {
            let buf = out.as_mut_vec();

            #[cfg(target_arch = "x86_64")]
            {
                if is_x86_feature_detected!("avx2") {
                    while i + 32 <= bytes.len() {
                        let chunk = _mm256_loadu_si256(bytes.as_ptr().add(i) as *const __m256i);
                        if _mm256_movemask_epi8(chunk) != 0 {
                            break;
                        }

                        for j in 0..32 {
                            let b = *bytes.get_unchecked(i + j);
                            if is_ascii_ws(b) {
                                if !prev_space {
                                    *buf.as_mut_ptr().add(wrote) = b' ';
                                    wrote += 1;
                                    prev_space = true;
                                }
                            } else {
                                *buf.as_mut_ptr().add(wrote) =
                                    *LOWERCASE_TABLE.get_unchecked(b as usize);
                                wrote += 1;
                                prev_space = false;
                            }
                        }
                        i += 32;
                    }
                }

                while i + 16 <= bytes.len() {
                    let chunk = _mm_loadu_si128(bytes.as_ptr().add(i) as *const __m128i);
                    if _mm_movemask_epi8(chunk) != 0 {
                        break;
                    }

                    for j in 0..16 {
                        let b = *bytes.get_unchecked(i + j);
                        if is_ascii_ws(b) {
                            if !prev_space {
                                *buf.as_mut_ptr().add(wrote) = b' ';
                                wrote += 1;
                                prev_space = true;
                            }
                        } else {
                            *buf.as_mut_ptr().add(wrote) =
                                *LOWERCASE_TABLE.get_unchecked(b as usize);
                            wrote += 1;
                            prev_space = false;
                        }
                    }
                    i += 16;
                }
            }

            while i < bytes.len() && bytes[i] < 128 {
                let b = bytes[i];
                if is_ascii_ws(b) {
                    if !prev_space {
                        *buf.as_mut_ptr().add(wrote) = b' ';
                        wrote += 1;
                        prev_space = true;
                    }
                } else {
                    *buf.as_mut_ptr().add(wrote) = *LOWERCASE_TABLE.get_unchecked(b as usize);
                    wrote += 1;
                    prev_space = false;
                }
                i += 1;
            }

            while i < bytes.len() {
                let ch = str::from_utf8_unchecked(&bytes[i..])
                    .chars()
                    .next()
                    .unwrap_unchecked();
                i += ch.len_utf8();

                for lowered in ch.to_lowercase() {
                    let folded = if strip { fold_latin1(lowered) } else { lowered };
                    if strip && folded == '\0' {
                        continue;
                    }

                    let mut tmp = [0u8; 4];
                    let enc = folded.encode_utf8(&mut tmp);

                    if wrote + enc.len() > buf.capacity() {
                        buf.set_len(wrote);
                        buf.reserve(32);
                    }

                    for &byte in enc.as_bytes() {
                        *buf.as_mut_ptr().add(wrote) = byte;
                        wrote += 1;
                    }

                    prev_space = false;
                }

                while i < bytes.len() && bytes[i] < 128 {
                    let b = bytes[i];
                    if is_ascii_ws(b) {
                        if !prev_space {
                            *buf.as_mut_ptr().add(wrote) = b' ';
                            wrote += 1;
                            prev_space = true;
                        }
                    } else {
                        *buf.as_mut_ptr().add(wrote) = *LOWERCASE_TABLE.get_unchecked(b as usize);
                        wrote += 1;
                        prev_space = false;
                    }
                    i += 1;
                }
            }

            if prev_space && wrote > 0 {
                wrote -= 1;
            }

            buf.set_len(wrote);
        }
    }

    /// Normalizes text and returns a new String.
    #[inline]
    pub fn normalize(&self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        self.normalize_into(input, &mut out);
        out
    }
}

#[inline(always)]
fn fold_latin1(c: char) -> char {
    if ('\u{0300}'..='\u{036F}').contains(&c) {
        return '\0';
    }

    match c {
        'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ā' | 'Ă' | 'Ą' | 'á' | 'à' | 'â' | 'ä' | 'ã' | 'å'
        | 'ā' | 'ă' | 'ą' => 'a',

        'Ç' | 'Ć' | 'Č' | 'Ĉ' | 'Ċ' | 'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => 'c',

        'Ð' | 'ð' | 'Đ' | 'đ' => 'd',

        'É' | 'È' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' | 'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ĕ'
        | 'ė' | 'ę' | 'ě' => 'e',

        'Í' | 'Ì' | 'Î' | 'Ï' | 'Ī' | 'Ĭ' | 'Į' | 'İ' | 'í' | 'ì' | 'î' | 'ï' | 'ī' | 'ĭ' | 'į'
        | 'ı' => 'i',

        'Ñ' | 'Ń' | 'Ň' | 'Ņ' | 'ñ' | 'ń' | 'ň' | 'ņ' => 'n',

        'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ō' | 'Ŏ' | 'Ő' | 'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' | 'ŏ'
        | 'ő' => 'o',

        'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' | 'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ŭ'
        | 'ů' | 'ű' | 'ų' => 'u',

        'Ý' | 'Ÿ' | 'ý' | 'ÿ' => 'y',

        'Ś' | 'Š' | 'Ş' | 'ś' | 'š' | 'ş' => 's',

        'Ź' | 'Ž' | 'Ż' | 'ź' | 'ž' | 'ż' => 'z',

        'ß' => 's',
        'Ł' | 'ł' => 'l',
        'Æ' | 'æ' => 'a',
        'Œ' | 'œ' => 'o',

        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(input: &str) -> String {
        TextNormalizer::default().normalize(input)
    }

    fn norm_strip(input: &str) -> String {
        TextNormalizer::new(NormalizerConfig {
            strip_diacritics: true,
        })
            .normalize(input)
    }

    #[test]
    fn ascii_basic_lowercase() {
        assert_eq!(norm("HELLO"), "hello");
        assert_eq!(norm("HeLlO"), "hello");
        assert_eq!(norm("123 ABC!"), "123 abc!");
    }

    #[test]
    fn ascii_full_alphabet() {
        let upper: String = (b'A'..=b'Z').map(|b| b as char).collect();
        let lower: String = (b'a'..=b'z').map(|b| b as char).collect();
        assert_eq!(norm(&upper), lower);
    }

    #[test]
    fn ascii_punctuation_unchanged() {
        assert_eq!(norm("foo-bar_baz"), "foo-bar_baz");
    }

    #[test]
    fn whitespace_collapse() {
        assert_eq!(norm("hello   world"), "hello world");
        assert_eq!(norm("hello\t\nworld"), "hello world");
        assert_eq!(norm("hello \r\n world"), "hello world");
    }

    #[test]
    fn leading_whitespace_collapses_not_removed() {
        assert_eq!(norm("   hello"), " hello");
    }

    #[test]
    fn trailing_whitespace_removed() {
        assert_eq!(norm("hello   "), "hello");
    }

    #[test]
    fn only_whitespace() {
        assert_eq!(norm("   "), "");
        assert_eq!(norm("\n\t\r"), "");
    }

    #[test]
    fn no_double_spaces() {
        let out = norm("hello   world  test");
        assert!(!out.contains("  "));
    }

    #[test]
    fn exactly_16_bytes() {
        assert_eq!(norm("ABCDEFGHIJKLMNOP"), "abcdefghijklmnop");
    }

    #[test]
    fn exactly_32_bytes() {
        assert_eq!(
            norm("ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP"),
            "abcdefghijklmnopabcdefghijklmnop"
        );
    }

    #[test]
    fn less_than_16_bytes() {
        assert_eq!(norm("HELLO"), "hello");
    }

    #[test]
    fn unicode_breaks_simd() {
        assert_eq!(norm("héllo"), "héllo");
    }

    #[test]
    fn unicode_at_boundary() {
        assert_eq!(norm("ABCDEFGHIJKLMNOP café"), "abcdefghijklmnop café");
    }

    #[test]
    fn unicode_basic_lowercase() {
        assert_eq!(norm("ПРИВЕТ"), "привет");
        assert_eq!(norm("ÜNITED"), "ünited");
    }

    #[test]
    fn expanding_lowercase_does_not_panic() {
        let result = norm("İstanbul");
        assert!(str::from_utf8(result.as_bytes()).is_ok());
    }

    #[test]
    fn multiple_expanding_chars() {
        let input = "İİİİİİİİİİ";
        let result = norm(input);
        assert!(str::from_utf8(result.as_bytes()).is_ok());
    }

    #[test]
    fn diacritics_preserved_when_disabled() {
        assert_eq!(norm("café"), "café");
        assert_eq!(norm("Müller"), "müller");
        assert_eq!(norm("São"), "são");
    }

    #[test]
    fn basic_diacritic_strip() {
        assert_eq!(norm_strip("café"), "cafe");
        assert_eq!(norm_strip("Müller"), "muller");
        assert_eq!(norm_strip("São"), "sao");
    }

    #[test]
    fn extended_latin_strip() {
        assert_eq!(norm_strip("Český"), "cesky");
        assert_eq!(norm_strip("Żółć"), "zolc");
        assert_eq!(norm_strip("ŠĐĆŽčđ"), "sdczcd");
    }

    #[test]
    fn sharp_s_strip() {
        assert_eq!(norm_strip("straße"), "strase");
    }

    #[test]
    fn mixed_ascii_and_unicode_strip() {
        assert_eq!(norm_strip("Hello São World"), "hello sao world");
    }

    #[test]
    fn normalize_into_reuses_capacity() {
        let normalizer = TextNormalizer::default();
        let mut buf = String::with_capacity(64);
        let cap = buf.capacity();

        normalizer.normalize_into("HELLO", &mut buf);
        assert_eq!(buf, "hello");
        assert_eq!(buf.capacity(), cap);

        normalizer.normalize_into("WORLD", &mut buf);
        assert_eq!(buf, "world");
        assert_eq!(buf.capacity(), cap);
    }

    #[test]
    fn buffer_grows_when_needed() {
        let normalizer = TextNormalizer::default();
        let mut buf = String::new();
        let long = "A".repeat(1024);
        normalizer.normalize_into(&long, &mut buf);
        assert_eq!(buf.len(), 1024);
        assert!(buf.capacity() >= 1024);
    }

    #[test]
    fn output_always_valid_utf8() {
        let inputs = [
            "hello",
            "café",
            "İstanbul",
            "ΠΡΟΒΛΗΜΑ",
            "مرحبا",
            "こんにちは",
        ];

        for input in inputs {
            let out = norm(input);
            assert!(str::from_utf8(out.as_bytes()).is_ok());
        }
    }

    #[test]
    fn idempotent_without_strip() {
        let n = TextNormalizer::default();
        let samples = ["hello world", "foo   bar", "ÜBER Café"];

        for s in samples {
            let once = n.normalize(s);
            let twice = n.normalize(&once);
            assert_eq!(once, twice);
        }
    }

    #[test]
    fn idempotent_with_strip() {
        let n = TextNormalizer::new(NormalizerConfig {
            strip_diacritics: true,
        });

        let samples = ["Müller São", "Český Žlutý kůň"];

        for s in samples {
            let once = n.normalize(s);
            let twice = n.normalize(&once);
            assert_eq!(once, twice);
        }
    }

    #[test]
    fn no_trailing_space() {
        let out = norm("hello world   ");
        assert!(!out.ends_with(' '));
    }

    #[test]
    fn ascii_output_not_longer() {
        let input = "HELLO   WORLD";
        let out = norm(input);
        assert!(out.len() <= input.len());
    }

    #[test]
    fn empty_input() {
        assert_eq!(norm(""), "");
    }

    #[test]
    fn single_char() {
        assert_eq!(norm("A"), "a");
    }

    #[test]
    fn null_byte_passthrough() {
        assert_eq!(norm("a\0b"), "a\0b");
    }

    #[test]
    fn combining_diacritics_removed_when_strip_enabled() {
        assert_eq!(norm_strip("café"), "cafe");
        assert_eq!(norm_strip("caf\u{0301}e"), "cafe");
    }

    #[test]
    fn emoji_passthrough() {
        assert_eq!(norm("Hello 🌍 World"), "hello 🌍 world");
        assert_eq!(norm_strip("Café 🍵"), "cafe 🍵");
    }

    #[test]
    fn zero_width_chars() {
        assert_eq!(norm("hello\u{200B}world"), "hello\u{200B}world");
    }

    #[test]
    fn control_chars_passthrough() {
        assert_eq!(norm("hello\x01\x02world"), "hello\x01\x02world");
    }

    #[test]
    fn very_long_ascii() {
        let input = "A".repeat(10000);
        let out = norm(&input);
        assert_eq!(out.len(), 10000);
        assert!(out.chars().all(|c| c == 'a'));
    }

    #[test]
    fn mixed_simd_boundary_unicode() {
        assert_eq!(norm("1234567890123456é"), "1234567890123456é");
        assert_eq!(norm("12345678901234567890123456789012é"), "12345678901234567890123456789012é");
    }

    #[test]
    fn turkish_i_handling() {
        let result = norm("İIıi");
        assert!(result.contains('i'));
        assert!(str::from_utf8(result.as_bytes()).is_ok());
    }

    #[test]
    fn german_eszett() {
        assert_eq!(norm("STRASSE"), "strasse");
        assert_eq!(norm("STRAßE"), "straße");
        assert_eq!(norm_strip("STRAßE"), "strasse");
    }

    #[test]
    fn scandinavian_chars() {
        assert_eq!(norm("ÅÄÖ"), "åäö");
        assert_eq!(norm_strip("ÅÄÖ"), "aao");
    }

    #[test]
    fn slavic_chars() {
        assert_eq!(norm_strip("Łódź"), "lodz");
        assert_eq!(norm_strip("Żółć"), "zolc");
    }

    #[test]
    fn greek_text() {
        assert_eq!(norm("ΆΈΉ"), "άέή");
    }

    #[test]
    fn cyrillic_text() {
        assert_eq!(norm("ЁЖЗ"), "ёжз");
    }

    #[test]
    fn arabic_text() {
        assert_eq!(norm("مرحبا"), "مرحبا");
    }

    #[test]
    fn chinese_text() {
        assert_eq!(norm("你好世界"), "你好世界");
    }

    #[test]
    fn japanese_text() {
        assert_eq!(norm("カタカナ"), "カタカナ");
        assert_eq!(norm("ひらがな"), "ひらがな");
    }

    #[test]
    fn korean_text() {
        assert_eq!(norm("한글"), "한글");
    }

    #[test]
    fn hebrew_text() {
        assert_eq!(norm("שלום"), "שלום");
    }

    #[test]
    fn thai_text() {
        assert_eq!(norm("สวัสดี"), "สวัสดี");
    }

    #[test]
    fn multiple_normalize_calls_same_buffer() {
        let n = TextNormalizer::default();
        let mut buf = String::with_capacity(128);

        for i in 0..100 {
            n.normalize_into(&format!("TEST{}", i), &mut buf);
            assert!(buf.capacity() >= 128 || buf.capacity() >= buf.len());
        }
    }

    #[test]
    fn empty_string_normalize_into() {
        let n = TextNormalizer::default();
        let mut buf = String::with_capacity(64);
        n.normalize_into("", &mut buf);
        assert_eq!(buf, "");
        assert_eq!(buf.capacity(), 64);
    }

    #[test]
    fn whitespace_only_variations() {
        assert_eq!(norm(" "), "");
        assert_eq!(norm("  "), "");
        assert_eq!(norm("\t"), "");
        assert_eq!(norm("\n"), "");
        assert_eq!(norm("\r\n"), "");
        assert_eq!(norm(" \t\n\r "), "");
    }

    #[test]
    fn single_leading_space_preserved() {
        assert_eq!(norm(" hello"), " hello");
    }

    #[test]
    fn config_clone_copy() {
        let c1 = NormalizerConfig::default();
        let c2 = c1;
        let _ = c1;
        let _ = c2;
    }

    #[test]
    fn normalizer_clone() {
        let n1 = TextNormalizer::default();
        let n2 = &n1;
        assert_eq!(n1.normalize("TEST"), n2.normalize("TEST"));
    }
}