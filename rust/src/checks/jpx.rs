//! PDF/X-4 (ISO 15930-7) 6.27, JPEG 2000 images: 1, 3 or 4 channels, at most one colour specification, no `CIEJab`,
//! bit depths of 1, 8 or 16 and the same in every channel; and profile-less CMYK data with no `/ColorSpace` is
//! `DeviceCMYK`, held to 6.4.3.2.
//!
//! The image `XObjects` judged are those whose filters include `JPXDecode`, wherever they are in the file; JPEG 2000 data
//! whose header cannot be read is left to `x4.image.jpx-valid`.

use super::intent;
use super::walk::dicts;
use crate::common::jpx::{self, Jpx};
use crate::document::{Document, Value};
use crate::findings::{
    Check, Finding, Level, Location, X4_JPX_BIT_DEPTH, X4_JPX_CHANNELS, X4_JPX_CMYK_AS_DEVICE, X4_JPX_NO_CIEJAB,
    X4_JPX_ONE_COLOUR_SPACE, X4_JPX_UNIFORM_DEPTH,
};

/// The JPEG 2000 enumerated colour spaces 6.27 names (15444-2, Table M.25).
const CIEJAB: u32 = 19;
const CMYK: u32 = 12;

/// Every JPEG 2000 image: its object number, whether its dictionary gives a `/ColorSpace`, and its header.
fn images(doc: &Document) -> Vec<(u32, bool, Jpx)> {
    let mut out = Vec::new();
    for node in dicts(doc) {
        let filters = node.dict.get(b"Filter").and_then(|f| doc.resolve(f));
        let jpx_filter = match filters.map(|f| &f.value) {
            Some(Value::Name(n)) => n == b"JPXDecode",
            Some(Value::Array(items)) => items.iter().any(|i| i.name() == Some(b"JPXDecode")),
            _ => false,
        };
        if !(node.stream && jpx_filter && node.name(b"Subtype") == Some(b"Image")) {
            continue;
        }
        if let Some(header) = doc.image_data(node.num).as_deref().and_then(jpx::read) {
            out.push((node.num, node.dict.get(b"ColorSpace").is_some(), header));
        }
    }
    out
}

fn each(doc: &Document, check: &'static Check, judge: &dyn Fn(&Jpx, bool) -> Option<String>) -> Vec<Finding> {
    images(doc)
        .into_iter()
        .filter_map(|(num, spaced, header)| {
            let message = judge(&header, spaced)?;
            Some(
                Finding::new(check, format!("JPEG 2000 image {num}: {message}"))
                    .citing("6.27")
                    .at(vec![Location::Object(num, 0)]),
            )
        })
        .collect()
}

pub(super) fn channels(doc: &Document, _: Level) -> Vec<Finding> {
    each(doc, X4_JPX_CHANNELS, &|h, _| {
        let n = h.channels.unwrap_or(0);
        (!matches!(n, 1 | 3 | 4)).then(|| format!("{n} channels, not 1, 3 or 4"))
    })
}

pub(super) fn one_colour_space(doc: &Document, _: Level) -> Vec<Finding> {
    each(doc, X4_JPX_ONE_COLOUR_SPACE, &|h, _| {
        (h.colours.len() > 1).then(|| format!("{} colour specifications, not at most one", h.colours.len()))
    })
}

pub(super) fn no_ciejab(doc: &Document, _: Level) -> Vec<Finding> {
    each(doc, X4_JPX_NO_CIEJAB, &|h, _| {
        h.colours
            .iter()
            .any(|(_, e)| *e == Some(CIEJAB))
            .then(|| "the CIEJab colour space (19)".to_owned())
    })
}

pub(super) fn bit_depth(doc: &Document, _: Level) -> Vec<Finding> {
    each(doc, X4_JPX_BIT_DEPTH, &|h, _| {
        let bad: Vec<u8> = h.depths.iter().copied().filter(|d| !matches!(d, 1 | 8 | 16)).collect();
        bad.first().map(|d| format!("a bit depth of {d}, not 1, 8 or 16"))
    })
}

pub(super) fn uniform_depth(doc: &Document, _: Level) -> Vec<Finding> {
    each(doc, X4_JPX_UNIFORM_DEPTH, &|h, _| {
        let first = h.depths.first()?;
        h.depths
            .iter()
            .any(|d| d != first)
            .then(|| format!("channels of different bit depths: {:?}", h.depths))
    })
}

