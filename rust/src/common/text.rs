//! Text strings (PDF 1.6 Reference, 3.8.1): `PDFDocEncoding`, or UTF-16BE behind a byte order mark.
//!
//! PDF/X-4 references PDF 1.6, which has no UTF-8 text strings, so bytes starting `EF BB BF` are read as `PDFDocEncoding`.

/// `PDFDocEncoding`'s code points for bytes 0x18 to 0x1F (PDF 1.6 Reference, Appendix D).
const LOW: [u16; 8] = [0x02D8, 0x02C7, 0x02C6, 0x02D9, 0x02DD, 0x02DB, 0x02DA, 0x02DC];

/// `PDFDocEncoding`'s code points for bytes 0x80 to 0xA0; 0 marks the undefined 0x9F.
const HIGH: [u16; 33] = [
    0x2022, 0x2020, 0x2021, 0x2026, 0x2014, 0x2013, 0x0192, 0x2044, 0x2039, 0x203A, 0x2212, 0x2030, 0x201E, 0x201C,
    0x201D, 0x2018, 0x2019, 0x201A, 0x2122, 0xFB01, 0xFB02, 0x0141, 0x0152, 0x0160, 0x0178, 0x017D, 0x0131, 0x0142,
    0x0153, 0x0161, 0x017E, 0, 0x20AC,
];

/// One `PDFDocEncoding` byte as a character, or `None` where the encoding leaves it undefined.
fn pdf_doc(b: u8) -> Option<char> {
    let code = match b {
        0x09 | 0x0A | 0x0D | 0x20..=0x7E | 0xA1..=0xAC | 0xAE..=0xFF => u16::from(b),
        0x18..=0x1F => LOW.get(usize::from(b - 0x18)).copied().unwrap_or(0),
        0x80..=0xA0 => HIGH.get(usize::from(b - 0x80)).copied().unwrap_or(0),
        _ => 0,
    };
    (code != 0).then(|| char::from_u32(u32::from(code))).flatten()
}

/// A text string's characters, or `None` when its bytes are not valid in its encoding.
///
/// A UTF-16 string's language escapes (U+001B, a language code, U+001B) are dropped: they are not part of the text.
pub(crate) fn decode(bytes: &[u8]) -> Option<String> {
    let Some(utf16) = bytes.strip_prefix(b"\xFE\xFF") else {
        return bytes.iter().map(|&b| pdf_doc(b)).collect();
    };
    let (pairs, rest) = utf16.as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let units = pairs.iter().map(|pair| u16::from_be_bytes(*pair));
    let text: String = char::decode_utf16(units).collect::<Result<_, _>>().ok()?;
    Some(text.split('\u{1B}').step_by(2).collect())
}

/// Whether a string's bytes are UTF-16BE behind a byte order mark.
pub(crate) fn is_utf16(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\xFE\xFF")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_doc_encoding_differs_from_latin_1_in_its_own_places() {
        assert_eq!(decode(b"Caf\xE9 \x84 \x18\xA0").as_deref(), Some("Café — ˘€"));
        assert_eq!(decode(b"\x92\x93\x9E"), Some("™ﬁž".to_owned()));
        for undefined in [0x00, 0x7F, 0x9F, 0xAD, 0x17] {
            assert_eq!(decode(&[b'a', undefined]), None, "{undefined:#x}");
        }
        assert_eq!(decode(b"\xEF\xBB\xBFx").as_deref(), Some("ï»¿x"));
    }

    #[test]
    fn utf16_with_a_byte_order_mark() {
        assert_eq!(
            decode(b"\xFE\xFF\x00A\x00\xE9\xD8\x3D\xDE\x00").as_deref(),
            Some("Aé😀")
        );
        assert_eq!(decode(b"\xFE\xFF\x00A\x00"), None, "odd length");
        assert_eq!(decode(b"\xFE\xFF\xD8\x3D"), None, "lone surrogate");
        assert_eq!(
            decode(b"\xFE\xFF\x00\x1B\x00e\x00n\x00\x1B\x00H\x00i").as_deref(),
            Some("Hi")
        );
        assert!(is_utf16(b"\xFE\xFF") && !is_utf16(b"PDF/X-4"));
    }
}
