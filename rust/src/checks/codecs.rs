//! PDF/X-4 (ISO 15930-7) 6.8: `DCTDecode` and `JBIG2Decode` data is valid, judged at the marker and segment level as
//! the register's rows say: a JPEG's markers and frame and scan headers (T.81 Annex B), JBIG2's segment headers and
//! lengths (T.88 7.2) in the embedded organisation PDF uses (PDF 1.6 3.3.6), its `/JBIG2Globals` stream included.
//! The coded data itself is not decoded.
//!
//! The images judged are image `XObjects` whose filters include the codec, wherever they are in the file, as for
//! JPEG 2000 (`x4.image.jpx-valid`). An image whose data cannot be unfiltered up to the codec is left to the filter
//! checks. `JBIG2Decode` applies only to `XObjects` (PDF 1.6 3.3.6).
// ponytail: inline images with `DCTDecode` are not judged; walk content streams for them if one ever turns up.
// ponytail: a JPEG whose entropy-coded data is corrupt (a bad Huffman code, bytes before a marker) passes; decode it
// with `zune-jpeg`, as `common::raster` does, when such a file matters (5 of 31,698 corpus JPEGs, 2026-11-26).

use super::Settings;
use super::walk::dicts;
use crate::common::{jbig2, jpeg};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{Check, Finding, Location, X4_IMAGE_DCT_VALID, X4_IMAGE_JBIG2_VALID};

/// Every image `XObject` whose filters include `filter`: its object number, its dictionary, the filter's place among
/// the filters (`None` for a single filter name) and its data as that filter reads it.
pub(super) fn coded<'a>(doc: &'a Document, filter: &[u8]) -> Vec<(u32, &'a Dict, Option<usize>, Vec<u8>)> {
    let mut out = Vec::new();
    for node in dicts(doc) {
        let place = match node.dict.get(b"Filter").and_then(|f| doc.resolve(f)).map(|f| &f.value) {
            Some(Value::Name(n)) if n == filter => Some(None),
            Some(Value::Array(items)) => items.iter().position(|i| i.name() == Some(filter)).map(Some),
            _ => None,
        };
        let Some(place) = place else { continue };
        if !(node.stream && node.name(b"Subtype") == Some(b"Image")) {
            continue;
        }
        if let Some(data) = doc.image_data(node.num) {
            out.push((node.num, node.dict, place, data));
        }
    }
    out
}

fn finding(check: &'static Check, what: &str, num: u32, problem: &str) -> Finding {
    Finding::new(check, format!("{what} image {num}: {problem}"))
        .citing("6.8")
        .at(vec![Location::Object(num, 0)])
}

pub(super) fn dct_valid(doc: &Document, _: &Settings) -> Vec<Finding> {
    coded(doc, b"DCTDecode")
        .into_iter()
        .filter_map(|(num, _, _, data)| {
            let problem = jpeg::problem(&data)?;
            Some(finding(
                X4_IMAGE_DCT_VALID,
                "JPEG",
                num,
                &format!("not valid JPEG data: {problem}"),
            ))
        })
        .collect()
}

pub(super) fn jbig2_valid(doc: &Document, _: &Settings) -> Vec<Finding> {
    coded(doc, b"JBIG2Decode")
        .into_iter()
        .filter_map(|(num, dict, place, data)| {
            let problem = match globals(doc, dict, place) {
                Some(Err(())) => Some("its /JBIG2Globals is not a stream that can be read".to_owned()),
                Some(Ok(globals)) => jbig2::problem(&globals, false)
                    .map(|p| format!("its /JBIG2Globals is not valid JBIG2 data: {p}"))
                    .or_else(|| jbig2::problem(&data, true).map(|p| format!("not valid JBIG2 data: {p}"))),
                None => jbig2::problem(&data, true).map(|p| format!("not valid JBIG2 data: {p}")),
            }?;
            Some(finding(X4_IMAGE_JBIG2_VALID, "JBIG2", num, &problem))
        })
        .collect()
}

