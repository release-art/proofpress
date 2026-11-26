//! Tier 3: stream filters, bytes to bytes (ISO 32000-2, 7.4), decoding within a budget.
//!
//! Tier 3 knows nothing of objects: the caller names the filters and their parameters. A decode stops at its budget
//! (decoding takes at most one byte more than allowed, then refuses), and anything it cannot decode is damage with a
//! reason, never a guess. Image filters are not decoded here: a stream ending in one stays encoded until a check reads
//! the image.

use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::inflate::{TINFLStatus, decompress_to_vec_zlib_with_limit};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};

/// A filter, by the name the stream gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Filter {
    /// `ASCIIHexDecode`, or `AHx` in an inline image.
    AsciiHex,
    /// `ASCII85Decode`, or `A85`.
    Ascii85,
    /// `LZWDecode`, or `LZW`.
    Lzw,
    /// `FlateDecode`, or `Fl`.
    Flate,
    /// `RunLengthDecode`, or `RL`.
    RunLength,
    /// `DCTDecode`, `JPXDecode`, `JBIG2Decode` or `CCITTFaxDecode`: left encoded for the image checks.
    Image,
    /// `Crypt`: encryption, which a verifier refuses.
    Crypt,
}

impl Filter {
    /// The filter a name stands for, full or abbreviated (the abbreviations are only legal in inline images, which the
    /// caller judges), or `None` for a name no standard defines.
    pub(crate) fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"ASCIIHexDecode" | b"AHx" => Self::AsciiHex,
            b"ASCII85Decode" | b"A85" => Self::Ascii85,
            b"LZWDecode" | b"LZW" => Self::Lzw,
            b"FlateDecode" | b"Fl" => Self::Flate,
            b"RunLengthDecode" | b"RL" => Self::RunLength,
            b"DCTDecode" | b"DCT" | b"JPXDecode" | b"JBIG2Decode" | b"CCITTFaxDecode" | b"CCF" => Self::Image,
            b"Crypt" => Self::Crypt,
            _ => return None,
        })
    }
}

/// The decode parameters of Flate and LZW (7.4.4.4), with their defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Params {
    pub(crate) predictor: i64,
    pub(crate) colors: i64,
    pub(crate) bits_per_component: i64,
    pub(crate) columns: i64,
    pub(crate) early_change: i64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            predictor: 1,
            colors: 1,
            bits_per_component: 8,
            columns: 1,
            early_change: 1,
        }
    }
}

/// Why a decode failed. Inside a stream, reading stops at the first damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Damage {
    /// The data is not valid for the filter.
    Corrupt(Filter),
    /// Decoding would exceed the budget, in bytes.
    OverBudget(usize),
    /// The stream is encrypted.
    Encrypted,
    /// A predictor, or parameters, that 7.4.4.4 does not define.
    BadParams,
}

/// The decoded data, or `Encoded` when an image filter stops the chain: `data` is then what precedes that filter.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decoded {
    /// Every filter was applied.
    Plain(Vec<u8>),
    /// The chain reached an image filter; the data is what that filter would read.
    Encoded(Vec<u8>),
}

/// Apply `chain` in order to `data`, producing at most `budget` bytes at every step.
pub(crate) fn decode(data: &[u8], chain: &[(Filter, Params)], budget: usize) -> Result<Decoded, Damage> {
    let mut bytes = data.to_vec();
    for (filter, params) in chain {
        bytes = match filter {
            Filter::Image => return Ok(Decoded::Encoded(bytes)),
            Filter::Crypt => return Err(Damage::Encrypted),
            Filter::AsciiHex => ascii_hex(&bytes)?,
            Filter::Ascii85 => ascii85(&bytes)?,
            Filter::RunLength => run_length(&bytes, budget)?,
            Filter::Flate => predict(flate(&bytes, budget)?, params)?,
            Filter::Lzw => predict(lzw(&bytes, params.early_change != 0, budget)?.0, params)?,
        };
        if bytes.len() > budget {
            return Err(Damage::OverBudget(budget));
        }
    }
    Ok(Decoded::Plain(bytes))
}

/// Zero bytes decode to zero bytes: there is nothing to misread, and qpdf writes an empty stream so.
fn flate(data: &[u8], budget: usize) -> Result<Vec<u8>, Damage> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    decompress_to_vec_zlib_with_limit(data, budget).map_err(|e| match e.status {
        TINFLStatus::HasMoreOutput => Damage::OverBudget(budget),
        _ => Damage::Corrupt(Filter::Flate),
    })
}

const fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

const fn is_white(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}

