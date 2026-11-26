//! PDF/X-4 (ISO 15930-7) 6.27, JPEG 2000 images: 1, 3 or 4 channels, at most one colour specification, no `CIEJab`,
//! bit depths of 1, 8 or 16 and the same in every channel; and profile-less CMYK data with no `/ColorSpace` is
//! `DeviceCMYK`, held to 6.4.3.2.
//!
//! 6.27's JPX baseline is ITU-T T.801 (= ISO/IEC 15444-2) M.9.2, and 6.8's "valid" JPEG 2000 is judged from the header
//! only: the codestream is never decoded.
//!
//! The image `XObjects` judged are those whose filters include `JPXDecode`, wherever they are in the file; JPEG 2000 data
//! whose header cannot be read is left to `x4.image.jpx-valid`.

use super::Settings;
use super::codecs::coded;
use super::intent;
use crate::common::jpx::{self, Jpx};
use crate::document::Document;
use crate::findings::{
    Check, Finding, Location, X4_IMAGE_JPX_VALID, X4_JPX_BASELINE, X4_JPX_BIT_DEPTH, X4_JPX_CHANNELS,
    X4_JPX_CMYK_AS_DEVICE, X4_JPX_NO_CIEJAB, X4_JPX_ONE_COLOUR_SPACE, X4_JPX_UNIFORM_DEPTH,
};

/// The JPEG 2000 enumerated colour spaces 6.27 names (15444-2, Table M.25).
const CIEJAB: u32 = 19;
const CMYK: u32 = 12;

/// The enumerated colour spaces a baseline file's colour specification may name (T.801 M.9.2.4: sRGB, sRGB-grey,
/// ROMM-RGB, sYCC, e-sRGB, e-sYCC, `CIELab` and `CIEJab`), with CMYK, which 6.27 allows. 6.27's ban on `CIEJab` is
/// `x4.jpx.no-ciejab`'s.
const BASELINE_SPACES: [u32; 9] = [CMYK, 14, 16, 17, 18, CIEJAB, 20, 21, 24];

/// Every JPEG 2000 image: its object number, whether its dictionary gives a `/ColorSpace`, and its header, `None` when
/// the data is not JPEG 2000. Images whose data cannot be unfiltered up to `JPXDecode` are left to the filter checks.
fn images(doc: &Document) -> Vec<(u32, bool, Option<Jpx>)> {
    coded(doc, b"JPXDecode")
        .into_iter()
        .map(|(num, dict, _, data)| (num, dict.get(b"ColorSpace").is_some(), jpx::read(&data)))
        .collect()
}

fn each(doc: &Document, check: &'static Check, judge: &dyn Fn(&Jpx, bool) -> Option<String>) -> Vec<Finding> {
    each_read(doc, check, "6.27", &|header, spaced| judge(header.as_ref()?, spaced))
}

fn each_read(
    doc: &Document,
    check: &'static Check,
    clause: &str,
    judge: &dyn Fn(Option<&Jpx>, bool) -> Option<String>,
) -> Vec<Finding> {
    images(doc)
        .into_iter()
        .filter_map(|(num, spaced, header)| {
            let message = judge(header.as_ref(), spaced)?;
            Some(
                Finding::new(check, format!("JPEG 2000 image {num}: {message}"))
                    .citing(clause)
                    .at(vec![Location::Object(num, 0)]),
            )
        })
        .collect()
}

pub(super) fn channels(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_JPX_CHANNELS, &|h, _| {
        let n = h.channels.unwrap_or(0);
        (!matches!(n, 1 | 3 | 4)).then(|| format!("{n} channels, not 1, 3 or 4"))
    })
}

pub(super) fn one_colour_space(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_JPX_ONE_COLOUR_SPACE, &|h, _| {
        (h.colours.len() > 1).then(|| format!("{} colour specifications, not at most one", h.colours.len()))
    })
}

pub(super) fn no_ciejab(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_JPX_NO_CIEJAB, &|h, _| {
        h.colours
            .iter()
            .any(|c| c.enumerated == Some(CIEJAB))
            .then(|| "the CIEJab colour space (19)".to_owned())
    })
}

pub(super) fn bit_depth(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_JPX_BIT_DEPTH, &|h, _| {
        let bad: Vec<u8> = h.depths.iter().copied().filter(|d| !matches!(d, 1 | 8 | 16)).collect();
        bad.first().map(|d| format!("a bit depth of {d}, not 1, 8 or 16"))
    })
}

pub(super) fn uniform_depth(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_JPX_UNIFORM_DEPTH, &|h, _| {
        let first = h.depths.first()?;
        h.depths
            .iter()
            .any(|d| d != first)
            .then(|| format!("channels of different bit depths: {:?}", h.depths))
    })
}

