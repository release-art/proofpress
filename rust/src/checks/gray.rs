//! The gray-job check (ported from the Python `colour` module, under the parity gate): in a gray job, everything is
//! painted in the gray family and the output intent is not RGB (nor CMYK, unless K-only CMYK is allowed); spot inks,
//! which have no gray value, are a warning.
//!
//! With K-only CMYK allowed, `DeviceCMYK` counts as gray when it uses the K plate alone: a colour by its components, an
//! image `XObject` by decoding its samples once. ICC-based and indexed CMYK, CMYK shadings and patterns, and inline
//! CMYK images do not count, since a K-only value there does not mean gray, or cannot be read. A shading's or image's
//! own spot inks are read without the resources, as the Python did.

use std::collections::{BTreeMap, BTreeSet};

use super::ink::{describe, is_k_only};
use super::modes::{Page, intent_family, on_page, pages, shown, text, warning};
use super::{ColourMode, Settings};
use crate::common::family::{self, Family};
use crate::common::raster;
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{Finding, GRAY_ONLY};
use crate::graphics::{self, Fill, Ink, Kind, Role};

const UNREADABLE: &str = "could not be decoded, so K-only cannot be confirmed";

/// A gray job paints only gray.
pub(super) fn gray_only(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    if settings.params.colour_mode != ColourMode::Gray {
        return out;
    }
    let k_only = settings.params.allow_k_only;
    let profile = match intent_family(doc) {
        Some(Family::Rgb) => Some("an RGB profile"),
        Some(Family::Cmyk) if !k_only => Some("a CMYK profile"),
        _ => None,
    };
    if let Some(profile) = profile {
        out.push(Finding::new(
            GRAY_ONLY,
            format!("the output intent is {profile} in a gray job"),
        ));
    }
    let mut decoded = BTreeMap::new();
    for (number, page) in pages(doc) {
        let (images, colours, spots) = painted(doc, &page, k_only, &mut decoded);
        for (name, why) in images {
            out.push(on_page(GRAY_ONLY, number, format!("image {name} {why}")));
        }
        if !colours.is_empty() {
            let first: Vec<&str> = colours.iter().take(3).map(String::as_str).collect();
            let message = format!("artwork is painted outside the gray family ({}...)", first.join(", "));
            out.push(on_page(GRAY_ONLY, number, message));
        }
        if !spots.is_empty() {
            let names: Vec<&str> = spots.iter().map(String::as_str).collect();
            let message = format!("spot colour(s) {} have no gray value", names.join(", "));
            out.push(warning(on_page(GRAY_ONLY, number, message)));
        }
    }
    out
}