/// 7.4.2: hex digit pairs up to `>`; white space ignored; an odd last digit is followed by 0.
fn ascii_hex(data: &[u8]) -> Result<Vec<u8>, Damage> {
    let corrupt = Damage::Corrupt(Filter::AsciiHex);
    let end = data.iter().position(|&b| b == b'>').ok_or(corrupt)?;
    let mut digits = Vec::new();
    for &b in data.iter().take(end).filter(|&&b| !is_white(b)) {
        digits.push(hex_value(b).ok_or(corrupt)?);
    }
    Ok(digits
        .chunks(2)
        .map(|pair| pair.iter().chain([&0]).take(2).fold(0, |acc, d| acc << 4 | d))
        .collect())
}

/// 7.4.3: groups of five characters `!` to `u` give four bytes, `z` four zeros, up to `~>`.
fn ascii85(data: &[u8]) -> Result<Vec<u8>, Damage> {
    let corrupt = Damage::Corrupt(Filter::Ascii85);
    let end = data.windows(2).position(|w| w == b"~>").ok_or(corrupt)?;
    let mut out = Vec::new();
    let mut group: Vec<u32> = Vec::with_capacity(5);
    for &b in data.iter().take(end).filter(|&&b| !is_white(b)) {
        match b {
            b'z' if group.is_empty() => out.extend([0; 4]),
            b'!'..=b'u' => {
                group.push(u32::from(b - b'!'));
                if group.len() == 5 {
                    out.extend(base85(&group).ok_or(corrupt)?);
                    group.clear();
                }
            }
            _ => return Err(corrupt),
        }
    }
    // A final partial group of n characters is padded with `u` and gives n - 1 bytes; one character alone is invalid.
    match group.len() {
        0 => {}
        1 => return Err(corrupt),
        n => {
            let padded: Vec<u32> = group.iter().copied().chain(std::iter::repeat(84)).take(5).collect();
            out.extend(base85(&padded).ok_or(corrupt)?.into_iter().take(n - 1));
        }
    }
    Ok(out)
}

/// Four bytes from five base-85 digits, or `None` when they exceed 32 bits.
fn base85(digits: &[u32]) -> Option<[u8; 4]> {
    let value = digits
        .iter()
        .try_fold(0_u32, |acc, &d| acc.checked_mul(85)?.checked_add(d))?;
    Some(value.to_be_bytes())
}

/// 7.4.5: a length byte n, then n + 1 literal bytes (n < 128) or one byte repeated 257 - n times; 128 ends the data.
fn run_length(data: &[u8], budget: usize) -> Result<Vec<u8>, Damage> {
    let corrupt = Damage::Corrupt(Filter::RunLength);
    let mut out = Vec::new();
    let mut rest = data;
    loop {
        match rest {
            [128, ..] => return Ok(out),
            [n @ 0..=127, tail @ ..] => {
                let count = usize::from(*n) + 1;
                out.extend(tail.get(..count).ok_or(corrupt)?);
                rest = tail.get(count..).unwrap_or_default();
            }
            [n, byte, tail @ ..] => {
                out.extend(std::iter::repeat_n(*byte, 257 - usize::from(*n)));
                rest = tail;
            }
            _ => return Err(corrupt),
        }
        if out.len() > budget {
            return Err(Damage::OverBudget(budget));
        }
    }
}

/// 7.4.4: LZW with 9- to 12-bit codes, 256 clears the table, 257 ends the data. `early` is `EarlyChange`. With the
/// data, the number of bytes up to the end-of-data code, when there was one.
fn lzw(data: &[u8], early: bool, budget: usize) -> Result<(Vec<u8>, Option<usize>), Damage> {
    let corrupt = Damage::Corrupt(Filter::Lzw);
    let mut table: Vec<Vec<u8>> = Vec::new();
    let reset = |table: &mut Vec<Vec<u8>>| {
        table.clear();
        table.extend((0..=255_u8).map(|b| vec![b]));
        table.extend([Vec::new(), Vec::new()]);
    };
    reset(&mut table);
    let mut out = Vec::new();
    let mut previous: Option<Vec<u8>> = None;
    let (mut bits, mut held, mut pos) = (0_u32, 0_u32, 0_usize);
    loop {
        let width = match table.len() + usize::from(early) {
            0..=511 => 9,
            512..=1023 => 10,
            1024..=2047 => 11,
            _ => 12,
        };
        while bits < width {
            let Some(&byte) = data.get(pos) else {
                return Ok((out, None));
            };
            held = held << 8 | u32::from(byte);
            bits += 8;
            pos += 1;
        }
        let code = (held >> (bits - width)) & ((1 << width) - 1);
        bits -= width;
        held &= (1 << bits) - 1;
        // At most 12 bits, so it fits; anything else would miss the table below and be corrupt.
        let code = usize::try_from(code).unwrap_or(usize::MAX);
        match code {
            256 => {
                reset(&mut table);
                previous = None;
                continue;
            }
            257 => return Ok((out, Some(pos))),
            _ => {}
        }
        let entry = match (table.get(code), &previous) {
            (Some(entry), _) => entry.clone(),
            // The one code not yet in the table: the previous entry plus its own first byte.
            (None, Some(prev)) if code == table.len() => prev.iter().chain(prev.first()).copied().collect(),
            _ => return Err(corrupt),
        };
        if let Some(prev) = previous.take()
            && table.len() < 4096
        {
            table.push(prev.into_iter().chain(entry.first().copied()).collect());
        }
        out.extend(&entry);
        if out.len() > budget {
            return Err(Damage::OverBudget(budget));
        }
        previous = Some(entry);
    }
}