/// 6.27: CMYK data (enumerated colour space 12) with no ICC profile and no `/ColorSpace` is `DeviceCMYK`, so 6.4.3.2
/// allows it only under a CMYK intent.
pub(super) fn cmyk_as_device(doc: &Document, _: Level) -> Vec<Finding> {
    let intent = intent::profile(doc).map(|h| h.space);
    if intent.is_none_or(|i| i == *b"CMYK") {
        return Vec::new();
    }
    each(doc, X4_JPX_CMYK_AS_DEVICE, &|h, spaced| {
        let cmyk = h.colours.iter().any(|(_, e)| *e == Some(CMYK));
        let profiled = h.colours.iter().any(|(m, _)| matches!(m, 2 | 3));
        (cmyk && !profiled && !spaced)
            .then(|| "CMYK data with no profile and no /ColorSpace, so DeviceCMYK, under a non-CMYK intent".to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::jpx::tests::{boxed, codestream, file, ihdr};
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    const RUNS: [fn(&Document, Level) -> Vec<Finding>; 6] = [
        channels,
        one_colour_space,
        no_ciejab,
        bit_depth,
        uniform_depth,
        cmyk_as_device,
    ];

    fn profile(space: &[u8]) -> Vec<u8> {
        let mut icc = vec![0u8; 128];
        icc.get_mut(16..20).unwrap().copy_from_slice(space);
        icc.get_mut(36..40).unwrap().copy_from_slice(b"acsp");
        let mut out = format!("<< /Length {} >>\nstream\n", icc.len()).into_bytes();
        out.extend(icc);
        out.extend(b"\nendstream");
        out
    }

    fn image(entries: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    }

    fn ids(intent: &[u8], images: &[Vec<u8>]) -> Vec<&'static str> {
        let refs: Vec<String> = (0..images.len()).map(|i| format!("{} 0 R", 10 + i)).collect();
        let catalog = format!(
            "<< /OutputIntents [<< /S /GTS_PDFX /DestOutputProfile 8 0 R >>] /Images [{}] >>",
            refs.join(" ")
        );
        let mut objects: Vec<(u32, &[u8])> = vec![(1, catalog.as_bytes()), (8, intent)];
        for (i, data) in images.iter().enumerate() {
            objects.push((u32::try_from(10 + i).unwrap(), data));
        }
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        RUNS.iter()
            .flat_map(|run| run(&doc, Level::X4))
            .map(|f| f.check.id)
            .collect()
    }

    #[test]
    fn images_and_their_headers() {
        let jpx = "/Subtype /Image /Filter /JPXDecode";
        let profiled = file(
            &[
                ihdr(4, 7),
                boxed(*b"colr", &[1, 0, 0, 0, 0, 0, 12]),
                boxed(*b"colr", &[2, 0, 0]),
            ],
            &[8; 4],
        );
        let images = [
            image(
                jpx,
                &file(&[ihdr(4, 7), boxed(*b"colr", &[1, 0, 0, 0, 0, 0, 12])], &[8; 4]),
            ),
            image("/Subtype /Image /Filter [/FlateDecode /JPXDecode]", b"x"),
            image("/Subtype /Image /Filter /DCTDecode", b"x"),
            image("/Subtype /Form /Filter /JPXDecode", b"x"),
            image(
                &format!("{jpx} /ColorSpace /DeviceCMYK"),
                &file(&[ihdr(4, 7), boxed(*b"colr", &[1, 0, 0, 0, 0, 0, 12])], &[8; 4]),
            ),
            image(jpx, &profiled),
            image(jpx, &codestream(&[])),
            image("/Subtype /Image /Filter 5", b"x"),
            image(jpx, b"not jpeg 2000"),
        ];
        assert_eq!(
            ids(&profile(b"RGB "), &images),
            ["x4.jpx.channels", "x4.jpx.one-colour-space", "x4.jpx.cmyk-as-device"]
        );
        assert_eq!(
            ids(&profile(b"CMYK"), &images),
            ["x4.jpx.channels", "x4.jpx.one-colour-space"],
            "CMYK is DeviceCMYK's own intent"
        );
        assert_eq!(
            ids(b"<< >>", &images),
            ["x4.jpx.channels", "x4.jpx.one-colour-space"],
            "no intent to judge it by"
        );
    }
}