type Painted = (BTreeMap<String, &'static str>, BTreeSet<String>, BTreeSet<String>);

/// What a page paints outside gray: why each image is not gray by name (the last reason for a name), its other
/// colours described, and its spot inks.
fn painted(doc: &Document, page: &Page, k_only: bool, decoded: &mut BTreeMap<u32, Option<&'static str>>) -> Painted {
    let (mut images, mut colours, mut spots) = (BTreeMap::new(), BTreeSet::new(), BTreeSet::new());
    for (kind, uses, ink, object) in &page.paints {
        let mask = uses.iter().any(|u| u.role == Role::ImageMask);
        if matches!(kind, Kind::Image | Kind::InlineImage | Kind::Shading) && !mask {
            let names = family::spot_names(doc, ink.own_space.as_ref(), None);
            spots.extend(names.iter().map(|n| text(n)));
            if !names.is_empty() || ink.space == Some(Family::Gray) {
                continue;
            }
            let name = shown(*kind, &ink.name);
            if *kind == Kind::Shading {
                colours.insert(format!("shading {name}"));
            } else if let Some(why) = image_problem(doc, *kind, ink, *object, k_only, decoded) {
                images.insert(name, why);
            }
            continue;
        }
        for colour in graphics::colours(*kind, uses, ink) {
            spots.extend(colour.spot.iter().map(|s| text(s)));
            if colour.spot.is_empty()
                && let Some(why) = colour_problem(colour, k_only)
            {
                colours.insert(why);
            }
        }
    }
    (images, colours, spots)
}

/// Why a painted colour is not gray, or `None` when it is (or is K-only CMYK and that is allowed).
fn colour_problem(colour: &Fill, k_only: bool) -> Option<String> {
    if !colour.pattern.is_empty() {
        return (colour.pattern != BTreeSet::from([Family::Gray])).then(|| describe(colour));
    }
    match colour.family {
        Family::Gray => None,
        Family::Cmyk if k_only && colour.profiled => Some(format!(
            "{} in an ICC-based or indexed space (not treated as K-only)",
            describe(colour)
        )),
        Family::Cmyk => (!(k_only && is_k_only(colour) && colour.value.len() >= 4)).then(|| describe(colour)),
        Family::Rgb => Some("RGB".to_owned()),
        Family::Other => Some("Lab or another non-gray colour".to_owned()),
    }
}

/// Why an image is not gray, as the rest of a sentence after its name, or `None` when it is.
fn image_problem(
    doc: &Document,
    kind: Kind,
    ink: &Ink,
    object: Option<u32>,
    k_only: bool,
    decoded: &mut BTreeMap<u32, Option<&'static str>>,
) -> Option<&'static str> {
    match ink.space {
        Some(Family::Cmyk) => {}
        Some(Family::Rgb) => return Some("is in an RGB colour space in a gray job"),
        _ => return Some("is not in a gray colour space"),
    }
    if !k_only {
        return Some("is in a CMYK colour space in a gray job");
    }
    // An inline image's abbreviated `/CMYK` is spelled out, as pikepdf does; a resource name or array is not plain.
    let space = ink
        .own_space
        .as_ref()
        .and_then(|s| doc.resolve(s))
        .and_then(Object::name);
    let plain = space == Some(b"DeviceCMYK") || (kind == Kind::InlineImage && space == Some(b"CMYK"));
    if !plain {
        return Some("is ICC-based or indexed CMYK, which is not treated as K-only");
    }
    if kind == Kind::InlineImage {
        return Some("is an inline CMYK image: inline images are not decoded, so K-only cannot be confirmed");
    }
    object.map_or(Some(UNREADABLE), |num| {
        *decoded.entry(num).or_insert_with(|| match cmy_ink(doc, num) {
            None => Some(UNREADABLE),
            Some(ink) => ink.iter().any(|i| *i > 0).then_some("has C, M or Y ink in a gray job"),
        })
    })
}

fn number(object: &Object) -> Option<f64> {
    match object.value {
        #[expect(clippy::cast_precision_loss, reason = "a PDF integer")]
        Value::Integer(n) => Some(n as f64),
        Value::Real(r) => Some(r),
        _ => None,
    }
}