/// How many bytes of `data` the filter reads, up to and including its end-of-data marker, decoding within `budget`;
/// `None` when the data has no such marker or the filter is an image filter, whose end cannot be found this way. This
/// measures an inline image without `/L` (ISO 32000-2, 8.9.7), where the first filter's data ends the image.
pub(crate) fn encoded_len(data: &[u8], filter: Filter, early_change: bool, budget: usize) -> Option<usize> {
    match filter {
        Filter::AsciiHex => data.iter().position(|&b| b == b'>').map(|i| i + 1),
        Filter::Ascii85 => data.windows(2).position(|w| w == b"~>").map(|i| i + 2),
        Filter::RunLength => run_length_len(data),
        Filter::Lzw => lzw(data, early_change, budget).ok()?.1,
        Filter::Flate => flate_len(data, budget),
        Filter::Image | Filter::Crypt => None,
    }
}

/// CCITT fax parameters (ISO 32000-2, 7.4.6, Table 11) as a decoder needs them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fax {
    /// `K`: below 0 is Group 4, 0 is Group 3 one-dimensional, above 0 Group 3 two-dimensional.
    pub(crate) k: i64,
    pub(crate) columns: u32,
    pub(crate) rows: u32,
    pub(crate) end_of_block: bool,
    pub(crate) byte_align: bool,
    pub(crate) end_of_line: bool,
}

/// The bytes of CCITT fax data the decoder reads to fill `fax.rows` rows or reach its end-of-block code.
pub(crate) fn fax_len(data: &[u8], fax: &Fax) -> Option<usize> {
    use hayro_ccitt::{DecodeSettings, Decoder, DecoderContext, EncodingMode};
    struct Rows;
    impl Decoder for Rows {
        fn push_row(&mut self, _: &[u32]) {}
    }
    let encoding = match fax.k {
        ..0 => EncodingMode::Group4,
        0 => EncodingMode::Group3_1D,
        _ => EncodingMode::Group3_2D,
    };
    let settings = DecodeSettings {
        columns: fax.columns,
        rows: fax.rows,
        end_of_block: fax.end_of_block,
        end_of_line: fax.end_of_line,
        rows_are_byte_aligned: fax.byte_align,
        encoding,
        strict: true,
    };
    hayro_ccitt::decode(data, &mut Rows, &mut DecoderContext::new(settings))
        .ok()
        .filter(|&n| n > 0)
}

/// The bytes of a JPEG (ITU-T T.81) up to and including its end-of-image marker, found by walking its segments; the
/// entropy-coded data after a start-of-scan runs to the next marker that is not a restart or a stuffed byte.
pub(crate) fn dct_len(data: &[u8]) -> Option<usize> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2;
    loop {
        // Fill bytes (0xFF) may come before a marker.
        while data.get(at + 1) == Some(&0xFF) && data.get(at) == Some(&0xFF) {
            at += 1;
        }
        let (0xFF, marker) = (*data.get(at)?, *data.get(at + 1)?) else {
            return None;
        };
        match marker {
            0xD9 => return Some(at + 2),
            0x01 | 0xD0..=0xD7 => at += 2,
            _ => {
                let len = usize::from(u16::from_be_bytes([*data.get(at + 2)?, *data.get(at + 3)?]));
                at += 2 + len;
                if marker == 0xDA {
                    at += data
                        .get(at..)?
                        .windows(2)
                        .position(|w| matches!(w, [0xFF, b] if !matches!(b, 0x00 | 0xD0..=0xD7)))?;
                }
            }
        }
    }
}

