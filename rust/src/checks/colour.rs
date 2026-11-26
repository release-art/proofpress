//! PDF/X-4 (ISO 15930-7) colour rows on what the pages paint (X4-Q3: print elements only): device spaces against the
//! output intent (6.4.3.2, with the `Default` spaces of 6.4.3.1 applied by tier 7), ICC profiles (6.4.3.3) and the
//! alternate spaces of spot colours (6.4.3.4, X4-Q4).
//!
//! Each colour space a paint uses is flattened into its leaves: an `Indexed` or `Pattern` space into its base, a
//! `Separation` or `DeviceN` into itself, its alternate and the separations of its `/Colorants`.

use super::Settings;
use std::collections::BTreeSet;

use super::{intent, jpx, x4p};
use crate::common::icc;
use crate::document::{Document, Object, Value};
use crate::findings::{
    Check, Finding, Location, X4_COLOUR_DEVICE_SPACE_MATCHES_INTENT, X4_ICC_CMYK_NOT_INTENT_PROFILE,
    X4_ICC_OVERPRINT_MODE_ZERO, X4_ICC_PROFILE_VERSION, X4_PRE_SEPARATED_ABSENT, X4_SPOT_ALTERNATE_SPACE_RESTRICTED,
    X4_TRANSPARENCY_GROUP_CS_RESTRICTED, X5_N_DEVICECMYK_NEEDS_CMYK, X5_N_DEVICEGRAY_NEEDS_BLACK, X5_N_NO_DEVICERGB,
};
use crate::graphics::{self, Event, Role, Use};

/// How a leaf was reached from the space a paint uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Via {
    /// The space itself.
    Direct,
    /// The base of an `Indexed` or `Pattern` space.
    Base,
    /// The alternate of a `Separation` or `DeviceN` space, or a separation in its `/Colorants`.
    Alternate,
}

/// A leaf colour space, resolved, with how it was reached.
struct Leaf<'a> {
    space: &'a Object,
    via: Via,
}

/// How deep spaces may nest before the flattening stops; real ones nest two or three levels.
const DEPTH: usize = 8;