/// 6.8: JPEG 2000 data is valid, as far as its header shows (the codestream is not decoded). A bare codestream is valid
/// here; `x4.jpx.baseline` fails it.
pub(super) fn valid(doc: &Document, _: &Settings) -> Vec<Finding> {
    each_read(doc, X4_IMAGE_JPX_VALID, "6.8", &|header, _| match header {
        None => Some("not JPEG 2000 data (neither the file format nor a codestream)".to_owned()),
        Some(h) if !h.structure.is_empty() => Some(format!(
            "its header is not valid (the codestream is not decoded): {}",
            h.structure.join("; ")
        )),
        Some(_) => None,
    })
}

/// T.801 M.9.2.3: the codestream needs at most Part 1 profile 0, 1 or 2 (`Rsiz` 0 to 2), or, with Part 2 extensions
/// (bit 15), only the multiple component and non-linearity transforms (bits 8 and 9) and the informative bits 2 and 3.
/// Later bits, such as HTJ2K's, came after the 2004 edition 6.27 cites and fail.
fn baseline_capabilities(rsiz: u16) -> bool {
    if rsiz & 0x8000 == 0 {
        rsiz <= 2
    } else {
        rsiz & !(0x8000 | 0x0200 | 0x0100 | 0x0008 | 0x0004) == 0
    }
}

/// What keeps a JPX file from the baseline set of features (T.801 M.9.2), in words.
fn baseline_problems(h: &Jpx) -> Vec<String> {
    if !h.wrapped {
        return vec!["a bare codestream, not the JPX file format".to_owned()];
    }
    let mut out = Vec::new();
    let mut problem = |p: &str, holds: bool| {
        if holds {
            out.push(p.to_owned());
        }
    };
    problem(
        "jpx is not among the compatible brands",
        !h.compatible.contains(b"jpx "),
    );
    problem(
        "jpxb is not among the compatible brands",
        !h.compatible.contains(b"jpxb"),
    );
    problem("no Reader Requirements box after the File Type box", !h.requirements);
    problem(
        "the Image Header's compression type is not 7",
        h.compression.is_some_and(|c| c != 7),
    );
    problem(
        "the codestream needs more than the baseline's capabilities",
        h.capabilities.is_some_and(|r| !baseline_capabilities(r)),
    );
    let usable = |c: &&jpx::Colour| {
        c.approx <= 3
            && (matches!(c.method, 2 | 3)
                || c.method == 1 && c.enumerated.is_some_and(|e| BASELINE_SPACES.contains(&e)))
    };
    problem(
        "no baseline colour specification (ICC, or an enumerated space it lists) with an approximation of 3 or less",
        !h.colours.iter().any(|c| usable(&c)),
    );
    let at = |kind: &[u8; 4]| h.order.iter().position(|k| k == kind);
    problem(
        "the JP2 Header box comes after the codestream",
        at(b"jp2h").zip(at(b"jp2c")).is_some_and(|(head, code)| head > code),
    );
    problem(
        "fragment table or data reference boxes, which are not followed, so the codestream's place is not verified",
        h.order.iter().any(|k| k == b"ftbl" || k == b"cref"),
    );
    out
}

pub(super) fn baseline(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_JPX_BASELINE, &|h, _| {
        let problems = baseline_problems(h);
        (!problems.is_empty()).then(|| format!("not baseline JPX: {}", problems.join("; ")))
    })
}

/// 6.27: CMYK data (enumerated colour space 12) with no ICC profile and no `/ColorSpace` is `DeviceCMYK`, so 6.4.3.2
/// allows it only under a CMYK intent.
pub(super) fn cmyk_as_device(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let intent = intent::space(doc, settings);
    if intent.is_none_or(|i| i == *b"CMYK") {
        return Vec::new();
    }
    device_cmyk(doc, X4_JPX_CMYK_AS_DEVICE, "6.27", "under a non-CMYK intent")
}