/// The bytes of run-length data up to and including its end-of-data byte, 128.
fn run_length_len(data: &[u8]) -> Option<usize> {
    let mut at = 0;
    loop {
        match *data.get(at)? {
            128 => return Some(at + 1),
            n @ 0..=127 => at += usize::from(n) + 2,
            _ => at += 2,
        }
    }
}

/// The bytes of zlib data up to the end of its deflate stream, inflating at most `budget` bytes on the way.
fn flate_len(data: &[u8], budget: usize) -> Option<usize> {
    let mut state = InflateState::new_boxed(DataFormat::Zlib);
    let mut out = vec![0; 32 * 1024];
    let (mut consumed, mut written) = (0, 0_usize);
    loop {
        let result = inflate(
            &mut state,
            data.get(consumed..).unwrap_or_default(),
            &mut out,
            MZFlush::None,
        );
        consumed += result.bytes_consumed;
        written = written.saturating_add(result.bytes_written);
        match result.status {
            _ if written > budget => return None,
            Ok(MZStatus::StreamEnd) => return Some(consumed),
            Ok(_) if result.bytes_consumed + result.bytes_written > 0 => {}
            _ => return None,
        }
    }
}

/// 7.4.4.4: undo a TIFF (2) or PNG (10 to 15) predictor; 1 means none.
fn predict(data: Vec<u8>, p: &Params) -> Result<Vec<u8>, Damage> {
    if p.predictor == 1 {
        return Ok(data);
    }
    let bad = Damage::BadParams;
    let colors = usize::try_from(p.colors)
        .ok()
        .filter(|&c| (1..=32).contains(&c))
        .ok_or(bad)?;
    let bits = usize::try_from(p.bits_per_component)
        .ok()
        .filter(|b| [1, 2, 4, 8, 16].contains(b))
        .ok_or(bad)?;
    let columns = usize::try_from(p.columns).ok().filter(|&c| c >= 1).ok_or(bad)?;
    let row = (colors * bits * columns).div_ceil(8);
    let pixel = (colors * bits).div_ceil(8);
    match p.predictor {
        2 => Ok(tiff(data, row, colors, bits)),
        10..=15 => png(&data, row, pixel),
        _ => Err(bad),
    }
}

/// TIFF predictor 2: each sample is the difference from the same sample of the pixel to its left, any bit depth.
fn tiff(mut data: Vec<u8>, row: usize, colors: usize, bits: usize) -> Vec<u8> {
    let mask = (1_u32 << bits) - 1;
    for line in data.chunks_mut(row) {
        let samples = line.len() * 8 / bits;
        for i in colors..samples {
            let value = (sample(line, i, bits) + sample(line, i - colors, bits)) & mask;
            set_sample(line, i, bits, value);
        }
    }
    data
}

fn sample(line: &[u8], i: usize, bits: usize) -> u32 {
    if bits == 16 {
        let at = i * 2;
        return u32::from(line.get(at).copied().unwrap_or_default()) << 8
            | u32::from(line.get(at + 1).copied().unwrap_or_default());
    }
    let bit = i * bits;
    let byte = u32::from(line.get(bit / 8).copied().unwrap_or_default());
    byte >> (8 - bits - bit % 8) & ((1 << bits) - 1)
}

fn set_sample(line: &mut [u8], i: usize, bits: usize, value: u32) {
    let bytes = value.to_be_bytes();
    if bits == 16 {
        for (slot, b) in line.iter_mut().skip(i * 2).take(2).zip(bytes.iter().skip(2)) {
            *slot = *b;
        }
        return;
    }
    let bit = i * bits;
    let shift = 8 - bits - bit % 8;
    // Both fit in the byte: the sample is masked to its width and shifted into place.
    let mask = (((1_u32 << bits) - 1) << shift).to_le_bytes()[0];
    let value = (value << shift).to_le_bytes()[0];
    for byte in line.iter_mut().skip(bit / 8).take(1) {
        *byte = (*byte & !mask) | (value & mask);
    }
}