/// The `/JBIG2Globals` stream's data for the `JBIG2Decode` filter at `place`: `None` without one, an error when the entry
/// is not a stream whose data can be read.
fn globals(doc: &Document, dict: &Dict, place: Option<usize>) -> Option<Result<Vec<u8>, ()>> {
    let parms = dict.get(b"DecodeParms").and_then(|p| doc.resolve(p));
    let parms = match (place, parms.map(|p| &p.value)) {
        (Some(i), Some(Value::Array(items))) => items.get(i).and_then(|p| doc.resolve(p)),
        _ => parms,
    };
    let entry = parms.and_then(Object::dict).and_then(|d| d.get(b"JBIG2Globals"))?;
    Some(match entry.value {
        Value::Reference(num, _) => doc.stream_data(num).ok_or(()),
        _ => Err(()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::raster::tests::JPEG;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    }

    /// The findings of both checks on a file holding `objects` from number 10 on, each referred to by the catalog.
    fn messages(objects: &[Vec<u8>]) -> Vec<String> {
        let refs: Vec<String> = (0..objects.len()).map(|i| format!("{} 0 R", 10 + i)).collect();
        let catalog = format!("<< /Things [{}] >>", refs.join(" "));
        let mut all: Vec<(u32, &[u8])> = vec![(1, catalog.as_bytes())];
        for (i, data) in objects.iter().enumerate() {
            all.push((u32::try_from(10 + i).unwrap(), data));
        }
        let doc = Document::open(&classic(&all, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        let settings = Level::X4.into();
        let mut out: Vec<String> = dct_valid(&doc, &settings).into_iter().map(|f| f.message).collect();
        out.extend(jbig2_valid(&doc, &settings).into_iter().map(|f| f.message));
        out
    }

    /// A page information segment, all a JBIG2 image needs here; global segments are a symbol dictionary.
    const PAGE: [u8; 30] = [
        0, 0, 0, 1, 48, 0, 1, 0, 0, 0, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const SYMBOLS: [u8; 11] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    #[test]
    fn jpeg_and_jbig2_images() {
        let image = "/Subtype /Image";
        let objects = [
            stream(&format!("{image} /Filter /DCTDecode"), &JPEG),
            stream(&format!("{image} /Filter [/FlateDecode /DCTDecode]"), b"x"),
            stream(&format!("{image} /Filter [/ASCIIHexDecode /DCTDecode]"), b"FFD9>"),
            stream("/Subtype /Form /Filter /DCTDecode", b"x"),
            b"<< /Subtype /Image /Filter /DCTDecode >>".to_vec(),
            stream(&format!("{image} /Filter 5"), b"x"),
            stream(&format!("{image} /Filter /JBIG2Decode"), &PAGE),
            stream(&format!("{image} /Filter /JBIG2Decode"), &SYMBOLS),
            stream(
                &format!("{image} /Filter /JBIG2Decode /DecodeParms << /JBIG2Globals 30 0 R >>"),
                &PAGE,
            ),
            stream(
                &format!("{image} /Filter [/JBIG2Decode] /DecodeParms [<< /JBIG2Globals 31 0 R >>]"),
                &PAGE,
            ),
            stream(
                &format!("{image} /Filter /JBIG2Decode /DecodeParms << /JBIG2Globals 5 >>"),
                &PAGE,
            ),
            stream(
                &format!("{image} /Filter [/JBIG2Decode] /DecodeParms << /JBIG2Globals 30 0 R >>"),
                &SYMBOLS,
            ),
            stream(&format!("{image} /Filter [/JBIG2Decode] /DecodeParms [5]"), &PAGE),
        ];
        let mut all = objects.to_vec();
        all.resize(20, b"null".to_vec());
        all.push(stream("", &SYMBOLS));
        all.push(stream("", &PAGE[..20]));
        assert_eq!(
            messages(&all),
            [
                "JPEG image 12: not valid JPEG data: no start-of-image marker at its start",
                "JBIG2 image 17: not valid JBIG2 data: no page information segment",
                "JBIG2 image 19: its /JBIG2Globals is not valid JBIG2 data: segment 1's data runs past the end",
                "JBIG2 image 20: its /JBIG2Globals is not a stream that can be read",
                "JBIG2 image 21: not valid JBIG2 data: no page information segment",
            ]
        );
    }
}