/// A finding from `check` for each JPEG 2000 image whose CMYK data has no profile and no `/ColorSpace`, so reads as
/// `DeviceCMYK`; `why` says why that is not allowed.
pub(super) fn device_cmyk(doc: &Document, check: &'static Check, clause: &str, why: &str) -> Vec<Finding> {
    each_read(doc, check, clause, &|h, spaced| {
        let h = h?;
        let cmyk = h.colours.iter().any(|c| c.enumerated == Some(CMYK));
        let profiled = h.colours.iter().any(|c| matches!(c.method, 2 | 3));
        (cmyk && !profiled && !spaced)
            .then(|| format!("CMYK data with no profile and no /ColorSpace, so DeviceCMYK, {why}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::jpx::tests::{boxed, codestream, colr, file, ihdr};
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    const RUNS: [fn(&Document, &Settings) -> Vec<Finding>; 8] = [
        valid,
        baseline,
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
            .flat_map(|run| run(&doc, &Level::X4.into()))
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
            image(
                &format!("{jpx} /ColorSpace /DeviceCMYK"),
                &file(&[ihdr(4, 7), boxed(*b"colr", &[1, 0, 1, 0, 0, 0, 12])], &[8; 3]),
            ),
        ];
        assert_eq!(
            ids(&profile(b"RGB "), &images),
            [
                "x4.image.jpx-valid",
                "x4.image.jpx-valid",
                "x4.jpx.baseline",
                "x4.jpx.channels",
                "x4.jpx.one-colour-space",
                "x4.jpx.cmyk-as-device"
            ]
        );
        assert_eq!(
            ids(&profile(b"CMYK"), &images),
            [
                "x4.image.jpx-valid",
                "x4.image.jpx-valid",
                "x4.jpx.baseline",
                "x4.jpx.channels",
                "x4.jpx.one-colour-space"
            ],
            "CMYK is DeviceCMYK's own intent"
        );
        assert_eq!(
            ids(b"<< >>", &images),
            [
                "x4.image.jpx-valid",
                "x4.image.jpx-valid",
                "x4.jpx.baseline",
                "x4.jpx.channels",
                "x4.jpx.one-colour-space"
            ],
            "no intent to judge it by"
        );
    }

    #[test]
    fn baseline_features() {
        let problems = |data: &[u8]| baseline_problems(&jpx::read(data).unwrap());
        let signature = boxed(*b"jP  ", &[0x0D, 0x0A, 0x87, 0x0A]);
        let head = |boxes: &[Vec<u8>]| boxed(*b"jp2h", &boxes.concat());
        let stream = |rsiz: u16| {
            let mut c = codestream(&[8; 3]);
            c.get_mut(6..8).unwrap().copy_from_slice(&rsiz.to_be_bytes());
            boxed(*b"jp2c", &c)
        };
        assert_eq!(problems(&file(&[ihdr(3, 7), colr(16)], &[8; 3])), Vec::<String>::new());
        assert_eq!(
            problems(&codestream(&[8])),
            ["a bare codestream, not the JPX file format"]
        );
        let plain_jp2 = [
            signature.clone(),
            boxed(*b"ftyp", b"jp2 \0\0\0\0jp2 "),
            head(&[ihdr(3, 7), colr(16)]),
            stream(0),
        ]
        .concat();
        assert_eq!(
            problems(&plain_jp2),
            [
                "jpx is not among the compatible brands",
                "jpxb is not among the compatible brands",
                "no Reader Requirements box after the File Type box",
            ]
        );
        let mut wavelet = ihdr(3, 7);
        *wavelet.get_mut(19).unwrap() = 6;
        let ftyp = boxed(*b"ftyp", b"jpx \0\0\0\0jpxbjpx ");
        let rreq = boxed(*b"rreq", &[1, 0xFF, 0xFF, 0, 0, 0, 0]);
        let unusual = [
            signature.clone(),
            ftyp.clone(),
            rreq.clone(),
            boxed(*b"ftbl", b""),
            stream(0x4000),
            head(&[
                wavelet,
                boxed(*b"colr", &[1, 0, 4, 0, 0, 0, 19]),
                boxed(*b"colr", &[2, 0, 4]),
            ]),
        ]
        .concat();
        assert_eq!(
            problems(&unusual),
            [
                "the Image Header's compression type is not 7",
                "the codestream needs more than the baseline's capabilities",
                "no baseline colour specification (ICC, or an enumerated space it lists) with an approximation of 3 or less",
                "the JP2 Header box comes after the codestream",
                "fragment table or data reference boxes, which are not followed, so the codestream's place is not verified",
            ]
        );
        let extended = |rsiz: u16, colour: &[u8]| {
            problems(
                &[
                    signature.clone(),
                    ftyp.clone(),
                    rreq.clone(),
                    head(&[ihdr(3, 7), boxed(*b"colr", colour)]),
                    stream(rsiz),
                ]
                .concat(),
            )
        };
        assert_eq!(
            extended(0x8304, &[3, 0, 1]),
            Vec::<String>::new(),
            "Part 2 transforms, restricted ICC"
        );
        assert_eq!(
            extended(2, &[1, 0, 3, 0, 0, 0, 12]),
            Vec::<String>::new(),
            "CMYK, a reasonable approximation"
        );
        assert_eq!(extended(3, &[9, 0, 1]).len(), 2, "profile 3 and a vendor colour method");
        assert_eq!(extended(0, &[1, 0, 1]).len(), 1, "an enumerated method with no space");
        assert_eq!(
            problems(&[signature, boxed(*b"cref", b"")].concat()).last().unwrap(),
            &problems(&unusual)[4]
        );
    }
}