fn leaves<'a>(doc: &'a Document, space: &'a Object, via: Via, depth: usize, out: &mut Vec<Leaf<'a>>) {
    let Some(resolved) = doc.resolve(space).filter(|_| depth < DEPTH) else {
        return;
    };
    out.push(Leaf { space: resolved, via });
    let Some(items) = resolved.array() else { return };
    let inner = |v: Via| if via == Via::Direct { v } else { via };
    let deeper = |i: usize, v: Via, out: &mut Vec<Leaf<'a>>| {
        if let Some(next) = items.get(i) {
            leaves(doc, next, inner(v), depth + 1, out);
        }
    };
    match items.first().and_then(Object::name) {
        Some(b"Indexed" | b"I" | b"Pattern") => deeper(1, Via::Base, out),
        Some(b"Separation") => deeper(2, Via::Alternate, out),
        Some(b"DeviceN") => {
            deeper(2, Via::Alternate, out);
            let colorants = items
                .get(4)
                .and_then(|a| doc.resolve(a))
                .and_then(Object::dict)
                .and_then(|a| a.get(b"Colorants"))
                .and_then(|c| doc.resolve(c))
                .and_then(Object::dict);
            for (_, separation) in colorants.map_or_default(|c| c.entries.as_slice()) {
                leaves(doc, separation, Via::Alternate, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// The family of a resolved space: its name, or its array's first name.
fn family(space: &Object) -> &[u8] {
    space
        .array()
        .and_then(|a| a.first())
        .unwrap_or(space)
        .name()
        .unwrap_or_default()
}

/// Every use of a colour space in the pages, with where it was painted.
fn uses(doc: &Document) -> Vec<(Use, Vec<Location>)> {
    let mut out = Vec::new();
    for event in graphics::walk(doc) {
        if let Event::Paint { uses, frames, .. } = event {
            out.extend(uses.into_iter().map(|u| (u, frames.clone())));
        }
    }
    out
}

/// One finding per distinct message and place.
fn distinct(findings: Vec<Finding>) -> Vec<Finding> {
    let mut seen = BTreeSet::new();
    findings
        .into_iter()
        .filter(|f| seen.insert((f.message.clone(), format!("{:?}", f.chain))))
        .collect()
}

fn finding(check: &'static Check, message: String, frames: &[Location]) -> Finding {
    Finding::new(check, message)
        .citing(if check == X4_SPOT_ALTERNATE_SPACE_RESTRICTED {
            "6.4.3.4"
        } else {
            "6.4.3.2"
        })
        .at(frames.to_vec())
}

/// Whether a device space may be used under an intent whose data colour space is `intent` (6.4.3.2).
fn device_allowed(device: &[u8], intent: [u8; 4]) -> bool {
    matches!(
        (device, &intent),
        (b"DeviceGray", b"GRAY" | b"CMYK") | (b"DeviceRGB", b"RGB ") | (b"DeviceCMYK", b"CMYK")
    )
}

/// The device-space rule on the leaves reached `which` way.
fn device_rule(
    doc: &Document,
    settings: &Settings,
    check: &'static Check,
    which: &dyn Fn(Via) -> bool,
) -> Vec<Finding> {
    // Under an intent that is not gray, RGB or CMYK nothing can match; x4.output-intent.channels says why.
    let Some(intent) = intent::space(doc, settings).filter(|s| matches!(s, b"GRAY" | b"RGB " | b"CMYK")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (used, frames) in uses(doc) {
        let mut found = Vec::new();
        leaves(doc, &used.space, Via::Direct, 0, &mut found);
        for leaf in found.iter().filter(|l| which(l.via)) {
            let device = family(leaf.space);
            if device.starts_with(b"Device") && !matches!(device, b"DeviceN") && !device_allowed(device, intent) {
                let intent_name = String::from_utf8_lossy(&intent).trim_end().to_owned();
                let what = if leaf.via == Via::Alternate {
                    "a spot colour's alternate"
                } else {
                    "a print element"
                };
                let message = format!(
                    "{what} uses {} under a {intent_name} output intent",
                    String::from_utf8_lossy(device)
                );
                out.push(finding(check, message, &frames));
            }
        }
    }
    distinct(out)
}

/// 6.4.3.2: device spaces in print elements match the intent; `DeviceGray` is also allowed under a CMYK intent.
pub(super) fn device_spaces(doc: &Document, settings: &Settings) -> Vec<Finding> {
    device_rule(doc, settings, X4_COLOUR_DEVICE_SPACE_MATCHES_INTENT, &|v| {
        v != Via::Alternate
    })
}

/// 6.4.3.4 (X4-Q4): the alternates of spot colours obey 6.4 too.
pub(super) fn alternate_spaces(doc: &Document, settings: &Settings) -> Vec<Finding> {
    device_rule(doc, settings, X4_SPOT_ALTERNATE_SPACE_RESTRICTED, &|v| {
        v == Via::Alternate
    })
}

/// 6.20: a transparency group's `/CS` obeys 6.4.3: a device space matches the intent, a profile is version 2 or 4.
pub(super) fn group_spaces(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let intent = intent::space(doc, settings).filter(|s| matches!(s, b"GRAY" | b"RGB " | b"CMYK"));
    let mut out = Vec::new();
    for event in graphics::walk(doc) {
        let Event::Group {
            space: Some(space),
            frames,
        } = event
        else {
            continue;
        };
        let mut found = Vec::new();
        leaves(doc, &space, Via::Direct, 0, &mut found);
        for leaf in &found {
            let family = family(leaf.space);
            let device = family.starts_with(b"Device") && family != b"DeviceN";
            let bad_device = device && intent.is_some_and(|i| !device_allowed(family, i));
            let version = (family == b"ICCBased")
                .then(|| leaf.space.array().and_then(|a| a.get(1)))
                .flatten()
                .and_then(|s| match s.value {
                    Value::Reference(n, _) => doc.stream_data(n),
                    _ => None,
                })
                .and_then(|d| icc::header(&d))
                .map(|h| h.version.0);
            if bad_device || version.is_some_and(|v| !matches!(v, 2 | 4)) {
                let message = format!(
                    "a transparency group's /CS uses {}, which 6.4.3 does not allow here",
                    String::from_utf8_lossy(family)
                );
                out.push(
                    Finding::new(X4_TRANSPARENCY_GROUP_CS_RESTRICTED, message)
                        .citing("6.20")
                        .at(frames.clone()),
                );
            }
        }
    }
    distinct(out)
}

/// An `ICCBased` leaf of a use: its profile stream's number and data, its channels, the use, how it was reached and
/// where it was painted.
struct IccLeaf {
    num: Option<u32>,
    data: Option<Vec<u8>>,
    channels: usize,
    used: Use,
    via: Via,
    frames: Vec<Location>,
}

/// Every `ICCBased` leaf of every use.
fn icc_leaves(doc: &Document) -> Vec<IccLeaf> {
    let mut out = Vec::new();
    for (used, frames) in uses(doc) {
        let mut found = Vec::new();
        leaves(doc, &used.space, Via::Direct, 0, &mut found);
        for leaf in found.iter().filter(|l| family(l.space) == b"ICCBased") {
            let stream = leaf.space.array().and_then(|a| a.get(1));
            let num = stream.and_then(|s| match s.value {
                Value::Reference(n, _) => Some(n),
                _ => None,
            });
            let data = num.and_then(|n| doc.stream_data(n));
            let channels = graphics::components(doc, leaf.space).unwrap_or(0);
            out.push(IccLeaf {
                num,
                data,
                channels,
                used: used.clone(),
                via: leaf.via,
                frames: frames.clone(),
            });
        }
    }
    out
}

/// 6.4.3.3: every ICC profile used is version 2 or 4 (decision log, 2026-11-25).
pub(super) fn profile_version(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for IccLeaf { num, data, frames, .. } in icc_leaves(doc) {
        let Some(header) = data.as_deref().and_then(icc::header) else {
            continue;
        };
        let (major, minor) = header.version;
        if !matches!(major, 2 | 4) {
            let what = num.map_or_else(String::new, |n| format!(" {n}"));
            let message = format!("the ICC profile{what} is version {major}.{minor}, not 2 or 4");
            out.push(
                Finding::new(X4_ICC_PROFILE_VERSION, message)
                    .citing("6.4.3.3")
                    .at(frames),
            );
        }
    }
    distinct(out)
}

/// 6.4.3.3: a four-colour `ICCBased` fill, stroke, text or image mask that overprints under a CMYK intent has overprint
/// mode 0. A `DefaultCMYK` standing in for `DeviceCMYK` is left out: it always knocks out (6.4.3.1, note 2).
pub(super) fn overprint_mode(doc: &Document, settings: &Settings) -> Vec<Finding> {
    if intent::space(doc, settings) != Some(*b"CMYK") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for IccLeaf {
        channels,
        used,
        via,
        frames,
        ..
    } in icc_leaves(doc)
    {
        let painted = matches!(used.role, Role::Fill | Role::Stroke | Role::ImageMask);
        if channels == 4 && via != Via::Alternate && painted && used.overprint && used.opm != 0 && !used.defaulted {
            let message = format!("a four-colour ICCBased paint overprints with OPM {}", used.opm);
            out.push(
                Finding::new(X4_ICC_OVERPRINT_MODE_ZERO, message)
                    .citing("6.4.3.3")
                    .at(frames),
            );
        }
    }
    distinct(out)
}

/// 6.4.3.3: no four-colour `ICCBased` space uses the output intent's profile: the same stream, or the same identity.
pub(super) fn not_intent_profile(doc: &Document, _: &Settings) -> Vec<Finding> {
    let Some((intent_num, intent_data)) = intent::profile_stream(doc) else {
        return Vec::new();
    };
    let intent_id = icc::identity(&intent_data);
    let same = |num: Option<u32>, data: Option<&[u8]>| {
        Some(num == Some(intent_num) || (intent_id.is_some() && data.and_then(icc::identity) == intent_id))
    };
    same_as_intent(doc, &same).0
}

/// 6.4.3.3 at the level being verified. At a level built on PDF/X-4p the intent's profile is the external one, compared by identity;
/// without it, a profile whose MD5 is the `/CheckSum` is the intent's, and none is when the intent is not CMYK
/// (pdfx-oracle, 2026-11-26). Otherwise it is not evaluated.
pub(super) fn not_intent_profile_at_level(doc: &Document, settings: &Settings) -> Result<Vec<Finding>, String> {
    if !x4p::external_profile_level(settings.level) {
        return Ok(not_intent_profile(doc, settings));
    }
    match x4p::profile(doc, settings) {
        Ok((_, external)) => {
            let id = icc::identity(&external);
            Ok(same_as_intent(doc, &|_, data| Some(id.is_some() && data.and_then(icc::identity) == id)).0)
        }
        Err(why) => {
            let same = |_, data: Option<&[u8]>| data.and_then(|d| x4p::is_intent_without_profile(doc, d));
            match same_as_intent(doc, &same) {
                (found, false) => Ok(found),
                (_, true) => Err(why),
            }
        }
    }
}

/// Whether a profile, by object number and data, is the intent's: `None` when that cannot be told.
type Same<'a> = dyn Fn(Option<u32>, Option<&[u8]>) -> Option<bool> + 'a;

/// The four-colour `ICCBased` spaces `same` says use the intent's profile, and whether `same` could not tell for one.
fn same_as_intent(doc: &Document, same: &Same<'_>) -> (Vec<Finding>, bool) {
    let mut out = Vec::new();
    let mut unknown = false;
    for IccLeaf {
        num,
        data,
        channels,
        used,
        frames,
        ..
    } in icc_leaves(doc)
    {
        if channels != 4 {
            continue;
        }
        match same(num, data.as_deref()) {
            Some(true) => {
                let how = if used.defaulted {
                    "a DefaultCMYK"
                } else {
                    "a four-colour ICCBased space"
                };
                let message = format!("{how} uses the output intent's profile");
                out.push(
                    Finding::new(X4_ICC_CMYK_NOT_INTENT_PROFILE, message)
                        .citing("6.4.3.3")
                        .at(frames),
                );
            }
            Some(false) => {}
            None => unknown = true,
        }
    }
    (distinct(out), unknown)
}

/// The one colorant a colour space paints, when it paints only one: gray for a one-channel space, the name of a
/// `Separation`, the ink of a one-ink `DeviceN`, an `Indexed` space's base's. `None` for spaces with more.
fn single_ink(doc: &Document, space: &Object, depth: usize) -> Option<Vec<u8>> {
    let resolved = doc.resolve(space).filter(|_| depth < DEPTH)?;
    let items = resolved.array().unwrap_or_default();
    match family(resolved) {
        b"Separation" => items.get(1)?.name().map(<[u8]>::to_vec),
        b"DeviceN" => match doc.resolve(items.get(1)?)?.array()? {
            [one] => one.name().map(<[u8]>::to_vec),
            _ => None,
        },
        b"Indexed" | b"I" => single_ink(doc, items.get(1)?, depth + 1),
        _ => (graphics::components(doc, resolved) == Some(1)).then(|| b"Gray".to_vec()),
    }
}

/// The single colorants a page's paints use; `None` stands for a paint with more than one.
type Inks = BTreeSet<Option<Vec<u8>>>;

/// 6.1.2 (X4-Q2): the file is not pre-separated: it fails only when every page that paints uses a single colorant and
/// those colorants differ between pages.
pub(super) fn pre_separated(doc: &Document, _: &Settings) -> Vec<Finding> {
    // Paints grouped by the page they are on, the first of their frames.
    let mut pages: Vec<(Option<Location>, Inks)> = Vec::new();
    for event in graphics::walk(doc) {
        let Event::Paint { uses, frames, .. } = event else {
            continue;
        };
        let page = frames.into_iter().next();
        let inks: Inks = uses.iter().map(|u| single_ink(doc, &u.space, 0)).collect();
        match pages.iter_mut().find(|(p, _)| *p == page) {
            Some((_, seen)) => seen.extend(inks),
            None => pages.push((page, inks)),
        }
    }
    let single: Vec<&Vec<u8>> = pages
        .iter()
        .filter_map(|(_, inks)| match inks.iter().collect::<Vec<_>>().as_slice() {
            [Some(ink)] => Some(ink),
            _ => None,
        })
        .collect();
    let distinct: BTreeSet<&&Vec<u8>> = single.iter().collect();
    if pages.len() < 2 || single.len() < pages.len() || distinct.len() < 2 {
        return Vec::new();
    }
    let names: Vec<String> = distinct
        .iter()
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .collect();
    let message = format!(
        "every page paints a single colorant, and they differ: {}",
        names.join(", ")
    );
    vec![Finding::new(X4_PRE_SEPARATED_ABSENT, message).citing("6.1.2")]
}

/// ISO 15930-8 7.3 under PDF/X-5n: a device space is used only when `/ColorantTable` names the colorants it stands
/// for (`needed`; none means never). Print elements, the alternates they reach (X5-Q10) and transparency groups count.
fn x5n_device(doc: &Document, check: &'static Check, device: &[u8], needed: &[&[u8]], why: &str) -> Vec<Finding> {
    let table = x4p::colorant_table(doc).unwrap_or_default();
    if !needed.is_empty() && needed.iter().all(|n| table.contains(n)) {
        return Vec::new();
    }
    let mut spaces: Vec<(Object, Vec<Location>, &str)> = uses(doc)
        .into_iter()
        .map(|(used, frames)| (used.space, frames, "a print element"))
        .collect();
    for event in graphics::walk(doc) {
        if let Event::Group {
            space: Some(space),
            frames,
        } = event
        {
            spaces.push((space, frames, "a transparency group"));
        }
    }
    let mut out = Vec::new();
    for (space, frames, what) in &spaces {
        let mut found = Vec::new();
        leaves(doc, space, Via::Direct, 0, &mut found);
        for leaf in found.iter().filter(|l| family(l.space) == device) {
            let what = if leaf.via == Via::Alternate {
                "a spot colour's alternate"
            } else {
                what
            };
            let message = format!("{what} uses {}, {why}", String::from_utf8_lossy(device));
            out.push(Finding::new(check, message).citing("7.3").at(frames.clone()));
        }
    }
    distinct(out)
}

/// ISO 15930-8 7.3: `DeviceGray` stands for a colorant named `Black`, and without one is not used.
pub(super) fn x5n_gray(doc: &Document, _: &Settings) -> Vec<Finding> {
    let why = "but /ColorantTable has no Black colorant";
    x5n_device(doc, X5_N_DEVICEGRAY_NEEDS_BLACK, b"DeviceGray", &[b"Black"], why)
}

/// ISO 15930-8 7.3: `DeviceCMYK` stands for colorants named `Cyan`, `Magenta`, `Yellow` and `Black`, and without them is
/// not used. CMYK JPEG 2000 data read as `DeviceCMYK` (ISO 15930-7 6.27) counts too.
pub(super) fn x5n_cmyk(doc: &Document, _: &Settings) -> Vec<Finding> {
    let why = "but /ColorantTable does not have Cyan, Magenta, Yellow and Black";
    let process: [&[u8]; 4] = [b"Cyan", b"Magenta", b"Yellow", b"Black"];
    let mut out = x5n_device(doc, X5_N_DEVICECMYK_NEEDS_CMYK, b"DeviceCMYK", &process, why);
    let table = x4p::colorant_table(doc).unwrap_or_default();
    if !process.iter().all(|n| table.contains(n)) {
        out.extend(jpx::device_cmyk(doc, X5_N_DEVICECMYK_NEEDS_CMYK, "7.3", why));
    }
    out
}

/// ISO 15930-8 7.3: `DeviceRGB` is never used.
pub(super) fn x5n_rgb(doc: &Document, _: &Settings) -> Vec<Finding> {
    x5n_device(
        doc,
        X5_N_NO_DEVICERGB,
        b"DeviceRGB",
        &[],
        "which PDF/X-5n does not allow",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    const RUNS: [fn(&Document, &Settings) -> Vec<Finding>; 5] = [
        device_spaces,
        alternate_spaces,
        profile_version,
        overprint_mode,
        not_intent_profile,
    ];

    fn profile(space: &[u8], version: u8) -> Vec<u8> {
        let mut icc = vec![0u8; 128];
        icc[8] = version;
        icc.get_mut(16..20).unwrap().copy_from_slice(space);
        icc.get_mut(36..40).unwrap().copy_from_slice(b"acsp");
        let mut out = format!("<< /N 4 /Length {} >>\nstream\n", icc.len()).into_bytes();
        out.extend(icc);
        out.extend(b"\nendstream");
        out
    }

    #[test]
    fn under_pdf_x_4p_without_its_profile_an_unknown_cmyk_profile_is_not_evaluated() {
        let catalog = format!(
            "<< /Pages 2 0 R /OutputIntents [<< /S /GTS_PDFX /DestOutputProfileRef << /ProfileCS (CMYK) /CheckSum <{}> \
             >> >>] >>",
            "00".repeat(16)
        );
        let page = b"<< /Type /Page /Contents 4 0 R /Resources << /ColorSpace << /A [/ICCBased 7 0 R] \
                     /B [/ICCBased 9 0 R] >> >> >>";
        let data = "/A cs 0 0 0 sc 0 0 1 1 re f /B cs 0 0 0 1 sc 0 0 1 1 re f";
        let content = format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len());
        let rgb = String::from_utf8_lossy(&profile(b"RGB ", 2)).replace("/N 4", "/N 3");
        let objects: [(u32, &[u8]); 6] = [
            (1, catalog.as_bytes()),
            (2, b"<< /Type /Pages /Kids [3 0 R] >>"),
            (3, page),
            (4, content.as_bytes()),
            (7, rgb.as_bytes()),
            (9, &profile(b"CMYK", 2)),
        ];
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        assert_eq!(
            not_intent_profile_at_level(&doc, &Level::X4p.into()),
            Err("the file is verified alone, without its external profile".to_owned())
        );
    }

    fn ids(intent: &[u8], resources: &str, data: &str) -> Vec<&'static str> {
        let catalog = b"<< /Pages 2 0 R /OutputIntents [<< /S /GTS_PDFX /DestOutputProfile 8 0 R >>] >>";
        let page = format!("<< /Type /Page /Contents 4 0 R /Group << /S /Transparency >> /Resources {resources} >>");
        let content = format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len());
        let objects: [(u32, &[u8]); 7] = [
            (1, catalog),
            (2, b"<< /Type /Pages /Kids [3 0 R] >>"),
            (3, page.as_bytes()),
            (4, content.as_bytes()),
            (7, &profile(b"CMYK", 5)),
            (8, intent),
            (9, &profile(b"CMYK", 2)),
        ];
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        RUNS.iter()
            .flat_map(|run| run(&doc, &Level::X4.into()))
            .map(|f| f.check.id)
            .collect()
    }

    #[test]
    fn spaces_reached_every_way() {
        let cmyk = profile(b"CMYK", 2);
        let spaces = "<< /ColorSpace << /N [/DeviceN [/A /B] /DeviceCMYK 6 0 R << /Colorants << /A [/Separation /A /DeviceRGB 6 0 R] /B 5 >> >>] \
            /I [/Indexed [/Indexed /DeviceRGB 0 <000000>] 0 <00>] /D [/DeviceN [/A] /DeviceCMYK 6 0 R 5] /E [/DeviceN [/A] /DeviceCMYK 6 0 R << /Colorants 5 >>] \
            /V [/ICCBased 7 0 R] /W [/ICCBased << /N 4 >>] /X [/Indexed /Missing 0 <00>] /Y [/Indexed] /O [/ICCBased 8 0 R] /DefaultCMYK [/ICCBased 8 0 R] >> \
            /ExtGState << /OP << /OP true /OPM 1 >> >> >>";
        let got = ids(
            &cmyk,
            spaces,
            "/N cs f /I cs f /D cs f /E cs f /V cs f /W cs f /X cs f /Y cs f /OP gs /O cs f 0 0 0 1 k f",
        );
        assert_eq!(
            got,
            [
                "x4.colour.device-space-matches-intent",
                "x4.spot.alternate-space-restricted",
                "x4.icc.profile-version",
                "x4.icc.overprint-mode-zero",
                "x4.icc.cmyk-not-intent-profile",
                "x4.icc.cmyk-not-intent-profile",
            ]
        );
    }

    #[test]
    fn intents_that_change_the_rules() {
        let rgb = profile(b"RGB ", 2);
        assert_eq!(
            ids(&rgb, "<< >>", "0 g f 1 0 0 rg f"),
            ["x4.colour.device-space-matches-intent"],
            "DeviceGray only under CMYK"
        );
        let gs = "<< /ColorSpace << /V [/ICCBased 9 0 R] >> /ExtGState << /OP << /OP true /OPM 1 >> >> >>";
        assert_eq!(
            ids(&rgb, gs, "/OP gs /V cs f"),
            Vec::<&str>::new(),
            "the OPM rule needs a CMYK intent"
        );
        let lab = profile(b"Lab ", 2);
        assert_eq!(
            ids(&lab, "<< >>", "1 0 0 rg f"),
            Vec::<&str>::new(),
            "nothing matches a Lab intent"
        );
        assert_eq!(
            ids(b"<< >>", "<< >>", "1 0 0 rg f"),
            Vec::<&str>::new(),
            "no readable intent"
        );
        let deep = format!(
            "<< /ColorSpace << /X {} >> >>",
            "[/Indexed ".repeat(10) + "/DeviceRGB" + &" 0 <00>]".repeat(10)
        );
        assert_eq!(
            ids(&profile(b"CMYK", 2), &deep, "/X cs f"),
            Vec::<&str>::new(),
            "deeper than the flattening goes"
        );
    }

    #[test]
    fn group_spaces_follow_the_colour_rules() {
        let groups = |cs: &str| -> Vec<String> {
            let catalog = b"<< /Pages 2 0 R /OutputIntents [<< /S /GTS_PDFX /DestOutputProfile 8 0 R >>] >>";
            let page = format!("<< /Type /Page /Group << /S /Transparency /CS {cs} >> /Contents 4 0 R >>");
            let objects: [(u32, &[u8]); 6] = [
                (1, catalog),
                (2, b"<< /Type /Pages /Kids [3 0 R] >>"),
                (3, page.as_bytes()),
                (4, b"<< /Length 0 >>\nstream\n\nendstream"),
                (7, &profile(b"CMYK", 5)),
                (8, &profile(b"CMYK", 2)),
            ];
            let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
            group_spaces(&doc, &Level::X4.into())
                .into_iter()
                .map(|f| f.message)
                .collect()
        };
        assert_eq!(groups("/DeviceCMYK"), Vec::<String>::new());
        assert_eq!(groups("/DeviceGray"), Vec::<String>::new());
        assert_eq!(groups("[/ICCBased 8 0 R]"), Vec::<String>::new());
        assert_eq!(
            groups("[/ICCBased 7 0 R]"),
            ["a transparency group's /CS uses ICCBased, which 6.4.3 does not allow here"]
        );
        assert_eq!(
            groups("[/ICCBased << /N 4 >>]"),
            Vec::<String>::new(),
            "a profile that is not a stream"
        );
        assert_eq!(groups("[/ICCBased]"), Vec::<String>::new());
        assert_eq!(
            groups("/DeviceRGB"),
            ["a transparency group's /CS uses DeviceRGB, which 6.4.3 does not allow here"]
        );
        assert_eq!(groups("/Lab"), Vec::<String>::new());
    }

    #[test]
    fn pages_that_each_paint_one_colorant() {
        let separated = |contents: &[&str]| -> Vec<String> {
            let spaces = "<< /ColorSpace << /C [/Separation /Cyan /DeviceCMYK 9 0 R] /N [/DeviceN [/Spot] /DeviceCMYK 9 0 R] \
                /M [/DeviceN [/A /B] /DeviceCMYK 9 0 R] /I [/Indexed [/Separation /Cyan /DeviceCMYK 9 0 R] 1 <00ff>] \
                /G [/CalGray << >>] /X [/DeviceN 5 /DeviceCMYK 9 0 R] /S [/Separation 5 /DeviceCMYK 9 0 R] \
                /E [/Separation] /F [/DeviceN] /H [/DeviceN 99 0 R] /J [/Indexed] /K [/Indexed 98 0 R 0 <00>] /L 7 0 R >> >>";
            let mut objects: Vec<(u32, Vec<u8>)> = vec![
                (1, b"<< /Pages 2 0 R >>".to_vec()),
                (9, b"<< /FunctionType 2 >>".to_vec()),
                (7, b"[/Indexed 7 0 R 0 <00>]".to_vec()),
            ];
            let kids: Vec<String> = (0..contents.len()).map(|i| format!("{} 0 R", 10 + 2 * i)).collect();
            objects.push((
                2,
                format!("<< /Type /Pages /Kids [{}] /Resources {spaces} >>", kids.join(" ")).into_bytes(),
            ));
            for (i, data) in contents.iter().enumerate() {
                let n = u32::try_from(10 + 2 * i).unwrap();
                let page = format!(
                    "<< /Type /Page /Group << /S /Transparency >> /Contents {} 0 R >>",
                    n + 1
                );
                objects.push((n, page.into_bytes()));
                objects.push((
                    n + 1,
                    format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()).into_bytes(),
                ));
            }
            let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
            let doc = Document::open(&classic(&refs, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
            pre_separated(&doc, &Level::X4.into())
                .into_iter()
                .map(|f| f.message)
                .collect()
        };
        assert_eq!(
            separated(&["/C cs f", "/N cs f", "0 g f /G cs f"]),
            ["every page paints a single colorant, and they differ: Cyan, Gray, Spot"]
        );
        assert_eq!(
            separated(&["/I cs f", "/N cs f"]),
            ["every page paints a single colorant, and they differ: Cyan, Spot"],
            "an Indexed base"
        );
        assert_eq!(
            separated(&["/C cs f", "/C cs f"]),
            Vec::<String>::new(),
            "the same colorant on every page"
        );
        assert_eq!(separated(&["/C cs f"]), Vec::<String>::new(), "one page");
        assert_eq!(
            separated(&["/C cs f", "/M cs f"]),
            Vec::<String>::new(),
            "a page with two inks"
        );
        assert_eq!(
            separated(&["/C cs f", "0 0 0 1 k f"]),
            Vec::<String>::new(),
            "a page in CMYK"
        );
        assert_eq!(
            separated(&["/C cs f", "/X cs f", "/S cs f", ""]),
            Vec::<String>::new(),
            "odd spaces, and a blank page"
        );
        assert_eq!(
            separated(&["/C cs f", "/N cs f /G cs f"]),
            Vec::<String>::new(),
            "a page with a spot and gray"
        );
        let odd = ["/E cs f", "/F cs f", "/H cs f", "/J cs f", "/K cs f", "/L cs f"];
        for page in odd {
            assert_eq!(
                separated(&["/C cs f", page]),
                Vec::<String>::new(),
                "{page} has no single colorant"
            );
        }
    }

    /// A one-page file whose PDF/X intent is `intent`, with page `resources`, content `data` and `more` objects.
    fn x5n_doc(intent: &str, resources: &str, data: &str, more: &[(u32, Vec<u8>)]) -> Document {
        let catalog = format!("<< /Pages 2 0 R /OutputIntents [<< /S /GTS_PDFX {intent} >>] >>");
        let page = format!(
            "<< /Type /Page /Contents 4 0 R /Group << /S /Transparency /CS /DeviceRGB >> /Resources {resources} >>"
        );
        let content = format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len());
        let mut objects: Vec<(u32, &[u8])> = vec![
            (1, catalog.as_bytes()),
            (2, b"<< /Type /Pages /Kids [3 0 R] >>"),
            (3, page.as_bytes()),
            (4, content.as_bytes()),
        ];
        objects.extend(more.iter().map(|(n, b)| (*n, b.as_slice())));
        Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap()
    }

    #[test]
    fn pdf_x_5n_device_spaces_stand_for_colorants() {
        let intent = "/DestOutputProfileRef << /ColorantTable [/Black /Orange] >>";
        let resources = "<< /ColorSpace << /S [/Separation /Orange /DeviceRGB << /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 1 1] /N 1 >>] >> >>";
        let doc = x5n_doc(
            intent,
            resources,
            "/S cs 1 sc 0 0 1 1 re f 0 g 0 0 1 1 re f 0 0 0 1 k 0 0 1 1 re f",
            &[],
        );
        let found = |run: fn(&Document, &Settings) -> Vec<Finding>| -> Vec<String> {
            run(&doc, &Level::X5n.into()).into_iter().map(|f| f.message).collect()
        };
        assert_eq!(found(x5n_gray), Vec::<String>::new());
        assert_eq!(
            found(x5n_cmyk),
            ["a print element uses DeviceCMYK, but /ColorantTable does not have Cyan, Magenta, Yellow and Black"]
        );
        assert_eq!(
            found(x5n_rgb),
            [
                "a spot colour's alternate uses DeviceRGB, which PDF/X-5n does not allow",
                "a transparency group uses DeviceRGB, which PDF/X-5n does not allow"
            ]
        );
    }

    #[test]
    fn under_pdf_x_4p_the_external_profile_is_the_intent_s() {
        let folder = std::env::temp_dir().join(format!("proof-pdf-colour-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let stream = profile(b"CMYK", 2);
        let data = &stream[stream.iter().position(|b| *b == b'\n').unwrap() + 8..stream.len() - 10];
        std::fs::write(folder.join("p.icc"), data).unwrap();
        let sum = md5::compute(data)
            .0
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .concat();
        let intent = format!("/DestOutputProfileRef << /URLs [(p.icc)] /CheckSum <{sum}> /ProfileCS (CMYK) >>");
        let resources = "<< /ColorSpace << /DefaultCMYK [/ICCBased 9 0 R] >> >>";
        let doc = x5n_doc(&intent, resources, "0 0 0 1 k 0 0 1 1 re f", &[(9, stream.clone())]);
        let settings = Settings {
            files: crate::checks::Files::Set(folder.clone()),
            ..Level::X4p.into()
        };
        let found = not_intent_profile_at_level(&doc, &settings).unwrap();
        assert_eq!(found[0].message, "a DefaultCMYK uses the output intent's profile");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