/// The highest cyan, magenta and yellow ink in a `DeviceCMYK` image's 8-bit samples after its `/Decode`; `None` when
/// they cannot be read, as the Python could not: other than 8 bits, a mask with an image filter (Pillow then gives
/// RGBA), an image filter other than a CMYK JPEG or JPEG 2000, run-length data, or a `/Decode` that is not a plain or
/// inverted mapping.
fn cmy_ink(doc: &Document, num: u32) -> Option<[u8; 3]> {
    // The walk found the image as a stream, so its dictionary is there.
    let empty = Dict::default();
    let dict = doc.stream_dict(num).unwrap_or(&empty);
    let get = |key: &[u8]| dict.get(key).and_then(|v| doc.resolve(v));
    // Python's `int()`: 8.5 bits is 8.
    if !(8.0..9.0).contains(&get(b"BitsPerComponent").map_or(Some(0.0), number)?) {
        return None;
    }
    let filters: Vec<&[u8]> = match get(b"Filter").map(|f| &f.value) {
        Some(Value::Array(items)) => items.iter().filter_map(|f| doc.resolve(f)?.name()).collect(),
        Some(Value::Name(name)) => vec![name.as_slice()],
        _ => Vec::new(),
    };
    let samples = if filters
        .iter()
        .any(|f| matches!(*f, b"DCTDecode" | b"JPXDecode" | b"CCITTFaxDecode" | b"JBIG2Decode"))
    {
        if [b"SMask".as_slice(), b"Mask"]
            .iter()
            .any(|k| get(k).is_some_and(|m| m.value != Value::Null))
        {
            return None;
        }
        let data = |jpeg: bool| if jpeg { doc.whole_jpeg(num) } else { doc.image_data(num) };
        let image = |f: &&[u8]| {
            matches!(
                *f,
                b"DCTDecode" | b"DCT" | b"JPXDecode" | b"JBIG2Decode" | b"CCITTFaxDecode" | b"CCF"
            )
        };
        match filters.iter().copied().find(image) {
            Some(b"DCTDecode" | b"DCT") => raster::jpeg_cmyk(&data(true)?)?,
            Some(b"JPXDecode") => raster::jpx_cmyk(&data(false)?)?,
            _ => return None,
        }
    } else if filters.iter().any(|f| matches!(*f, b"RunLengthDecode" | b"RL")) {
        return None;
    } else {
        doc.stream_data(num)?
    };
    let decode: Vec<f64> = match get(b"Decode").map(|d| &d.value) {
        None => [0.0, 1.0].repeat(4),
        Some(Value::Array(items)) => items.iter().map(|v| number(doc.resolve(v)?)).collect::<Option<_>>()?,
        Some(_) => return None,
    };
    if decode.len() != 8 || decode.iter().any(|d| ![0.0, 1.0].contains(d)) {
        return None;
    }
    let mut most = [0u8; 3];
    if samples.is_empty() {
        return Some(most);
    }
    for (c, top) in most.iter_mut().enumerate() {
        let inverted = decode.get(2 * c).is_some_and(|d| *d > 0.5);
        let channel = samples
            .iter()
            .skip(c)
            .step_by(4)
            .map(|b| if inverted { 255 - b } else { *b });
        // A partial last pixel leaves a channel empty: Python's `max()` of nothing raised.
        *top = channel.max()?;
    }
    Some(most)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::Params;
    use crate::checks::test_doc::paged;
    use crate::common::raster::tests::{JP2, JPEG};
    use crate::findings::{Level, Severity};

    /// A stream object of `entries` holding `data`.
    fn stream(entries: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend(data);
        out.extend(b"\nendstream");
        out
    }

    /// An image `XObject`, 1 by 1 unless `entries` say otherwise.
    fn image(entries: &str, data: &[u8]) -> Vec<u8> {
        stream(&format!("/Subtype /Image /Width 1 /Height 1 {entries}"), data)
    }

    /// The test JPEG claiming 65535 by 65535 pixels: whole, but too large to decode.
    fn huge() -> Vec<u8> {
        let mut jpeg = JPEG.to_vec();
        let sof = jpeg.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        jpeg.splice(sof + 5..sof + 9, [0xFF; 4]);
        jpeg
    }

    const CMYK: &str = "/BitsPerComponent 8 /ColorSpace /DeviceCMYK";
    const GOLD: &str = "[/Separation /Gold /DeviceGray << /FunctionType 2 /Domain [0 1] /C1 [0] /N 1 >>]";

    fn objects() -> Vec<(u32, Vec<u8>)> {
        let cmyk = |more: &str, data: &[u8]| image(&format!("{CMYK} {more}"), data);
        let k = [0, 0, 0, 128];
        vec![
            (5, cmyk("", &k)),
            (6, cmyk("", &[1, 0, 0, 128])),
            (7, image("/BitsPerComponent 8 /ColorSpace /DeviceGray", &[0])),
            (8, image("/BitsPerComponent 8 /ColorSpace /DeviceRGB", &[0; 3])),
            (
                9,
                image(
                    "/BitsPerComponent 8 /ColorSpace [/Lab << /WhitePoint [1 1 1] >>]",
                    &[0; 3],
                ),
            ),
            (10, image("/BitsPerComponent 8 /ColorSpace [/ICCBased 30 0 R]", &k)),
            (
                11,
                image(&format!("/Width 8 /Height 8 {CMYK} /Filter /DCTDecode"), &JPEG),
            ),
            (12, cmyk("/Filter [/DCTDecode] /SMask 7 0 R", &JPEG)),
            (13, image("/BitsPerComponent 16 /ColorSpace /DeviceCMYK", &[0; 8])),
            (14, cmyk("/Filter /RL", &[3, 0, 0, 0, 128, 128])),
            (15, cmyk("/Decode [1 0 1 0 1 0 0 1]", &[255, 255, 255, 128])),
            (16, cmyk("/Decode [0 0.5 0 1 0 1 0 1]", &k)),
            (17, image(&format!("/BitsPerComponent 8 /ColorSpace {GOLD}"), &[0])),
            (
                18,
                image(&format!("/Width 8 /Height 8 {CMYK} /Filter /JPXDecode"), &JP2),
            ),
            (19, cmyk("", &[0, 0])),
            (20, cmyk("/Decode 5", &[])),
            (21, cmyk("/Filter /CCITTFaxDecode", &[0])),
            (22, cmyk("/Decode [0 1 0 1 0 1 0 /X]", &k)),
            (23, image("/ColorSpace /DeviceCMYK", &k)),
            (24, image("/BitsPerComponent /X /ColorSpace /DeviceCMYK", &k)),
            (25, cmyk("/Filter /DCT", &JPEG)),
            (26, cmyk("/Decode [0 1]", &k)),
            (27, cmyk("/Filter [/FlateDecode 5] /SMask null", &[])),
            (28, image("/ImageMask true", &[0])),
            (
                29,
                image(&format!("/Width 8 /Height 8 {CMYK} /Filter /JBIG2Decode"), &[0]),
            ),
            (30, stream("/N 4", &[[0; 16].as_slice(), b"CMYK", &[0; 108]].concat())),
            (31, image(&format!("/Width 1 /Height 2 {CMYK}"), &[0; 8])),
            (
                32,
                stream("/PatternType 1 /PaintType 1 /Resources << >>", b"0.5 g 0 0 1 1 re f"),
            ),
            (
                33,
                stream("/PatternType 1 /PaintType 1 /Resources << >>", b"1 0 0 rg 0 0 1 1 re f"),
            ),
            (34, cmyk("/Filter [99 0 R /FlateDecode]", &[0])),
            (35, cmyk("/Filter [/FlateDecode /DCTDecode]", &[0])),
            (36, cmyk("/Filter /DCTDecode", b"junk")),
            (
                40,
                cmyk("/Width 8 /Height 8 /Filter /DCTDecode", JPEG.split_at(JPEG.len() - 2).0),
            ),
            (41, cmyk("/Filter [/FlateDecode /JPXDecode]", &[0])),
            (42, cmyk("/Filter /DCTDecode", &huge())),
            (37, cmyk("/Filter /JPXDecode", b"junk")),
            (38, cmyk("/Decode [99 0 R 1 0 1 0 1 0 1]", &k)),
            (39, cmyk("", &[])),
        ]
    }

    const RESOURCES: &str = "/Resources << /XObject << /K 5 0 R /C 6 0 R /G 7 0 R /R 8 0 R /L 9 0 R /I 10 0 R /J 11 0 R \
        /M 12 0 R /B 13 0 R /U 14 0 R /D 15 0 R /E 16 0 R /S 17 0 R /X 18 0 R /P 19 0 R /F 20 0 R /T 21 0 R /N 22 0 R \
        /O 23 0 R /Q 24 0 R /A 25 0 R /V 26 0 R /W 27 0 R /Y 28 0 R /Z 29 0 R /H 31 0 R /Fa 34 0 R /Fb 35 0 R \
        /Fc 36 0 R /Fd 37 0 R /Fe 38 0 R /Ff 39 0 R /Fg 40 0 R /Fh 41 0 R /Fi 42 0 R >> \
        /Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceGray >> /Sh1 << /ShadingType 2 /ColorSpace /DeviceCMYK >> \
        /Sh2 << /ShadingType 2 /ColorSpace [/Separation /Silver /DeviceGray 99 0 R] >> >> \
        /ColorSpace << /CS0 /DeviceCMYK /CS1 [/ICCBased 30 0 R] /CS2 [/Lab << /WhitePoint [1 1 1] >>] /CS3 SPOT >> \
        /Pattern << /P0 32 0 R /P1 33 0 R >> >>";

    fn found(pages: &[&str], catalog: &str, k_only: bool, mode: ColourMode) -> Vec<(Severity, Option<u32>, String)> {
        let resources = RESOURCES.replace("SPOT", GOLD);
        let pages: Vec<(&str, &str)> = pages.iter().map(|p| (resources.as_str(), *p)).collect();
        let doc = paged(&pages, catalog, &objects());
        let params = Params {
            colour_mode: mode,
            allow_k_only: k_only,
            ..Params::default()
        };
        gray_only(
            &doc,
            &Settings {
                level: Level::Print,
                params,
            },
        )
        .into_iter()
        .map(|f| (f.severity, f.page, f.message))
        .collect()
    }

    fn messages(pages: &[&str], k_only: bool) -> Vec<String> {
        found(pages, "", k_only, ColourMode::Gray)
            .into_iter()
            .map(|f| f.2)
            .collect()
    }

    #[test]
    fn images() {
        let all = "/K Do /C Do /G Do /R Do /L Do /I Do /J Do /M Do /B Do /U Do /D Do /E Do /S Do /X Do /P Do /F Do \
            /T Do /N Do /O Do /Q Do /A Do /V Do /W Do /Z Do /H Do /Fa Do /Fb Do /Fc Do /Fd Do /Fe Do /Ff Do /Fg Do /Fh Do /Fi Do";
        let why = |name: &str, why: &str| format!("image /{name} {why}");
        let (ink, unreadable) = ("has C, M or Y ink in a gray job", UNREADABLE);
        assert_eq!(
            messages(&[all], true),
            [
                why("A", unreadable),
                why("B", unreadable),
                why("C", ink),
                why("E", unreadable),
                why("F", unreadable),
                why("Fa", unreadable),
                why("Fb", unreadable),
                why("Fc", unreadable),
                why("Fd", unreadable),
                why("Fe", unreadable),
                why("Fg", unreadable),
                why("Fh", unreadable),
                why("Fi", unreadable),
                why("I", "is ICC-based or indexed CMYK, which is not treated as K-only"),
                why("J", ink),
                why("L", "is not in a gray colour space"),
                why("M", unreadable),
                why("N", unreadable),
                why("O", unreadable),
                why("P", unreadable),
                why("Q", unreadable),
                why("R", "is in an RGB colour space in a gray job"),
                why("T", unreadable),
                why("U", unreadable),
                why("V", unreadable),
                why("W", unreadable),
                why("X", ink),
                why("Z", unreadable),
                "spot colour(s) Gold have no gray value".to_owned(),
            ]
        );
        assert_eq!(
            messages(&["/K Do /F Do"], false),
            [
                "image /F is in a CMYK colour space in a gray job",
                "image /K is in a CMYK colour space in a gray job"
            ]
        );
    }

    #[test]
    fn inline_images_shadings_and_colours() {
        let page = "BI /W 1 /H 1 /BPC 8 /CS /CMYK ID \0\0\0\0 EI BI /W 1 /H 1 /BPC 8 /CS /CS0 ID \0\0\0\0 EI \
            BI /W 1 /H 1 /BPC 8 /CS /G ID \0 EI /Sh0 sh /Sh1 sh /Sh2 sh /Nope sh 1 0 0 rg /Y Do";
        assert_eq!(
            messages(&[page], true),
            [
                "image (inline) is ICC-based or indexed CMYK, which is not treated as K-only",
                "artwork is painted outside the gray family (RGB, shading /Nope, shading /Sh1...)",
                "spot colour(s) Silver have no gray value",
            ]
        );
        let inline = "BI /W 1 /H 1 /BPC 8 /CS /DeviceCMYK ID \0\0\0\0 EI";
        assert_eq!(
            messages(&[inline, "BI /W 1 /H 1 /BPC 8 /CS /CMYK ID \0\0\0\0 EI"], true),
            [
                "image (inline) is an inline CMYK image: inline images are not decoded, so K-only cannot be confirmed",
                "image (inline) is an inline CMYK image: inline images are not decoded, so K-only cannot be confirmed",
            ]
        );
        let fill = |paint: &str| format!("{paint} 0 0 1 1 re f");
        let colours = [
            fill("0.1 0 0 1 k"),
            fill("0 0 0 1 k"),
            fill("0.5 g"),
            fill("/CS1 cs 0 0 0 1 scn"),
            fill("/CS2 cs 50 0 0 scn"),
            fill("/CS3 cs 1 scn"),
            fill("/Pattern cs /P0 scn"),
            fill("/Pattern cs /P1 scn"),
        ];
        let colours: Vec<&str> = colours.iter().map(String::as_str).collect();
        let k_only = messages(&colours, true);
        let strict = messages(&colours, false);
        let artwork = |what: &str| format!("artwork is painted outside the gray family ({what}...)");
        assert_eq!(
            k_only,
            [
                artwork("C10 M0 Y0 K100"),
                artwork("C0 M0 Y0 K100 in an ICC-based or indexed space (not treated as K-only)"),
                artwork("Lab or another non-gray colour"),
                "spot colour(s) Gold have no gray value".to_owned(),
                artwork("a pattern using RGB"),
            ]
        );
        assert_eq!(strict.get(1), Some(&artwork("C0 M0 Y0 K100")));
        assert_eq!(strict.get(2), Some(&artwork("C0 M0 Y0 K100")));
    }

    #[test]
    fn intents_modes_and_pages() {
        let intent = |n: u8| format!("/OutputIntents [<< /DestOutputProfile << /N {n} >> >>]");
        let gray = |catalog: &str, k_only: bool| found(&[""], catalog, k_only, ColourMode::Gray);
        let error = |m: &str| vec![(Severity::Error, None, m.to_owned())];
        assert_eq!(
            gray(&intent(3), true),
            error("the output intent is an RGB profile in a gray job")
        );
        assert_eq!(
            gray(&intent(4), false),
            error("the output intent is a CMYK profile in a gray job")
        );
        assert_eq!(gray(&intent(4), true), []);
        assert_eq!(gray(&intent(1), false), []);
        assert_eq!(
            found(&["/C Do 1 0 0 rg /S Do"], "", true, ColourMode::Cmyk),
            [],
            "not a gray job"
        );
        let pages = found(&["/C Do /C Do", "/C Do"], "", true, ColourMode::Gray);
        let pages: Vec<(Severity, Option<u32>)> = pages.into_iter().map(|f| (f.0, f.1)).collect();
        assert_eq!(pages, [(Severity::Error, Some(1)), (Severity::Error, Some(2))]);
    }

    #[test]
    fn an_image_is_decoded_once() {
        let resources = "/Resources << /XObject << /C 6 0 R /K 5 0 R >> >>";
        let doc = paged(&[(resources, "/C Do /C Do /K Do")], "", &objects());
        let mut decoded = BTreeMap::new();
        for (_, page) in pages(&doc) {
            painted(&doc, &page, true, &mut decoded);
        }
        assert_eq!(
            decoded,
            BTreeMap::from([(5, None), (6, Some("has C, M or Y ink in a gray job"))])
        );
    }
}
