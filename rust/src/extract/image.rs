//! An image `XObject` written out as a file of its own.
//!
//! Images come out as they are stored where a file format holds that data: JPEG (`.jpg`) and JPEG 2000 (`.jp2`) as
//! their bytes, CCITT fax data behind a TIFF header (`.tif`). Anything else is decoded into samples, `/Decode`
//! applied, and written as a PNG (gray, 16-bit gray or RGB) or, for CMYK, a Deflate-compressed TIFF, with an
//! `ICCBased` space's profile embedded; JBIG2 data is decoded to gray. An `Indexed` image comes out in its base
//! space. A soft mask is a file of its own, written the same way, never composited into the image, so nothing is
//! re-encoded. Lab, `Separation` and `DeviceN` images are not written: no common file format holds them unconverted.

use std::borrow::Cow;

use crate::document::{Dict, Document, Object, Value};

/// Why an image could not be written.
type Failure = String;

/// The filters that produce image samples themselves (ISO 32000-2 7.4.1, Table 6).
const IMAGE_FILTERS: [&[u8]; 4] = [b"DCTDecode", b"JPXDecode", b"CCITTFaxDecode", b"JBIG2Decode"];

/// Image `num` as a file: its extension (without the dot) and bytes.
pub(super) fn write(doc: &Document, num: u32) -> Result<(&'static str, Vec<u8>), Failure> {
    let dict = doc.stream_dict(num).ok_or("it is not a stream that can be read")?;
    let filters = filter_names(doc, dict);
    let Some(at) = filters.iter().position(|f| IMAGE_FILTERS.contains(&f.as_slice())) else {
        let data = doc.stream_data(num).ok_or("its data cannot be decoded")?;
        return encode(&raster(doc, dict, &data)?);
    };
    let data = doc.image_data(num).ok_or("its data cannot be decoded")?;
    match filters.get(at).map(Vec::as_slice) {
        Some(b"DCTDecode") => Ok(("jpg", data)),
        Some(b"JPXDecode") => Ok(("jp2", data)),
        Some(b"CCITTFaxDecode") => fax_tiff(doc, dict, at, &data).map(|tiff| ("tif", tiff)),
        _ => encode(&jbig2(doc, dict, &data)?),
    }
}