/// PNG predictors: every row starts with its own filter byte, 0 to 4.
fn png(data: &[u8], row: usize, pixel: usize) -> Result<Vec<u8>, Damage> {
    let mut out: Vec<u8> = Vec::with_capacity(data.len());
    let mut above = vec![0_u8; row];
    for line in data.chunks(row + 1) {
        // `chunks` never yields an empty line, so the filter byte is always there.
        let kind = line.first().copied().unwrap_or_default();
        let raw = line.get(1..).unwrap_or_default();
        let mut current: Vec<u8> = Vec::with_capacity(raw.len());
        for (i, &x) in raw.iter().enumerate() {
            let left = i
                .checked_sub(pixel)
                .and_then(|j| current.get(j))
                .copied()
                .unwrap_or_default();
            let up = above.get(i).copied().unwrap_or_default();
            let up_left = i
                .checked_sub(pixel)
                .and_then(|j| above.get(j))
                .copied()
                .unwrap_or_default();
            let predicted = match kind {
                0 => 0,
                1 => left,
                2 => up,
                3 => u16::midpoint(u16::from(left), u16::from(up)).to_le_bytes()[0],
                4 => paeth(left, up, up_left),
                _ => return Err(Damage::BadParams),
            };
            current.push(x.wrapping_add(predicted));
        }
        out.extend(&current);
        above = current;
    }
    Ok(out)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniz_oxide::deflate::compress_to_vec_zlib;
    use proptest::prelude::*;

    /// The bytes a decode gives, plain or stopped at an image filter.
    fn plain(data: &[u8], chain: &[(Filter, Params)]) -> Result<Vec<u8>, Damage> {
        match decode(data, chain, 1 << 20)? {
            Decoded::Plain(v) | Decoded::Encoded(v) => Ok(v),
        }
    }

    fn one(filter: Filter) -> [(Filter, Params); 1] {
        [(filter, Params::default())]
    }

    #[test]
    fn names() {
        let names: [&[u8]; 13] = [
            b"ASCIIHexDecode",
            b"AHx",
            b"ASCII85Decode",
            b"A85",
            b"LZWDecode",
            b"LZW",
            b"FlateDecode",
            b"Fl",
            b"RunLengthDecode",
            b"RL",
            b"DCTDecode",
            b"Crypt",
            b"JPXDecode",
        ];
        let found: Vec<_> = names.iter().map(|n| Filter::from_name(n)).collect();
        assert!(found.iter().all(Option::is_some));
        assert_eq!(Filter::from_name(b"Crypt"), Some(Filter::Crypt));
        assert_eq!(Filter::from_name(b"JPXDecode"), Some(Filter::Image));
        assert_eq!(Filter::from_name(b"Rot13Decode"), None);
    }

    #[test]
    fn empty_flate_is_empty() {
        assert_eq!(flate(&[], 10), Ok(Vec::new()));
        assert_eq!(flate(&[1], 10), Err(Damage::Corrupt(Filter::Flate)));
    }

    #[test]
    fn flate_and_a_chain() {
        let zipped = compress_to_vec_zlib(b"hello hello hello", 6);
        assert_eq!(plain(&zipped, &one(Filter::Flate)), Ok(b"hello hello hello".to_vec()));
        let hex: Vec<u8> = zipped
            .iter()
            .flat_map(|b| format!("{b:02x}").into_bytes())
            .chain(*b">")
            .collect();
        let chain = [
            (Filter::AsciiHex, Params::default()),
            (Filter::Flate, Params::default()),
        ];
        assert_eq!(plain(&hex, &chain), Ok(b"hello hello hello".to_vec()));
        assert_eq!(
            plain(b"not zlib", &one(Filter::Flate)),
            Err(Damage::Corrupt(Filter::Flate))
        );
        assert_eq!(
            decode(&compress_to_vec_zlib(&[0; 100], 6), &one(Filter::Flate), 10),
            Err(Damage::OverBudget(10))
        );
    }

    #[test]
    fn image_filters_stay_encoded_and_crypt_refuses() {
        let chain = [
            (Filter::AsciiHex, Params::default()),
            (Filter::Image, Params::default()),
        ];
        assert_eq!(decode(b"4142>", &chain, 100), Ok(Decoded::Encoded(b"AB".to_vec())));
        assert_eq!(decode(b"x", &one(Filter::Crypt), 100), Err(Damage::Encrypted));
        assert_eq!(
            decode(b"41424344>", &one(Filter::AsciiHex), 3),
            Err(Damage::OverBudget(3))
        );
    }

    #[test]
    fn ascii_hex() {
        assert_eq!(plain(b"41 42\n4>trailing", &one(Filter::AsciiHex)), Ok(b"AB@".to_vec()));
        assert_eq!(
            plain(b"414", &one(Filter::AsciiHex)),
            Err(Damage::Corrupt(Filter::AsciiHex))
        );
        assert_eq!(
            plain(b"4G>", &one(Filter::AsciiHex)),
            Err(Damage::Corrupt(Filter::AsciiHex))
        );
    }

    #[test]
    fn ascii85() {
        assert_eq!(
            plain(b"87cURD]i,\"Ebo80~>", &one(Filter::Ascii85)),
            Ok(b"Hello World!".to_vec())
        );
        assert_eq!(plain(b"z87cUR~>", &one(Filter::Ascii85)), Ok(b"\0\0\0\0Hell".to_vec()));
        assert_eq!(plain(b"87cURD]i~>", &one(Filter::Ascii85)), Ok(b"Hello ".to_vec()));
        for bad in [
            &b"87cU"[..],
            b"8~>",
            b"87cURvvvvv~>",
            b"87z~>",
            b"s8W-\"~>",
            b"uuuuu~>",
            b"uuuu~>",
        ] {
            assert_eq!(
                plain(bad, &one(Filter::Ascii85)),
                Err(Damage::Corrupt(Filter::Ascii85)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn run_length() {
        assert_eq!(
            plain(&[2, b'a', b'b', b'c', 254, b'x', 128], &one(Filter::RunLength)),
            Ok(b"abcxxx".to_vec())
        );
        for bad in [&[2, b'a'][..], &[200], &[]] {
            assert_eq!(
                plain(bad, &one(Filter::RunLength)),
                Err(Damage::Corrupt(Filter::RunLength)),
                "{bad:?}"
            );
        }
        assert_eq!(
            decode(&[129, b'x', 128], &one(Filter::RunLength), 5),
            Err(Damage::OverBudget(5))
        );
    }

    /// LZW-encodes `codes` at the widths the decoder expects, with `EarlyChange`.
    fn lzw_codes(codes: &[(u32, u32)]) -> Vec<u8> {
        let (mut out, mut held, mut bits) = (Vec::new(), 0_u64, 0_u32);
        for &(code, width) in codes {
            held = held << width | u64::from(code);
            bits += width;
            while bits >= 8 {
                out.push((held >> (bits - 8)).to_le_bytes()[0]);
                bits -= 8;
            }
        }
        if bits > 0 {
            out.push((held << (8 - bits)).to_le_bytes()[0]);
        }
        out
    }

    #[test]
    fn lzw() {
        // The example of 7.4.4.2: 45 45 45 45 45 65 45 45 45 66.
        let data = [0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01];
        assert_eq!(
            plain(&data, &one(Filter::Lzw)),
            Ok(vec![45, 45, 45, 45, 45, 65, 45, 45, 45, 66])
        );
        let cleared = lzw_codes(&[(65, 9), (256, 9), (66, 9), (257, 9)]);
        assert_eq!(plain(&cleared, &one(Filter::Lzw)), Ok(b"AB".to_vec()));
        assert_eq!(plain(&lzw_codes(&[(65, 9)]), &one(Filter::Lzw)), Ok(b"A".to_vec()));
        assert_eq!(
            plain(&lzw_codes(&[(300, 9)]), &one(Filter::Lzw)),
            Err(Damage::Corrupt(Filter::Lzw))
        );
        // 600 literal codes grow the table past 512 entries (10-bit codes) without EarlyChange too.
        let late = Params {
            early_change: 0,
            ..Params::default()
        };
        let mut codes: Vec<(u32, u32)> = (0..600).map(|i| (i % 256, if i < 255 { 9 } else { 10 })).collect();
        codes.push((257, 10));
        assert_eq!(
            plain(&lzw_codes(&codes), &[(Filter::Lzw, late)]).map(|v| v.len()),
            Ok(600)
        );
        assert_eq!(
            decode(&lzw_codes(&[(65, 9), (65, 9), (65, 9)]), &one(Filter::Lzw), 2),
            Err(Damage::OverBudget(2))
        );
    }

    #[test]
    fn lzw_widths_up_to_12_bits() {
        let mut codes: Vec<(u32, u32)> = Vec::new();
        for i in 0..3900_u32 {
            // The decoder sizes code i by its table before reading it, 258 + (i - 1) entries, plus one for EarlyChange.
            let width = match 258 + i.saturating_sub(1) + 1 {
                0..=511 => 9,
                512..=1023 => 10,
                1024..=2047 => 11,
                _ => 12,
            };
            codes.push((i % 256, width));
        }
        codes.push((257, 12));
        assert_eq!(plain(&lzw_codes(&codes), &one(Filter::Lzw)).map(|v| v.len()), Ok(3900));
    }

    fn png(predictor: i64, columns: i64) -> Params {
        Params {
            predictor,
            columns,
            ..Params::default()
        }
    }

    #[test]
    fn png_predictors() {
        // Two rows of three bytes, one per PNG filter type in turn.
        let rows = [0_u8, 1, 2, 3, 1, 1, 1, 1, 2, 1, 1, 1, 3, 2, 2, 2, 4, 1, 1, 1];
        let zipped = compress_to_vec_zlib(&rows, 6);
        let out = plain(&zipped, &[(Filter::Flate, png(12, 3))]).unwrap();
        assert_eq!(out, [1, 2, 3, 1, 2, 3, 2, 3, 4, 3, 5, 6, 4, 6, 7]);
        let bad = compress_to_vec_zlib(&[5, 0, 0, 0], 6);
        assert_eq!(plain(&bad, &[(Filter::Flate, png(12, 3))]), Err(Damage::BadParams));
        assert_eq!(paeth(10, 20, 15), 15);
    }

    #[test]
    fn tiff_predictor_at_every_depth() {
        let tiff = |data: &[u8], colors, bits, columns| {
            let p = Params {
                predictor: 2,
                colors,
                bits_per_component: bits,
                columns,
                ..Params::default()
            };
            plain(&compress_to_vec_zlib(data, 6), &[(Filter::Flate, p)]).unwrap()
        };
        assert_eq!(tiff(&[1, 1, 1, 5, 1, 1], 1, 8, 3), [1, 2, 3, 5, 6, 7]);
        assert_eq!(tiff(&[0b0101_0000], 1, 1, 4), [0b0110_0000]);
        assert_eq!(tiff(&[0x11, 0x11], 1, 4, 4), [0x12, 0x34]);
        assert_eq!(tiff(&[0b0101_0101], 1, 2, 4), [0b0110_1100]);
        assert_eq!(tiff(&[0, 1, 0, 2], 1, 16, 2), [0, 1, 0, 3]);
    }

    #[test]
    fn bad_predictor_parameters() {
        let zipped = compress_to_vec_zlib(b"ab", 6);
        for p in [
            Params {
                predictor: 7,
                ..Params::default()
            },
            Params {
                predictor: 2,
                colors: 0,
                ..Params::default()
            },
            Params {
                predictor: 2,
                bits_per_component: 3,
                ..Params::default()
            },
            Params {
                predictor: 2,
                columns: 0,
                ..Params::default()
            },
        ] {
            assert_eq!(plain(&zipped, &[(Filter::Flate, p)]), Err(Damage::BadParams), "{p:?}");
        }
        assert_eq!(
            plain(&compress_to_vec_zlib(&[], 6), &[(Filter::Flate, png(10, 1))]),
            Ok(vec![])
        );
    }

    proptest! {
        #[test]
        fn never_panics(data in proptest::collection::vec(any::<u8>(), 0..300), which in 0_usize..7, predictor in prop_oneof![Just(1_i64), Just(2), Just(12)], bits in prop_oneof![Just(1_i64), Just(8), Just(16)]) {
            let filters = [Filter::AsciiHex, Filter::Ascii85, Filter::Lzw, Filter::Flate, Filter::RunLength, Filter::Image, Filter::Crypt];
            let params = Params { predictor, bits_per_component: bits, columns: 3, ..Params::default() };
            let _ = decode(&data, &[(filters[which], params)], 4096);
        }
    }

    #[test]
    fn upper_case_hex_and_a_predictor_after_lzw() {
        assert_eq!(plain(b"4A4b>", &one(Filter::AsciiHex)), Ok(b"JK".to_vec()));
        let bad_predictor = Params {
            predictor: 7,
            ..Params::default()
        };
        assert_eq!(
            plain(&lzw_codes(&[(65, 9)]), &[(Filter::Lzw, bad_predictor)]),
            Err(Damage::BadParams)
        );
        assert_eq!(
            plain(
                b"4142>",
                &[
                    (Filter::AsciiHex, Params::default()),
                    (Filter::Image, Params::default())
                ]
            ),
            Ok(b"AB".to_vec())
        );
    }

    #[test]
    fn lzw_codes_that_end_on_a_byte() {
        // Eight 9-bit codes fill exactly nine bytes.
        let codes = [(65, 9), (66, 9), (67, 9), (68, 9), (69, 9), (70, 9), (71, 9), (257, 9)];
        assert_eq!(lzw_codes(&codes).len(), 9);
        assert_eq!(plain(&lzw_codes(&codes), &one(Filter::Lzw)), Ok(b"ABCDEFG".to_vec()));
    }

    #[test]
    fn encoded_lengths_end_at_the_end_of_data_marker() {
        let tail = b" EI Q";
        let with = |data: &[u8]| [data, tail].concat();
        assert_eq!(encoded_len(&with(b"68 69>"), Filter::AsciiHex, true, 9), Some(6));
        assert_eq!(encoded_len(b"6869", Filter::AsciiHex, true, 9), None);
        assert_eq!(
            encoded_len(&with(b"BOu!rD]j7BEbo80~>"), Filter::Ascii85, true, 9),
            Some(17)
        );
        assert_eq!(
            encoded_len(&with(&[1, b'a', b'b', 255, b'c', 128]), Filter::RunLength, true, 9),
            Some(6)
        );
        assert_eq!(encoded_len(&[1, b'a'], Filter::RunLength, true, 9), None);
        let flate = compress_to_vec_zlib(b"hello hello hello", 6);
        assert_eq!(encoded_len(&with(&flate), Filter::Flate, true, 99), Some(flate.len()));
        assert_eq!(
            encoded_len(flate.get(..4).unwrap(), Filter::Flate, true, 99),
            None,
            "cut short"
        );
        assert_eq!(encoded_len(&flate, Filter::Flate, true, 3), None, "over budget");
        assert_eq!(encoded_len(b"\x00\x00", Filter::Flate, true, 99), None, "not zlib");
        // LZW: a clear code, "A" (65) and the end-of-data code are 27 bits in 9-bit codes, so 4 bytes; then two more.
        let lzw_data = [0x80, 0x10, 0x60, 0x20, 0xFF, 0xFF];
        assert_eq!(encoded_len(&lzw_data, Filter::Lzw, true, 9), Some(4));
        assert_eq!(encoded_len(lzw_data.get(..2).unwrap(), Filter::Lzw, true, 9), None);
        assert_eq!(
            encoded_len(&[0xFF, 0xFF, 0xFF], Filter::Lzw, true, 9),
            None,
            "a code not in the table"
        );
        assert_eq!(encoded_len(b"x", Filter::Image, true, 9), None);
        assert_eq!(encoded_len(b"x", Filter::Crypt, true, 9), None);
    }

    #[test]
    fn a_jpeg_ends_at_its_end_of_image_marker() {
        // SOI, an APP0 segment of 2 payload bytes, a fill byte, SOS (header of 2), entropy data with a stuffed 0xFF and
        // a restart marker, a standalone TEM marker, EOI; then whatever follows.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 1, 2, 0xFF, 0xFF, 0xDA, 0x00, 0x02, 7, 0xFF, 0x00, 8, 0xFF, 0xD0, 9,
            0xFF, 0x01, 0xFF, 0xD9, b'E', b'I',
        ];
        assert_eq!(dct_len(&jpeg), Some(24));
        assert_eq!(dct_len(b"JFIF"), None, "no start-of-image marker");
        assert_eq!(dct_len(jpeg.get(..12).unwrap()), None, "cut short in the scan header");
        assert_eq!(dct_len(&[0xFF, 0xD8, 0x00]), None, "not a marker");
        assert_eq!(
            dct_len(&[0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 1, 2]),
            None,
            "a scan with no marker after it"
        );
        assert_eq!(dct_len(&[0xFF, 0xD8]), None, "nothing after the start");
        assert_eq!(
            dct_len(&[0xFF, 0xD8, 0x00, 0x00]),
            None,
            "a byte where a marker should be"
        );
        assert_eq!(dct_len(&[0xFF, 0xD8, 0xFF, 0xE0]), None, "a segment with no length");
        assert_eq!(
            dct_len(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00]),
            None,
            "a segment length cut short"
        );
        assert_eq!(
            dct_len(&[0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x09]),
            None,
            "a scan header longer than the data"
        );
    }

    #[test]
    fn fax_data_ends_where_its_rows_do() {
        // Group 4: an all-white row against an all-white reference is one V0 code, a single 1 bit; 8 rows, 1 byte.
        let g4 = Fax {
            k: -1,
            columns: 8,
            rows: 8,
            end_of_block: true,
            byte_align: false,
            end_of_line: false,
        };
        // Then the end-of-block code, two EOLs (000000000001 twice), is part of the data too.
        assert_eq!(fax_len(&[0xFF, 0x00, 0x10, 0x01, b'E', b'I'], &g4), Some(4));
        assert_eq!(
            fax_len(
                &[0xFF, b' ', b'E', b'I'],
                &Fax {
                    end_of_block: false,
                    ..g4
                }
            ),
            Some(1)
        );
        assert_eq!(
            fax_len(&[0xFF], &Fax { columns: 0, ..g4 }),
            None,
            "no columns, nothing read"
        );
        assert_eq!(fax_len(&[0x00, 0x00], &Fax { k: 0, ..g4 }), None);
        assert_eq!(fax_len(&[0x00, 0x00], &Fax { k: 2, ..g4 }), None);
    }
}