/// The names of a stream's filters, in order.
fn filter_names(doc: &Document, dict: &Dict) -> Vec<Vec<u8>> {
    let Some(filter) = dict.get(b"Filter").and_then(|f| doc.resolve(f)) else {
        return Vec::new();
    };
    match &filter.value {
        Value::Name(n) => vec![n.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(|i| doc.resolve(i)?.name().map(<[u8]>::to_vec))
            .collect(),
        _ => Vec::new(),
    }
}

fn integer(doc: &Document, dict: &Dict, key: &[u8]) -> Option<i64> {
    dict.get(key).and_then(|v| doc.resolve(v)).and_then(Object::integer)
}

#[expect(clippy::cast_precision_loss, reason = "a /Decode value, far below 2^53")]
fn numbers(doc: &Document, object: Option<&Object>) -> Option<Vec<f64>> {
    let items = doc.resolve(object?)?.array()?;
    items
        .iter()
        .map(|v| match doc.resolve(v)?.value {
            Value::Integer(i) => Some(i as f64),
            Value::Real(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// CCITT fax data behind a little-endian TIFF header, as pikepdf writes it: the fax parameters from the
/// `/DecodeParms` of the filter at `at`, white as 0 unless `/BlackIs1` (flipped by a `/Decode` of `[1 0]`), and an
/// `ICCBased` space's profile.
#[expect(clippy::cast_possible_truncation, reason = "lengths of at most a GiB")]
fn fax_tiff(doc: &Document, dict: &Dict, at: usize, data: &[u8]) -> Result<Vec<u8>, Failure> {
    let params = dict
        .get(b"DecodeParms")
        .and_then(|p| doc.resolve(p))
        .and_then(|p| match &p.value {
            Value::Array(items) => items.get(at).and_then(|i| doc.resolve(i)).and_then(Object::dict),
            Value::Dict(d) => Some(d),
            _ => None,
        });
    let param = |key: &[u8]| params.and_then(|p| p.get(key)).and_then(|v| doc.resolve(v));
    if param(b"EncodedByteAlign").map(|v| &v.value) == Some(&Value::Bool(true)) {
        return Err("CCITT data aligned to bytes has no TIFF form".to_owned());
    }
    let k = param(b"K").and_then(Object::integer).unwrap_or(0);
    let mut photometric = u32::from(param(b"BlackIs1").map(|v| &v.value) == Some(&Value::Bool(true)));
    if numbers(doc, dict.get(b"Decode")).as_deref() == Some(&[1.0, 0.0]) {
        photometric = 1 - photometric;
    }
    let size = |key: &[u8]| integer(doc, dict, key).and_then(|v| u32::try_from(v).ok());
    let (Some(width), Some(height)) = (size(b"Width"), size(b"Height")) else {
        return Err("its /Width or /Height is not a size".to_owned());
    };
    let icc = icc_profile(doc, dict.get(b"ColorSpace"));
    // Tag, type (3 SHORT, 4 LONG, 7 UNDEFINED), count, value, in pikepdf's order.
    let mut tags: Vec<(u16, u16, u32, u32)> = vec![(256, 4, 1, width), (257, 4, 1, height), (258, 3, 1, 1)];
    tags.push((259, 3, 1, if k < 0 { 4 } else { 3 }));
    tags.push((266, 3, 1, 1));
    if k > 0 {
        tags.push((292, 4, 1, 1));
    }
    tags.push((262, 3, 1, photometric));
    // A stream decodes to at most `Limits::decoded_bytes` (1 GiB), so every length fits a TIFF's 32 bits.
    let icc_len = icc.as_ref().map_or(0, Vec::len) as u32;
    let count = tags.len() + 3 + usize::from(icc.is_some());
    let header = (8 + 2 + 12 * count + 4) as u32;
    let data_len = data.len() as u32;
    tags.push((273, 4, 1, header + icc_len));
    tags.push((278, 4, 1, height));
    tags.push((279, 4, 1, data_len));
    if icc.is_some() {
        tags.push((34675, 7, icc_len, header));
    }
    let mut out = b"II*\0\x08\0\0\0".to_vec();
    out.extend((tags.len() as u16).to_le_bytes());
    for (tag, kind, count, value) in tags {
        out.extend(tag.to_le_bytes());
        out.extend(kind.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend(value.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes());
    out.extend(icc.unwrap_or_default());
    out.extend(data);
    Ok(out)
}

/// JBIG2 data (with its `/JBIG2Globals`) as gray samples, black 0, then `/Decode`.
fn jbig2(doc: &Document, dict: &Dict, data: &[u8]) -> Result<Raster, Failure> {
    use hayro_jbig2::{DecodeSettings, Decoder, Globals, Page};
    struct Gray(Vec<u8>);
    impl Decoder for Gray {
        fn push_pixel(&mut self, black: bool) {
            self.0.push(if black { 0 } else { 255 });
        }
        fn push_pixel_chunk(&mut self, black: bool, chunk_count: u32) {
            let n = 8 * chunk_count as usize;
            self.0.extend(std::iter::repeat_n(if black { 0 } else { 255 }, n));
        }
        fn next_line(&mut self) {}
    }
    let settings = DecodeSettings::default();
    let globals = dict
        .get(b"DecodeParms")
        .and_then(|p| doc.resolve(p))
        .and_then(|p| match &p.value {
            Value::Array(items) => items
                .iter()
                .filter_map(|i| doc.resolve(i).and_then(Object::dict))
                .find_map(|d| d.get(b"JBIG2Globals")),
            Value::Dict(d) => d.get(b"JBIG2Globals"),
            _ => None,
        })
        .and_then(|g| match g.value {
            Value::Reference(num, _) => doc.stream_data(num),
            _ => None,
        });
    let globals = globals.map(|g| Globals::new(&g, &settings)).transpose().ok().flatten();
    let mut gray = Gray(Vec::new());
    let page = Page::new_embedded(data, globals.as_ref(), &settings)
        .ok()
        .filter(|page| page.decode(&mut gray).is_ok())
        .ok_or("its JBIG2 data cannot be decoded")?;
    // The image is /Width by /Height, whatever size the JBIG2 page gives: its rows are cut or padded white to fit.
    let size = |key: &[u8]| {
        integer(doc, dict, key)
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v > 0)
    };
    let width = size(b"Width").unwrap_or(page.width());
    let height = size(b"Height").unwrap_or(page.height());
    let stride = page.width() as usize;
    let mut samples: Vec<u8> = (0..height as usize)
        .flat_map(|y| (0..width as usize).map(move |x| (y, x)))
        .map(|(y, x)| {
            if x < stride {
                gray.0.get(y * stride + x).copied().unwrap_or(255)
            } else {
                255
            }
        })
        .collect();
    decode_array(doc, dict, Kind::Gray, false, &mut samples);
    Ok(Raster {
        kind: Kind::Gray,
        width,
        height,
        wide: false,
        samples,
        icc: None,
    })
}

/// The colour a decoded image's samples are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Gray,
    Rgb,
    Cmyk,
}

impl Kind {
    fn channels(self) -> usize {
        match self {
            Kind::Gray => 1,
            Kind::Rgb => 3,
            Kind::Cmyk => 4,
        }
    }
}

/// An image's colour space, as far as writing it goes.
enum Space {
    Device(Kind),
    /// A palette of base colours, `lookup` holding each in turn.
    Indexed(Kind, Vec<u8>),
}

/// Decoded samples: 8 bits each, or 16 big-endian for 16-bit gray.
struct Raster {
    kind: Kind,
    width: u32,
    height: u32,
    wide: bool,
    samples: Vec<u8>,
    icc: Option<Vec<u8>>,
}

/// The profile of an `ICCBased` space, or of an `Indexed` space's `ICCBased` base.
fn icc_profile(doc: &Document, space: Option<&Object>) -> Option<Vec<u8>> {
    let items = space
        .and_then(|s| doc.resolve(s))
        .and_then(Object::array)
        .unwrap_or_default();
    match items.first().and_then(|n| doc.resolve(n)).and_then(Object::name) {
        Some(b"ICCBased") => items.get(1).and_then(|s| match s.value {
            Value::Reference(num, _) => doc.stream_data(num),
            _ => None,
        }),
        Some(b"Indexed") => icc_profile(doc, items.get(1)),
        _ => None,
    }
}

/// The base colour of a device space, a `Cal` space or an `ICCBased` one with 1, 3 or 4 components.
fn kind(doc: &Document, space: &Object) -> Result<Kind, Failure> {
    let space = doc.resolve(space).ok_or("its colour space cannot be read")?;
    let name = match &space.value {
        Value::Name(n) => n.as_slice(),
        Value::Array(items) => items
            .first()
            .and_then(|n| doc.resolve(n))
            .and_then(Object::name)
            .unwrap_or_default(),
        _ => b"",
    };
    match name {
        b"DeviceGray" | b"CalGray" | b"G" => Ok(Kind::Gray),
        b"DeviceRGB" | b"CalRGB" | b"RGB" => Ok(Kind::Rgb),
        b"DeviceCMYK" | b"CMYK" => Ok(Kind::Cmyk),
        b"ICCBased" => {
            let n = space
                .array()
                .and_then(|items| items.get(1))
                .and_then(|s| match s.value {
                    Value::Reference(num, _) => doc.stream_dict(num),
                    _ => None,
                })
                .and_then(|d| integer(doc, d, b"N"));
            match n {
                Some(1) => Ok(Kind::Gray),
                Some(3) => Ok(Kind::Rgb),
                Some(4) => Ok(Kind::Cmyk),
                _ => Err("its ICC profile's /N is not 1, 3 or 4".to_owned()),
            }
        }
        _ => Err(format!(
            "its colour space {} has no file format of its own",
            if name.is_empty() {
                "?".into()
            } else {
                String::from_utf8_lossy(name)
            }
        )),
    }
}

fn space(doc: &Document, dict: &Dict) -> Result<Space, Failure> {
    let object = dict.get(b"ColorSpace").ok_or("it has no colour space")?;
    let resolved = doc.resolve(object).ok_or("its colour space cannot be read")?;
    if let Some([head, base, _, lookup]) = resolved.array()
        && doc.resolve(head).and_then(Object::name) == Some(b"Indexed")
    {
        let base = kind(doc, base)?;
        let lookup = match doc.resolve(lookup).map(|l| &l.value) {
            Some(Value::String(bytes, _)) => bytes.clone(),
            _ => match lookup.value {
                Value::Reference(num, _) => doc.stream_data(num),
                _ => None,
            }
            .ok_or("its palette cannot be read")?,
        };
        return Ok(Space::Indexed(base, lookup));
    }
    kind(doc, object).map(Space::Device)
}

/// Python's `round()`: to the nearest integer, ties to even.
fn round_even(x: f64) -> f64 {
    x.round_ties_even()
}

/// `dict`'s image as decoded samples, from its decoded stream `data`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "samples of at most 16 bits, scaled to their width"
)]
fn raster(doc: &Document, dict: &Dict, data: &[u8]) -> Result<Raster, Failure> {
    let size = |key: &[u8]| {
        integer(doc, dict, key)
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v > 0)
    };
    let (Some(width), Some(height)) = (size(b"Width"), size(b"Height")) else {
        return Err("its /Width or /Height is not a size".to_owned());
    };
    let stencil = dict.get(b"ImageMask").and_then(|m| doc.resolve(m)).map(|m| &m.value) == Some(&Value::Bool(true));
    let (space, bits) = if stencil {
        (Space::Device(Kind::Gray), 1)
    } else {
        (space(doc, dict)?, integer(doc, dict, b"BitsPerComponent").unwrap_or(8))
    };
    let bits = u32::try_from(bits)
        .ok()
        .filter(|b| [1, 2, 4, 8, 16].contains(b))
        .ok_or("its /BitsPerComponent is not 1, 2, 4, 8 or 16")?;
    let (kind, palette) = match &space {
        Space::Device(kind) => (*kind, None),
        Space::Indexed(kind, lookup) => (*kind, Some(lookup)),
    };
    if palette.is_some() && bits == 16 {
        return Err("a 16-bit indexed image is not allowed".to_owned());
    }
    let per_pixel = if palette.is_some() { 1 } else { kind.channels() };
    let (w, h) = (width as usize, height as usize);
    let row = (w * per_pixel * bits as usize).div_ceil(8);
    if data.len() < row * h {
        return Err(format!(
            "its data is {} bytes, short of the {} its size needs",
            data.len(),
            row * h
        ));
    }
    let max = (1u32 << bits) - 1;
    // Each sample's value, row by row; rows start on a byte. The data is long enough, checked above.
    let byte = |at: usize| u32::from(data.get(at).copied().unwrap_or(0));
    let sample = |y: usize, i: usize| -> u32 {
        let start = y * row;
        match bits {
            16 => (byte(start + 2 * i) << 8) | byte(start + 2 * i + 1),
            8 => byte(start + i),
            _ => {
                let bit = i * bits as usize;
                (byte(start + bit / 8) >> (8 - bits as usize - bit % 8)) & max
            }
        }
    };
    let wide = kind == Kind::Gray && bits == 16 && palette.is_none();
    let mut samples = Vec::with_capacity(w * h * kind.channels() * if wide { 2 } else { 1 });
    for y in 0..h {
        for i in 0..w * per_pixel {
            let v = sample(y, i);
            match palette {
                Some(lookup) => {
                    let n = kind.channels();
                    let at = v as usize * n;
                    samples.extend((0..n).map(|c| lookup.get(at + c).copied().unwrap_or(0)));
                }
                None if wide => samples.extend((v as u16).to_be_bytes()),
                None if bits == 16 => samples.push((v >> 8) as u8),
                None => samples.push((v * 255 / max) as u8),
            }
        }
    }
    if palette.is_none() {
        decode_array(doc, dict, kind, wide, &mut samples);
    }
    let icc = icc_profile(doc, dict.get(b"ColorSpace")).filter(|_| !stencil);
    Ok(Raster {
        kind,
        width,
        height,
        wide,
        samples,
        icc,
    })
}

/// Apply the image's `/Decode` to its 8-bit samples, as pikepdf does: each band through
/// `round(clamp(dmin + p / 255 * (dmax - dmin), 0, 1) * 255)`, rounding ties to even. 16-bit gray takes only `[1 0]`,
/// an inversion; an array of the wrong length is ignored.
fn decode_array(doc: &Document, dict: &Dict, kind: Kind, wide: bool, samples: &mut [u8]) {
    let Some(decode) = numbers(doc, dict.get(b"Decode")) else {
        return;
    };
    let n = kind.channels();
    if decode.len() != 2 * n || decode.chunks(2).all(|d| d == [0.0, 1.0]) {
        return;
    }
    if wide {
        if decode == [1.0, 0.0] {
            for pair in samples.as_chunks_mut::<2>().0 {
                *pair = (65535 - u16::from_be_bytes(*pair)).to_be_bytes();
            }
        }
        return;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a value clamped to 0..=255"
    )]
    let tables: Vec<[u8; 256]> = decode
        .chunks(2)
        .map(|d| {
            std::array::from_fn(|p| {
                let (low, high) = (d.first().copied().unwrap_or(0.0), d.get(1).copied().unwrap_or(1.0));
                let x = (low + (f64::from(p as u8) / 255.0) * (high - low)).clamp(0.0, 1.0);
                round_even(x * 255.0) as u8
            })
        })
        .collect();
    for (s, table) in samples.iter_mut().zip(tables.iter().cycle()) {
        *s = table.get(usize::from(*s)).copied().unwrap_or(*s);
    }
}

/// A raster as a file: PNG for gray and RGB, a Deflate TIFF for CMYK. Encoding into memory fails only on sizes the
/// raster cannot have, so a failure is told without its detail.
fn encode(raster: &Raster) -> Result<(&'static str, Vec<u8>), Failure> {
    let failed = "it cannot be encoded";
    if raster.kind == Kind::Cmyk {
        use tiff::encoder::{Compression, DeflateLevel, TiffEncoder, colortype::CMYK8};
        use tiff::tags::Tag;
        let mut out = std::io::Cursor::new(Vec::new());
        let written = TiffEncoder::new(&mut out).ok().and_then(|encoder| {
            let mut encoder = encoder.with_compression(Compression::Deflate(DeflateLevel::Balanced));
            encoder
                .new_image::<CMYK8>(raster.width, raster.height)
                .ok()
                .and_then(|mut image| {
                    let icc = raster.icc.as_deref().unwrap_or_default();
                    let tagged = icc.is_empty() || image.encoder().write_tag(Tag::IccProfile, icc).is_ok();
                    image.write_data(&raster.samples).ok().filter(|()| tagged)
                })
        });
        return written.map(|()| ("tiff", out.into_inner())).ok_or(failed.to_owned());
    }
    let mut info = png::Info::with_size(raster.width, raster.height);
    info.color_type = if raster.kind == Kind::Gray {
        png::ColorType::Grayscale
    } else {
        png::ColorType::Rgb
    };
    info.bit_depth = if raster.wide {
        png::BitDepth::Sixteen
    } else {
        png::BitDepth::Eight
    };
    info.icc_profile = raster.icc.as_deref().map(Cow::Borrowed);
    let mut out = Vec::new();
    let written = png::Encoder::with_info(&mut out, info)
        .and_then(png::Encoder::write_header)
        .and_then(|mut writer| writer.write_image_data(&raster.samples).and_then(|()| writer.finish()))
        .ok();
    written.map(|()| ("png", out)).ok_or(failed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    const PROFILE: &[u8] = include_bytes!("../../../tests/fixtures/icc/little-cms-test1.icc");

    fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    }

    /// Image 2, a stream of `dict` and `data`, written out; `more` are objects from 3 on.
    fn written(dict: &str, data: &[u8], more: &[(u32, Vec<u8>)]) -> Result<(&'static str, Vec<u8>), Failure> {
        let mut objects = vec![
            (1, b"<< /Type /Catalog /Im 2 0 R >>".to_vec()),
            (2, stream(&format!("/Subtype /Image {dict}"), data)),
        ];
        objects.extend(more.iter().cloned());
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        let doc = Document::open(&classic(&refs, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        write(&doc, 2)
    }

    /// A PNG's colour type, depth, ICC profile and samples.
    fn png(bytes: &[u8]) -> (png::ColorType, png::BitDepth, Option<Vec<u8>>, Vec<u8>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut samples = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut samples).unwrap();
        let info = reader.info();
        (
            info.color_type,
            info.bit_depth,
            info.icc_profile.as_ref().map(|p| p.to_vec()),
            samples,
        )
    }

    fn gray(dict: &str, data: &[u8]) -> Vec<u8> {
        let (kind, _, _, samples) = png(&written(dict, data, &[]).unwrap().1);
        assert_eq!(kind, png::ColorType::Grayscale);
        samples
    }

    #[test]
    fn stored_formats_are_copied() {
        let jpeg = written(
            "/Width 1 /Height 1 /Filter /DCTDecode /ColorSpace /DeviceRGB",
            b"\xFF\xD8jpeg",
            &[],
        );
        assert_eq!(jpeg, Ok(("jpg", b"\xFF\xD8jpeg".to_vec())));
        let jpx = written(
            "/Width 1 /Height 1 /Filter [/ASCIIHexDecode /JPXDecode]",
            b"6A7032>",
            &[],
        );
        assert_eq!(jpx, Ok(("jp2", b"jp2".to_vec())));
        // The filters before the image filter cannot be undone.
        let broken = written(
            "/Width 1 /Height 1 /Filter [/FlateDecode /DCTDecode]",
            b"not flate",
            &[],
        );
        assert_eq!(broken, Err("its data cannot be decoded".to_owned()));
    }

    #[test]
    fn fax_data_gets_a_tiff_header() {
        let fax = |dict: &str, more: &[(u32, Vec<u8>)]| {
            written(
                &format!("/Width 8 /Height 2 /Filter /CCITTFaxDecode {dict}"),
                b"FAX",
                more,
            )
        };
        let (ext, tiff) = fax("/DecodeParms << /K -1 /BlackIs1 true >>", &[]).unwrap();
        assert_eq!(ext, "tif");
        // Nine tags of 12 bytes after the 10-byte start, then the next-IFD offset, then the data.
        assert_eq!((&tiff[..4], tiff.len()), (&b"II*\0"[..], 8 + 2 + 9 * 12 + 4 + 3));
        let tag = |tiff: &[u8], wanted: u16| {
            tiff[10..]
                .chunks(12)
                .take(usize::from(u16::from_le_bytes([tiff[8], tiff[9]])))
                .find(|t| u16::from_le_bytes([t[0], t[1]]) == wanted)
                .map(|t| u32::from_le_bytes([t[8], t[9], t[10], t[11]]))
        };
        assert_eq!(
            (tag(&tiff, 259), tag(&tiff, 262), tag(&tiff, 292)),
            (Some(4), Some(1), None)
        );
        assert!(tiff.ends_with(b"FAX"));
        // K above 0 is two-dimensional Group 3; a /Decode of [1 0] flips white; parameters may be an array.
        let (_, tiff) = fax("/DecodeParms [<< /K 1 >>] /Decode [1 0]", &[]).unwrap();
        assert_eq!(
            (tag(&tiff, 259), tag(&tiff, 262), tag(&tiff, 292)),
            (Some(3), Some(1), Some(1))
        );
        let (_, tiff) = fax("/DecodeParms null", &[]).unwrap();
        assert_eq!((tag(&tiff, 259), tag(&tiff, 262)), (Some(3), Some(0)));
        // An ICC-based space's profile goes in its own tag, before the data.
        let (_, tiff) = fax("/ColorSpace [/ICCBased 3 0 R]", &[(3, stream("/N 1", PROFILE))]).unwrap();
        // A profile written directly, not as a stream, cannot be read and is left out.
        let (_, direct) = fax("/ColorSpace [/ICCBased << /N 1 >>]", &[]).unwrap();
        assert_eq!(tag(&direct, 34675), None);
        let at = tag(&tiff, 34675).unwrap() as usize;
        assert!(tiff[at..].starts_with(PROFILE));
        assert_eq!(
            fax("/DecodeParms << /EncodedByteAlign true >>", &[]),
            Err("CCITT data aligned to bytes has no TIFF form".to_owned())
        );
        assert_eq!(
            written("/Filter /CCITTFaxDecode", b"FAX", &[]),
            Err("its /Width or /Height is not a size".to_owned())
        );
    }

    #[test]
    fn jbig2_is_decoded_to_gray() {
        // A page-information segment alone: a 2 by 1 page of its default pixel, black when flag 4 is set.
        let page = |flags: u8| {
            let mut body = [2u32, 1, 0, 0]
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<u8>>();
            body.push(flags);
            body.extend([0, 0]);
            let mut segment = vec![0, 0, 0, 1, 48, 0, 1, 0, 0, 0, 19];
            segment.extend(body);
            segment
        };
        let jbig2 = |dict: &str, data: &[u8], more: &[(u32, Vec<u8>)]| {
            let (kind, _, _, samples) =
                png(
                    &written(&format!("/Width 2 /Height 1 /Filter /JBIG2Decode {dict}"), data, more)
                        .unwrap()
                        .1,
                );
            (kind, samples)
        };
        assert_eq!(jbig2("", &page(0), &[]), (png::ColorType::Grayscale, vec![255, 255]));
        assert_eq!(jbig2("/Decode [1 0]", &page(4), &[]).1, vec![255, 255]);
        // Globals that do not decode are left out; a dictionary or an array of parameters may name them.
        let globals = [(3, stream("", b"junk"))];
        assert_eq!(
            jbig2("/DecodeParms << /JBIG2Globals 3 0 R >>", &page(4), &globals).1,
            vec![0, 0]
        );
        assert_eq!(
            jbig2("/DecodeParms [<< /JBIG2Globals 3 0 R >>]", &page(4), &globals).1,
            vec![0, 0]
        );
        assert_eq!(jbig2("/DecodeParms << /JBIG2Globals 9 >>", &page(4), &[]).1, vec![0, 0]);
        assert_eq!(jbig2("/DecodeParms null", &page(4), &[]).1, vec![0, 0]);
        assert_eq!(jbig2("/DecodeParms [null]", &page(4), &[]).1, vec![0, 0]);
        // A row of whole bytes comes in chunks of eight pixels.
        let mut wide = page(4);
        wide[11..15].copy_from_slice(&16u32.to_be_bytes());
        let (_, wide) = written("/Width 16 /Height 1 /Filter /JBIG2Decode", &wide, &[]).unwrap();
        assert_eq!(png(&wide).3, vec![0; 16]);
        let mut white = page(0);
        white[11..15].copy_from_slice(&16u32.to_be_bytes());
        let (_, white) = written("/Width 16 /Height 1 /Filter /JBIG2Decode", &white, &[]).unwrap();
        assert_eq!(png(&white).3, vec![255; 16]);
        // The image's own size wins: the black page is cut to 3 by 1, then padded white to 18 by 2.
        let mut black = page(4);
        black[11..15].copy_from_slice(&16u32.to_be_bytes());
        let (_, cut) = written("/Width 3 /Height 1 /Filter /JBIG2Decode", &black, &[]).unwrap();
        assert_eq!(png(&cut).3, vec![0; 3]);
        let (_, padded) = written("/Width 18 /Height 2 /Filter /JBIG2Decode", &black, &[]).unwrap();
        assert_eq!(png(&padded).3, [vec![0; 16], vec![255; 2], vec![255; 18]].concat());
        let (_, own) = written("/Filter /JBIG2Decode", &black, &[]).unwrap();
        assert_eq!(png(&own).3, vec![0; 16]);
        assert_eq!(
            written("/Width 2 /Height 1 /Filter /JBIG2Decode", b"junk", &[]),
            Err("its JBIG2 data cannot be decoded".to_owned())
        );
    }

    #[test]
    fn gray_samples_scaled_to_8_bits_and_decoded() {
        assert_eq!(
            gray(
                "/Width 3 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &[0, 128, 255]
            ),
            [0, 128, 255]
        );
        assert_eq!(
            gray("/Width 4 /Height 1 /ColorSpace /G /BitsPerComponent 2", &[0b0001_1011]),
            [0, 85, 170, 255]
        );
        assert_eq!(
            gray(
                "/Width 2 /Height 1 /ColorSpace [/CalGray << >>] /BitsPerComponent 4",
                &[0x0F]
            ),
            [0, 255]
        );
        // Rows start on a byte; 1-bit samples, and a stencil mask, read 1 as 255.
        assert_eq!(
            gray(
                "/Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 1",
                &[0x80, 0x40]
            ),
            [255, 0, 0, 255]
        );
        assert_eq!(gray("/Width 2 /Height 1 /ImageMask true", &[0x80]), [255, 0]);
        // /Decode by pikepdf's table, ties to even; an identity, or one of the wrong length, changes nothing.
        let decoded = "/Width 4 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Decode [0 0.5]";
        assert_eq!(gray(decoded, &[1, 3, 5, 255]), [0, 2, 2, 128]);
        assert_eq!(
            gray("/Width 1 /Height 1 /ColorSpace /DeviceGray /Decode [1 0]", &[10]),
            [245]
        );
        assert_eq!(
            gray("/Width 1 /Height 1 /ColorSpace /DeviceGray /Decode [0 1]", &[10]),
            [10]
        );
        assert_eq!(
            gray("/Width 1 /Height 1 /ColorSpace /DeviceGray /Decode [1 0 1 0]", &[10]),
            [10]
        );
        assert_eq!(
            gray("/Width 1 /Height 1 /ColorSpace /DeviceGray /Decode [1 (x)]", &[10]),
            [10]
        );
    }

    #[test]
    fn sixteen_bit_gray_stays_wide() {
        let wide = |dict: &str| {
            let (_, depth, _, samples) = png(&written(
                &format!(
                    "/Width 1 /Height 1 /ColorSpace /DeviceGray \
                /BitsPerComponent 16 {dict}"
                ),
                &[0x12, 0x34],
                &[],
            )
            .unwrap()
            .1);
            (depth, samples)
        };
        assert_eq!(wide(""), (png::BitDepth::Sixteen, vec![0x12, 0x34]));
        assert_eq!(wide("/Decode [1 0]").1, [0xED, 0xCB]);
        assert_eq!(wide("/Decode [0 0.5]").1, [0x12, 0x34]);
    }

    #[test]
    fn colour_images_and_their_profiles() {
        let (kind, depth, icc, samples) = png(&written(
            "/Width 1 /Height 1 /ColorSpace [/ICCBased 3 0 R] /BitsPerComponent 16",
            &[1, 2, 3, 4, 5, 6],
            &[(3, stream("/N 3", PROFILE))],
        )
        .unwrap()
        .1);
        assert_eq!(
            (kind, depth, icc.as_deref(), samples),
            (png::ColorType::Rgb, png::BitDepth::Eight, Some(PROFILE), vec![1, 3, 5])
        );
        let rgb = written(
            "/Width 1 /Height 1 /ColorSpace /DeviceRGB /Decode [1 0 0 1 0 1]",
            &[10, 20, 30],
            &[],
        );
        assert_eq!(png(&rgb.unwrap().1).3, [245, 20, 30]);
        // CMYK comes out as a TIFF, with its profile.
        let cmyk = |space: &str, more: &[(u32, Vec<u8>)]| {
            let (ext, tiff) = written(&format!("/Width 1 /Height 1 /ColorSpace {space}"), &[1, 2, 3, 4], more).unwrap();
            let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(tiff)).unwrap();
            let icc = decoder.get_tag_u8_vec(tiff::tags::Tag::IccProfile).ok();
            let mut samples = vec![0; 4];
            decoder.read_image_bytes(&mut samples).unwrap();
            (ext, decoder.colortype().unwrap(), icc, samples)
        };
        assert_eq!(
            cmyk("/DeviceCMYK", &[]),
            ("tiff", tiff::ColorType::CMYK(8), None, vec![1, 2, 3, 4])
        );
        let profiled = cmyk("[/ICCBased 3 0 R]", &[(3, stream("/N 4", PROFILE))]);
        assert_eq!(profiled.2.as_deref(), Some(PROFILE));
        let n1 = written(
            "/Width 1 /Height 1 /ColorSpace [/ICCBased 3 0 R]",
            &[7],
            &[(3, stream("/N 1", PROFILE))],
        );
        assert_eq!(png(&n1.unwrap().1).0, png::ColorType::Grayscale);
    }

    #[test]
    fn indexed_images_come_out_in_their_base() {
        let indexed = |space: &str, more: &[(u32, Vec<u8>)]| {
            written(
                &format!("/Width 3 /Height 1 /BitsPerComponent 2 /ColorSpace {space}"),
                &[0b0001_1000],
                more,
            )
        };
        let (kind, _, _, samples) = png(&indexed("[/Indexed /DeviceRGB 1 <FF0000 00FF00>]", &[]).unwrap().1);
        // Index 2 is past the palette: black.
        assert_eq!(
            (kind, samples),
            (png::ColorType::Rgb, vec![255, 0, 0, 0, 255, 0, 0, 0, 0])
        );
        let lookup = [(3, stream("", &[10, 20, 30]))];
        assert_eq!(
            png(&indexed("[/Indexed /DeviceGray 2 3 0 R]", &lookup).unwrap().1).3,
            [10, 20, 30]
        );
        // The profile of an ICC-based base goes with the image.
        let icc = [(3, stream("/N 1", PROFILE))];
        assert_eq!(
            png(&indexed("[/Indexed [/ICCBased 3 0 R] 1 <0102>]", &icc).unwrap().1)
                .2
                .as_deref(),
            Some(PROFILE)
        );
        assert_eq!(
            indexed("[/Indexed /DeviceGray 1 7]", &[]),
            Err("its palette cannot be read".to_owned())
        );
        assert_eq!(
            written(
                "/Width 1 /Height 1 /BitsPerComponent 16 /ColorSpace [/Indexed /DeviceGray 1 <00>]",
                &[0, 0],
                &[]
            ),
            Err("a 16-bit indexed image is not allowed".to_owned())
        );
    }

    #[test]
    fn images_that_cannot_be_written_say_why() {
        let why = |dict: &str, data: &[u8], more: &[(u32, Vec<u8>)]| written(dict, data, more).unwrap_err();
        assert_eq!(why("/Height 1", &[0], &[]), "its /Width or /Height is not a size");
        assert_eq!(why("/Width 1 /Height 1", &[0], &[]), "it has no colour space");
        let damaged = [(3, b"[1".to_vec())];
        assert_eq!(
            why("/Width 1 /Height 1 /ColorSpace 3 0 R", &[0], &damaged),
            "its colour space cannot be read"
        );
        assert_eq!(
            why("/Width 1 /Height 1 /ColorSpace [/Indexed 3 0 R 1 <00>]", &[0], &damaged),
            "its colour space cannot be read"
        );
        assert_eq!(
            why("/Width 1 /Height 1 /ColorSpace [/Lab << >>]", &[0], &[]),
            "its colour space Lab has no file format of its own"
        );
        assert_eq!(
            why("/Width 1 /Height 1 /ColorSpace 5", &[0], &[]),
            "its colour space ? has no file format of its own"
        );
        assert_eq!(
            why(
                "/Width 1 /Height 1 /ColorSpace [/ICCBased 3 0 R]",
                &[0],
                &[(3, stream("/N 2", PROFILE))]
            ),
            "its ICC profile's /N is not 1, 3 or 4"
        );
        assert_eq!(
            why("/Width 1 /Height 1 /ColorSpace [/ICCBased << /N 1 >>]", &[0], &[]),
            "its ICC profile's /N is not 1, 3 or 4"
        );
        assert_eq!(
            why(
                "/Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 3",
                &[0],
                &[]
            ),
            "its /BitsPerComponent is not 1, 2, 4, 8 or 16"
        );
        assert_eq!(
            why("/Width 2 /Height 2 /ColorSpace /DeviceRGB", &[0; 5], &[]),
            "its data is 5 bytes, short of the 12 its size needs"
        );
        assert_eq!(
            why("/Width 1 /Height 1 /Filter /FlateDecode", b"x", &[]),
            "its data cannot be decoded"
        );
        // A filter or /Decode entry that cannot be read is no filter or /Decode; a filter that is not a name stops
        // the data from decoding at all.
        let gray = "/Width 1 /Height 1 /ColorSpace /DeviceGray";
        assert_eq!(
            why(&format!("{gray} /Filter [3 0 R]"), &[0], &damaged),
            "its data cannot be decoded"
        );
        assert_eq!(
            why(&format!("{gray} /Filter 5"), &[0], &[]),
            "its data cannot be decoded"
        );
        assert_eq!(
            png(&written(&format!("{gray} /Decode 3 0 R"), &[9], &damaged).unwrap().1).3,
            [9]
        );
        assert_eq!(png(&written(&format!("{gray} /Decode 5"), &[9], &[]).unwrap().1).3, [9]);
        assert_eq!(
            png(&written(&format!("{gray} /Decode [3 0 R 0]"), &[9], &damaged).unwrap().1).3,
            [9]
        );
        let doc = Document::open(
            &classic(
                &[(1, b"<< /Type /Catalog >>"), (2, b"<< >>")],
                "<< /Size SIZE /Root 1 0 R >>",
            ),
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(write(&doc, 2).unwrap_err(), "it is not a stream that can be read");
    }
}
