//! Print checks on the job's colour mode (ported from the Python `colour` module, under the parity gate): the blend
//! space transparency needs, spot inks on the allowed list, artwork in the job's colour family (RGB artwork in a CMYK
//! job, CMYK in an RGB job), and embedded ICC profiles that are sound and agree with each other and the output intent.
//!
//! Each page's paints are read outside tiling pattern cells; patterns count through the families they paint in. The
//! families are the print checks' own (`common::family`).

use std::collections::{BTreeMap, BTreeSet};

use super::ink::describe;
use super::painted::page_of;
use super::{ColourMode, Settings};
use crate::common::family::{self, Family};
use crate::common::function;
use crate::common::icc;
use crate::common::pyfmt::{repr, repr_float};
use crate::document::{Document, Object, Value};
use crate::findings::{
    BLEND_SPACE, COLOUR_SPACE, Finding, ICC_PROFILE, Location, RGB_ONLY, SPOT_DEFINITION, SPOT_INK, Severity,
    X5_N_TRANSPARENCY_GROUP_CS,
};
use crate::graphics::{self, Event, Kind, Role};

/// A family's name in messages.
fn upper(family: Family) -> &'static str {
    match family {
        Family::Gray => "GRAY",
        Family::Rgb => "RGB",
        Family::Cmyk => "CMYK",
        Family::Other => "OTHER",
    }
}

pub(super) fn text(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

/// The family of the first output intent whose profile gives `/N`, by `/N` alone.
pub(super) fn intent_family(doc: &Document) -> Option<Family> {
    let intents = doc
        .catalog()?
        .get(b"OutputIntents")
        .and_then(|i| doc.resolve(i))?
        .array()?;
    let n = intents.iter().find_map(|intent| {
        let profile = doc
            .resolve(doc.resolve(intent)?.dict()?.get(b"DestOutputProfile")?)?
            .dict()?;
        profile.get(b"N")
    })?;
    #[expect(clippy::cast_possible_truncation, reason = "a component count, truncated")]
    let n = match doc.resolve(n)?.value {
        Value::Integer(n) => n,
        Value::Real(r) => r as i64,
        _ => return None,
    };
    match n {
        1 => Some(Family::Gray),
        3 => Some(Family::Rgb),
        4 => Some(Family::Cmyk),
        _ => None,
    }
}

/// One paint as the colour-mode checks read it: its kind, colour spaces and ink, and an image `XObject`'s number.
pub(super) type Paint = (Kind, Vec<graphics::Use>, graphics::Ink, Option<u32>);

/// What each page holds for the colour-mode checks: its paints outside pattern cells, its spot inks and its
/// transparency reasons.
#[derive(Default)]
pub(super) struct Page {
    pub(super) paints: Vec<Paint>,
    spots: BTreeSet<String>,
    transparency: BTreeSet<&'static str>,
}

pub(super) fn pages(doc: &Document) -> BTreeMap<u32, Page> {
    let mut out: BTreeMap<u32, Page> = BTreeMap::new();
    for event in graphics::walk(doc) {
        match event {
            Event::Paint {
                kind,
                uses,
                ink,
                frames,
                ..
            } if !ink.cell => {
                let object = match frames.last() {
                    Some(Location::Object(num, _)) if kind == Kind::Image => Some(*num),
                    _ => None,
                };
                out.entry(page_of(&frames))
                    .or_default()
                    .paints
                    .push((kind, uses, *ink, object));
            }
            Event::Spots { names, frames } => {
                let page = out.entry(page_of(&frames)).or_default();
                page.spots.extend(names.iter().map(|n| text(n)));
            }
            Event::Transparency { reason, frames } => {
                out.entry(page_of(&frames)).or_default().transparency.insert(reason);
            }
            _ => {}
        }
    }
    out
}

/// A paint's resource name as the messages give it: `/Im0`, or `(inline)`.
pub(super) fn shown(kind: Kind, name: &[u8]) -> String {
    if kind == Kind::InlineImage || name.is_empty() {
        text(name)
    } else {
        format!("/{}", text(name))
    }
}

/// On a page, what is painted in `family`: its images by name, its other colours described, and the spot inks its
/// other colours use.
fn painted_in(page: &Page, wanted: Family) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
    let (mut images, mut colours, mut spots) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for (kind, uses, ink, _) in &page.paints {
        let mask = uses.iter().any(|u| u.role == Role::ImageMask);
        match kind {
            Kind::Image | Kind::InlineImage if !mask => {
                if ink.space == Some(wanted) {
                    images.insert(shown(*kind, &ink.name));
                }
            }
            Kind::Shading => {
                if ink.space == Some(wanted) {
                    colours.insert(format!("shading {}", shown(*kind, &ink.name)));
                }
            }
            _ => {
                for colour in graphics::colours(*kind, uses, ink) {
                    if colour.family == wanted || colour.pattern.contains(&wanted) {
                        colours.insert(describe(colour));
                    } else {
                        spots.extend(colour.spot.iter().map(|s| text(s)));
                    }
                }
            }
        }
    }
    (images, colours, spots)
}

pub(super) fn on_page(check: &'static crate::findings::Check, page: u32, message: String) -> Finding {
    Finding::new(check, message).at(vec![Location::Page(page)])
}

pub(super) fn warning(finding: Finding) -> Finding {
    Finding {
        severity: Severity::Warning,
        ..finding
    }
}

/// Transparency blends in a declared space, and in the output intent's family.
pub(super) fn blend_space(doc: &Document, _: &Settings) -> Vec<Finding> {
    let expected = intent_family(doc);
    let found = pages(doc);
    let mut out = Vec::new();
    for (i, dict) in doc.pages().iter().enumerate() {
        let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let Some(page) = found.get(&number).filter(|p| !p.transparency.is_empty()) else {
            continue;
        };
        let reasons: Vec<&str> = page.transparency.iter().copied().collect();
        let reasons = reasons.join(", ");
        let group = dict
            .dict
            .get(b"Group")
            .and_then(|g| doc.resolve(g))
            .and_then(Object::dict);
        let Some(space) = group.and_then(|g| g.get(b"CS")) else {
            let message = format!(
                "the page uses transparency ({reasons}) but declares no blend colour space: the RIP chooses one"
            );
            out.push(warning(on_page(BLEND_SPACE, number, message)));
            continue;
        };
        let resources = dict
            .get(b"Resources")
            .and_then(|r| doc.resolve(r))
            .and_then(Object::dict);
        let family = family::classify(doc, Some(space), resources);
        if let Some(expected) = expected.filter(|e| *e != family) {
            let message = format!(
                "transparency ({reasons}) blends in {} but the output intent is {}: overlapping colours will shift",
                upper(family),
                upper(expected)
            );
            out.push(on_page(BLEND_SPACE, number, message));
        }
    }
    out
}

/// Every spot ink a page selects is on the profile's list; `None` is never reported, `All` only when not listed.
pub(super) fn spot_ink(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(allowed) = &settings.params.allow_spots else {
        return Vec::new();
    };
    pages(doc)
        .into_iter()
        .filter_map(|(number, page)| {
            let unexpected: Vec<&String> = page
                .spots
                .iter()
                .filter(|s| *s != "None" && !allowed.contains(s))
                .collect();
            if unexpected.is_empty() {
                return None;
            }
            let all = unexpected.iter().any(|s| *s == "All");
            let note = if all {
                "; All is the registration colour and prints on every plate"
            } else {
                ""
            };
            let names: Vec<&str> = unexpected.iter().map(|s| s.as_str()).collect();
            let message = format!("spot ink(s) not on the allowed list: {}{note}", names.join(", "));
            Some(on_page(SPOT_INK, number, message))
        })
        .collect()
}

/// In a CMYK job: images in RGB fail, and vector artwork in RGB is advisory unless the profile is strict.
pub(super) fn colour_space(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    if settings.params.colour_mode != ColourMode::Cmyk {
        return out;
    }
    for (number, page) in pages(doc) {
        let (images, colours, _) = painted_in(&page, Family::Rgb);
        for name in images {
            let message =
                format!("image {name} is in an RGB colour space -- it must be converted to CMYK for a FOGRA51 press");
            out.push(on_page(COLOUR_SPACE, number, message));
        }
        if !colours.is_empty() {
            let message = "vector artwork is painted in RGB -- confirm the RIP is meant to convert it".to_owned();
            let finding = on_page(COLOUR_SPACE, number, message);
            out.push(if settings.params.strict_cmyk {
                finding
            } else {
                warning(finding)
            });
        }
    }
    out
}

/// In an RGB job: no CMYK output intent, no CMYK images or artwork, and spots (which have no RGB value) advisory.
pub(super) fn rgb_only(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    if settings.params.colour_mode != ColourMode::Rgb {
        return out;
    }
    if intent_family(doc) == Some(Family::Cmyk) {
        out.push(Finding::new(
            RGB_ONLY,
            "the output intent is a CMYK profile in an RGB job",
        ));
    }
    for (number, page) in pages(doc) {
        let (images, colours, spots) = painted_in(&page, Family::Cmyk);
        for name in images {
            out.push(on_page(
                RGB_ONLY,
                number,
                format!("image {name} is in a CMYK colour space"),
            ));
        }
        if !colours.is_empty() {
            let first: Vec<&str> = colours.iter().take(3).map(String::as_str).collect();
            let message = format!("artwork is painted in CMYK ({}...)", first.join(", "));
            out.push(on_page(RGB_ONLY, number, message));
        }
        if !spots.is_empty() {
            let names: Vec<&str> = spots.iter().map(String::as_str).collect();
            let message = format!("spot colour(s) {} have no RGB value", names.join(", "));
            out.push(warning(on_page(RGB_ONLY, number, message)));
        }
    }
    out
}

/// Every object reachable from the catalog, `/Parent` aside, as the print checks walk it: depth first, a dictionary's
/// entries in reverse key order, an array's items last to first, an indirect object once, a direct one each time it
/// is reached.
fn reachable(doc: &Document) -> Vec<&Object> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let root = doc.trailer().get(b"Root");
    let mut stack: Vec<&Object> = root.into_iter().collect();
    while let Some(object) = stack.pop() {
        if let Value::Reference(num, _) = object.value
            && !seen.insert(num)
        {
            continue;
        }
        let Some(resolved) = doc.resolve(object) else { continue };
        match &resolved.value {
            Value::Dict(dict) => {
                let mut entries: Vec<&(Vec<u8>, Object)> =
                    dict.entries().iter().filter(|(k, _)| k != b"Parent").collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                stack.extend(entries.into_iter().map(|(_, v)| v));
            }
            Value::Array(items) => stack.extend(items.iter()),
            _ => {}
        }
        out.push(resolved);
    }
    out
}

/// A name's text without leading slashes, as the print checks have always shown ink names; `None` when it is not
/// UTF-8.
fn ink_name(doc: &Document, object: &Object) -> Option<String> {
    let name = doc.resolve(object)?.name()?;
    Some(std::str::from_utf8(name).ok()?.trim_start_matches('/').to_owned())
}

/// A spot colour space read as the print checks read it: its ink names, its alternate space's label, and the
/// alternate's components at full tint (rounded to four places); `None` for a space they skip.
fn spot(
    doc: &Document,
    separation: bool,
    [names, alternate, transform]: [&Object; 3],
) -> Option<(Inks, Option<Vec<f64>>)> {
    let names = if separation {
        vec![ink_name(doc, names)?]
    } else {
        doc.resolve(names)?
            .array()?
            .iter()
            .map(|n| ink_name(doc, n))
            .collect::<Option<_>>()?
    };
    let label = match family::classify(doc, Some(alternate), None) {
        Family::Gray => Some("DeviceGray".to_owned()),
        Family::Rgb => Some("DeviceRGB".to_owned()),
        Family::Cmyk => Some("DeviceCMYK".to_owned()),
        Family::Other => match &doc.resolve(alternate)?.value {
            Value::Name(_) => Some(ink_name(doc, alternate)?),
            Value::Array(parts) => match parts.first() {
                None => None,
                Some(first) => {
                    let head = ink_name(doc, first)?;
                    if head == "ICCBased" {
                        let profile = doc.resolve(parts.get(1)?)?.dict()?;
                        #[expect(clippy::cast_possible_truncation, reason = "a component count, truncated")]
                        let n = match profile.get(b"N").and_then(|n| doc.resolve(n)).map(|n| &n.value) {
                            None => 0,
                            Some(Value::Integer(n)) => *n,
                            Some(Value::Real(r)) => *r as i64,
                            Some(_) => return None,
                        };
                        Some(format!("ICCBased({n})"))
                    } else {
                        Some(head)
                    }
                }
            },
            _ => None,
        },
    };
    let values = function::evaluate(doc, transform, &vec![1.0; names.len()])
        .map(|v| v.into_iter().map(|x| rounded(x, 4)).collect());
    Some(((names, label), values))
}

/// `round(v, places)` as Python gives it: the nearest decimal, ties to even on the exact binary value.
fn rounded(v: f64, places: usize) -> f64 {
    format!("{v:.places$}").parse().unwrap_or(v)
}

/// A spot ink: its names and its alternate space's label.
type Inks = (Vec<String>, Option<String>);

/// What a spot ink looks like: its alternate space's label and its full-tint components to two places.
type Look = (Option<String>, Vec<f64>);

/// A spot ink (same names, same alternate space) defined with different full-tint colours.
pub(super) fn spot_definition(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut definitions: Vec<(Inks, Vec<(Look, String)>)> = Vec::new();
    for object in reachable(doc) {
        let Some([head, names, alternate, transform, ..]) = object.array() else {
            continue;
        };
        let separation = match doc.resolve(head).and_then(Object::name) {
            Some(b"Separation") => true,
            Some(b"DeviceN") => false,
            _ => continue,
        };
        let Some(((names, label), Some(alternate))) = spot(doc, separation, [names, alternate, transform]) else {
            continue;
        };
        if names.iter().any(|n| n == "All" || n == "None") {
            continue;
        }
        let look: Look = (label.clone(), alternate.iter().map(|v| rounded(*v, 2)).collect());
        let shown = format!(
            "[{}]",
            alternate.iter().map(|v| repr_float(*v)).collect::<Vec<_>>().join(", ")
        );
        let key = (names, label);
        match definitions.iter_mut().find(|(k, _)| *k == key) {
            Some((_, looks)) => {
                if !looks.iter().any(|(l, _)| *l == look) {
                    looks.push((look, shown));
                }
            }
            None => definitions.push((key, vec![(look, shown)])),
        }
    }
    let key_repr = |(names, label): &Inks| {
        let names: Vec<String> = names.iter().map(|n| repr(n)).collect();
        let comma = if names.len() == 1 { "," } else { "" };
        format!(
            "(({}{comma}), {})",
            names.join(", "),
            label.as_deref().map_or_else(|| "None".to_owned(), repr)
        )
    };
    definitions.sort_by_cached_key(|(key, _)| key_repr(key));
    definitions
        .into_iter()
        .filter(|(_, looks)| looks.len() > 1)
        .map(|((names, label), looks)| {
            let shown: Vec<&str> = looks.iter().map(|(_, s)| s.as_str()).collect();
            Finding::new(
                SPOT_DEFINITION,
                format!(
                    "spot ink {} is defined {} different ways in {}: {}",
                    names.join(" + "),
                    looks.len(),
                    label.as_deref().unwrap_or("None"),
                    shown.join("; ")
                ),
            )
        })
        .collect()
}

/// A profile's data colour space: its family and component count, and its name in messages.
fn icc_space(data: &[u8]) -> Option<(&'static str, Option<Family>, i64)> {
    match data.get(16..20)? {
        b"GRAY" => Some(("GRAY", Some(Family::Gray), 1)),
        b"RGB " => Some(("RGB", Some(Family::Rgb), 3)),
        b"CMYK" => Some(("CMYK", Some(Family::Cmyk), 4)),
        b"Lab " => Some(("Lab", None, 3)),
        _ => None,
    }
}

/// A profile's name in messages: its description, `(unnamed)` without one, `(unreadable)` when it is not a profile.
fn label(data: &[u8]) -> String {
    if icc::header(data).is_none() {
        return "(unreadable)".to_owned();
    }
    let description = icc::description(data).map_or_default(|d| d.trim().to_owned());
    if description.is_empty() {
        "(unnamed)".to_owned()
    } else {
        description
    }
}

/// What tells two profiles of a family apart: a CMYK profile's computed MD5, another's description and size.
fn key(data: &[u8], family: Family, label: &str) -> String {
    if family == Family::Cmyk {
        icc::computed_id(data)
            .iter()
            .flat_map(|b| [b >> 4, b & 15])
            .map(|n| char::from_digit(u32::from(n), 16).unwrap_or('0'))
            .collect()
    } else {
        format!("{label}:{}", data.len())
    }
}

/// The output intent's profile: its key, family and label, from the first intent whose profile names a colour space
/// the print checks know.
fn intent_profile(doc: &Document) -> Option<(String, Family, String)> {
    let intents = doc
        .catalog()?
        .get(b"OutputIntents")
        .and_then(|i| doc.resolve(i))?
        .array()?;
    intents.iter().find_map(|intent| {
        let reference = doc.resolve(intent)?.dict()?.get(b"DestOutputProfile")?;
        let data = match reference.value {
            Value::Reference(num, _) => doc.stream_data(num)?,
            _ => return None,
        };
        let (_, family, _) = icc_space(&data)?;
        let family = family.unwrap_or(Family::Other);
        let name = label(&data);
        Some((key(&data, family, &name), family, name))
    })
}

/// Embedded ICC profiles are sound and agree with each other and the job: one profile per family in an RGB or gray
/// job, and in a CMYK job not the output intent's own profile under an `ICCBased` space.
pub(super) fn icc_profile(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut seen: Vec<(String, Family, String)> = Vec::new();
    for object in reachable(doc) {
        let Some([head, stream, ..]) = object.array() else {
            continue;
        };
        if doc.resolve(head).and_then(Object::name) != Some(b"ICCBased") {
            continue;
        }
        let data = match stream.value {
            Value::Reference(num, _) => doc.stream_data(num),
            _ => None,
        };
        let declared =
            doc.resolve(stream)
                .and_then(Object::dict)
                .map(|d| match d.get(b"N").and_then(|n| doc.resolve(n)) {
                    None => Some(0),
                    #[expect(clippy::cast_possible_truncation, reason = "a component count, truncated")]
                    Some(n) => match n.value {
                        Value::Integer(n) => Some(n),
                        Value::Real(r) => Some(r as i64),
                        _ => None,
                    },
                });
        let (Some(data), Some(Some(components))) = (data, declared) else {
            out.push(Finding::new(
                ICC_PROFILE,
                "an ICCBased colour space has an unreadable profile",
            ));
            continue;
        };
        let name = label(&data);
        let problem = if data.len() < 128 || data.get(36..40) != Some(b"acsp") {
            Some("is not a valid ICC profile".to_owned())
        } else {
            match icc_space(&data) {
                Some((space, _, n)) if n != components => Some(format!(
                    "is a {space} profile but the colour space declares /N {components}"
                )),
                _ => None,
            }
        };
        if let Some(problem) = problem {
            out.push(Finding::new(ICC_PROFILE, format!("ICC profile {name}: {problem}")));
            continue;
        }
        if let Some((_, Some(family), _)) = icc_space(&data) {
            let key = key(&data, family, &name);
            if !seen.iter().any(|(k, _, _)| *k == key) {
                seen.push((key, family, name));
            }
        }
    }
    out.extend(consistency(&seen, intent_profile(doc), settings.params.colour_mode));
    out
}

fn consistency(
    seen: &[(String, Family, String)],
    intent: Option<(String, Family, String)>,
    mode: ColourMode,
) -> Option<Finding> {
    let family = match mode {
        ColourMode::Rgb => Family::Rgb,
        ColourMode::Gray => Family::Gray,
        ColourMode::Cmyk => {
            let (intent_key, intent_family, _) = intent?;
            if intent_family != Family::Cmyk {
                return None;
            }
            let (_, _, same) = seen.iter().find(|(k, f, _)| *f == Family::Cmyk && *k == intent_key)?;
            return Some(Finding::new(
                ICC_PROFILE,
                format!(
                    "a CMYK ICCBased colour space uses the output intent's own profile {} -- PDF/X-4 requires DeviceCMYK for colour already in the printing condition (ISO 15930-7, 6.4.3.3)",
                    repr(same)
                ),
            ));
        }
    };
    let mut same: BTreeMap<String, String> = seen
        .iter()
        .filter(|(_, f, _)| *f == family)
        .map(|(k, _, l)| (k.clone(), l.clone()))
        .collect();
    if let Some((key, _, name)) = intent.filter(|(_, f, _)| *f == family) {
        same.entry(key).or_insert(name);
    }
    if same.len() < 2 {
        return None;
    }
    let name = if family == Family::Rgb { "RGB" } else { "gray" };
    let mut labels: Vec<&str> = same.values().map(String::as_str).collect();
    labels.sort_unstable();
    Some(Finding::new(
        ICC_PROFILE,
        format!(
            "the {name} job mixes {} {name} profiles: {}",
            same.len(),
            labels.join(", ")
        ),
    ))
}

/// ISO 15930-8 7.3 (X5-Q12): under PDF/X-5n a page on which any print element uses transparency has a `/Group` with
/// `/CS`, since the intent's n-colorant space cannot be the blending space.
pub(super) fn x5n_group_cs(doc: &Document, _: &Settings) -> Vec<Finding> {
    let found = pages(doc);
    let mut out = Vec::new();
    for (i, dict) in doc.pages().iter().enumerate() {
        let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let Some(page) = found.get(&number).filter(|p| !p.transparency.is_empty()) else {
            continue;
        };
        let group = dict
            .dict
            .get(b"Group")
            .and_then(|g| doc.resolve(g))
            .and_then(Object::dict);
        let what = match group {
            None => "no /Group",
            Some(g) if g.get(b"CS").is_none() => "a /Group without /CS",
            Some(_) => continue,
        };
        let reasons: Vec<&str> = page.transparency.iter().copied().collect();
        let message = format!("the page uses transparency ({}) but has {what}", reasons.join(", "));
        out.push(on_page(X5_N_TRANSPARENCY_GROUP_CS, number, message).citing("7.3"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::Params;
    use crate::checks::test_doc::paged;
    use crate::findings::Level;

    /// An ICC profile stream of `space` (four characters), its description `name`, with `/N` given as `n`.
    fn profile(space: [u8; 4], name: &str, n: &str) -> Vec<u8> {
        let mut data = vec![0u8; 128];
        data.splice(16..20, space);
        data.splice(36..40, *b"acsp");
        data.extend(1u32.to_be_bytes());
        data.extend(*b"desc");
        data.extend(144u32.to_be_bytes());
        let mut tag = b"desc\0\0\0\0".to_vec();
        tag.extend(u32::try_from(name.len() + 1).unwrap().to_be_bytes());
        tag.extend(name.as_bytes());
        tag.push(0);
        data.extend(u32::try_from(tag.len()).unwrap().to_be_bytes());
        data.extend(tag);
        let mut stream = format!("<< {n} /Length {} >>\nstream\n", data.len()).into_bytes();
        stream.extend(data);
        stream.extend(b"\nendstream");
        stream
    }

    fn run(
        doc: &Document,
        params: Params,
        check: fn(&Document, &Settings) -> Vec<Finding>,
    ) -> Vec<(Severity, Option<u32>, String)> {
        check(
            doc,
            &Settings {
                level: Level::Print,
                params,
                files: crate::checks::Files::Alone,
            },
        )
        .into_iter()
        .map(|f| (f.severity, f.page, f.message))
        .collect()
    }

    fn intent(n: &str) -> String {
        format!("/OutputIntents [5 << /DestOutputProfile 9 0 R >> << /DestOutputProfile << {n} >> >>]")
    }

    #[test]
    fn intent_families() {
        let family = |n: &str| {
            intent_family(&paged(
                &[],
                &intent(n),
                &[(9, b"<< /Length 0 >>\nstream\n\nendstream".to_vec())],
            ))
        };
        assert_eq!(
            [
                family("/N 4"),
                family("/N 3.0"),
                family("/N 1"),
                family("/N 2"),
                family("/N /X")
            ],
            [Some(Family::Cmyk), Some(Family::Rgb), Some(Family::Gray), None, None]
        );
        assert_eq!(intent_family(&paged(&[], "", &[])), None);
    }

    #[test]
    fn blend_spaces() {
        let resources = "/Resources << /ExtGState << /A << /ca 0.5 >> /B << /BM [/Normal /Multiply] >> /C << /SMask /None /CA /X >> /D << /BM /Compatible /SMask << >> >> >> >>";
        let doc = paged(
            &[
                (resources, "/A gs"),
                (&format!("{resources} /Group << /CS /DeviceRGB >>"), "/B gs /C gs"),
                (&format!("{resources} /Group << /CS /DeviceCMYK >>"), "/D gs"),
                (resources, ""),
            ],
            &intent("/N 4"),
            &[],
        );
        assert_eq!(
            run(&doc, Params::default(), blend_space),
            [
                (Severity::Warning, Some(1), "the page uses transparency (opacity) but declares no blend colour space: the RIP chooses one".to_owned()),
                (Severity::Error, Some(2), "transparency (blend mode, opacity) blends in RGB but the output intent is CMYK: overlapping colours will shift".to_owned()),
            ]
        );
        let unintended = paged(
            &[(&format!("{resources} /Group << /CS /DeviceRGB >>"), "/A gs")],
            "",
            &[],
        );
        assert_eq!(run(&unintended, Params::default(), blend_space), []);
    }

    #[test]
    fn spot_inks() {
        let resources = "/Resources << /ColorSpace << /G [/Separation /Gold /DeviceCMYK 7 0 R] /N [/DeviceN [/None /All /Silver] /DeviceCMYK 7 0 R] >> >>";
        let doc = paged(
            &[(resources, "/G cs /N CS"), (resources, "/G cs")],
            "",
            &[(7, b"<< >>".to_vec())],
        );
        let allowed = |names: &[&str]| Params {
            allow_spots: Some(names.iter().map(|s| (*s).to_owned()).collect()),
            ..Params::default()
        };
        assert_eq!(
            run(&doc, allowed(&["Silver"]), spot_ink),
            [
                (Severity::Error, Some(1), "spot ink(s) not on the allowed list: All, Gold; All is the registration colour and prints on every plate".to_owned()),
                (Severity::Error, Some(2), "spot ink(s) not on the allowed list: Gold".to_owned()),
            ]
        );
        assert_eq!(run(&doc, allowed(&["All", "Gold", "Silver"]), spot_ink), []);
        assert_eq!(run(&doc, Params::default(), spot_ink), [], "no list, no check");
    }

    #[test]
    fn colour_families_by_job() {
        let resources = "/Resources << /XObject << /I 8 0 R /M 9 0 R >> /Shading << /S << /ColorSpace /DeviceRGB >> /K << /ColorSpace /DeviceCMYK >> >> \
            /ColorSpace << /G [/Separation /Gold /DeviceCMYK 7 0 R] >> /Pattern << /P << /PatternType 2 /Shading << /ColorSpace /DeviceCMYK >> >> >> >>";
        let rgb = "/I Do /M Do BI /W 1 /H 1 /BPC 8 /CS /RGB ID abc EI 1 0 0 rg 0 0 1 1 re f /S sh";
        let cmyk = "0 0 0 1 k 0 0 1 1 re f 0.5 0 0 0 K 0 0 1 1 re S 0 0 1 0 k 0 0 1 1 re f /K sh /Pattern cs /P scn 0 0 1 1 re f \
            /G cs 0 0 1 1 re f BI /W 1 /H 1 /BPC 8 /CS /CMYK ID abcd EI";
        let more = [
            (7, b"<< >>".to_vec()),
            (
                8,
                b"<< /Subtype /Image /ColorSpace /DeviceRGB /Length 0 >>\nstream\n\nendstream".to_vec(),
            ),
            (
                9,
                b"<< /Subtype /Image /ImageMask true /Length 0 >>\nstream\n\nendstream".to_vec(),
            ),
        ];
        let doc = paged(&[(resources, rgb), (resources, cmyk)], &intent("/N 4"), &more);
        let strict = Params {
            strict_cmyk: true,
            ..Params::default()
        };
        assert_eq!(
            run(&doc, strict, colour_space),
            [
                (
                    Severity::Error,
                    Some(1),
                    "image (inline) is in an RGB colour space -- it must be converted to CMYK for a FOGRA51 press"
                        .to_owned()
                ),
                (
                    Severity::Error,
                    Some(1),
                    "image /I is in an RGB colour space -- it must be converted to CMYK for a FOGRA51 press".to_owned()
                ),
                (
                    Severity::Error,
                    Some(1),
                    "vector artwork is painted in RGB -- confirm the RIP is meant to convert it".to_owned()
                ),
            ]
        );
        assert_eq!(
            run(&doc, Params::default(), colour_space).last().map(|f| f.0),
            Some(Severity::Warning)
        );
        let rgb_job = Params {
            colour_mode: ColourMode::Rgb,
            ..Params::default()
        };
        assert_eq!(run(&doc, rgb_job.clone(), colour_space), [], "an RGB job");
        assert_eq!(
            run(&doc, rgb_job, rgb_only),
            [
                (
                    Severity::Error,
                    None,
                    "the output intent is a CMYK profile in an RGB job".to_owned()
                ),
                (
                    Severity::Error,
                    Some(2),
                    "image (inline) is in a CMYK colour space".to_owned()
                ),
                (
                    Severity::Error,
                    Some(2),
                    "artwork is painted in CMYK (C0 M0 Y0 K100, C0 M0 Y100 K0, C50 M0 Y0 K0...)".to_owned()
                ),
                (
                    Severity::Warning,
                    Some(2),
                    "spot colour(s) Gold have no RGB value".to_owned()
                ),
            ]
        );
        assert_eq!(run(&doc, Params::default(), rgb_only), [], "a CMYK job");
        assert_eq!(shown(Kind::Shading, b""), "", "a shading with no name");
    }

    #[test]
    fn icc_profiles() {
        let resources = "/Resources << /ColorSpace << /A [/ICCBased 10 0 R] /B [/ICCBased 11 0 R] /C [/ICCBased 12 0 R] \
            /D [/ICCBased 13 0 R] /E [/ICCBased 14 0 R] /F [/ICCBased << /N 3 >>] /G [/ICCBased 15 0 R] /H [/ICCBased 16 0 R] \
            /I [/ICCBased 17 0 R] /J [/ICCBased 18 0 R] /K [/ICCBased] /L [/DeviceN] >> >>";
        let more = [
            (10, profile(*b"RGB ", "sRGB", "/N 3")),
            (11, profile(*b"RGB ", "Adobe RGB", "/N 3")),
            (12, profile(*b"CMYK", "Coated", "/N 3")),
            (13, b"<< /N 3 /Length 3 >>\nstream\nabc\nendstream".to_vec()),
            (14, profile(*b"XYZ ", "Odd", "/N 3")),
            (15, profile(*b"Lab ", "", "/N 3.0")),
            (16, profile(*b"RGB ", "sRGB", "/N /X")),
            (17, profile(*b"RGB ", "sRGB", "")),
            (18, profile(*b"CMYK", "Coated", "/N 4")),
            (20, profile(*b"CMYK", "Coated", "/N 4")),
            (21, profile(*b"RGB ", "Display", "/N 3")),
        ];
        let messages = |intents: &str, mode: ColourMode| {
            let doc = paged(&[(resources, "")], intents, &more);
            run(
                &doc,
                Params {
                    colour_mode: mode,
                    ..Params::default()
                },
                icc_profile,
            )
            .into_iter()
            .map(|(_, _, m)| m)
            .collect::<BTreeSet<_>>()
        };
        let fixed: BTreeSet<String> = [
            "an ICCBased colour space has an unreadable profile",
            "ICC profile Coated: is a CMYK profile but the colour space declares /N 3",
            "ICC profile (unreadable): is not a valid ICC profile",
            "ICC profile sRGB: is a RGB profile but the colour space declares /N 0",
        ]
        .map(str::to_owned)
        .into();
        let cmyk = "/OutputIntents [<< /DestOutputProfile 20 0 R >>]";
        let mut want = fixed.clone();
        want.insert("a CMYK ICCBased colour space uses the output intent's own profile 'Coated' -- PDF/X-4 requires DeviceCMYK for colour already in the printing condition (ISO 15930-7, 6.4.3.3)".to_owned());
        assert_eq!(messages(cmyk, ColourMode::Cmyk), want);
        let mut want = fixed.clone();
        want.insert("the RGB job mixes 3 RGB profiles: Adobe RGB, Display, sRGB".to_owned());
        assert_eq!(
            messages("/OutputIntents [<< /DestOutputProfile 21 0 R >>]", ColourMode::Rgb),
            want
        );
        assert_eq!(messages("", ColourMode::Gray), fixed, "one gray profile at most");
        assert_eq!(
            messages("/OutputIntents [<< /DestOutputProfile 21 0 R >>]", ColourMode::Cmyk),
            fixed,
            "an RGB intent"
        );
        assert_eq!(
            messages(
                "/OutputIntents [<< /DestOutputProfile << >> >> << /DestOutputProfile 14 0 R >>]",
                ColourMode::Cmyk
            ),
            fixed
        );
        assert_eq!(label(b"not a profile"), "(unreadable)");
    }

    #[test]
    fn intents_and_profiles_read_or_not() {
        let more = [
            (9, b"<< /Length 0 >>\nstream\n\nendstream".to_vec()),
            (
                10,
                b"<< /N 1 /Filter /FlateDecode /Length 3 >>\nstream\nabc\nendstream".to_vec(),
            ),
            (11, b"<< /Length 3 >>\nstream\nabc\nendstream".to_vec()),
            (12, profile(*b"GRAY", "Dot Gain 15%", "/N 1")),
        ];
        let doc = |catalog: &str| paged(&[("", "")], catalog, &more);
        for catalog in [
            "",
            "/OutputIntents 5",
            "/OutputIntents [5 99 0 R << >> << /DestOutputProfile 99 0 R >> << /DestOutputProfile 9 0 R >>]",
        ] {
            assert_eq!(intent_family(&doc(catalog)), None, "{catalog}");
            assert_eq!(intent_profile(&doc(catalog)), None, "{catalog}");
        }
        let odd = "/OutputIntents [<< /DestOutputProfile << /N 1 >> >> << /DestOutputProfile 10 0 R >> << /DestOutputProfile 11 0 R >> << /DestOutputProfile 12 0 R >>]";
        assert_eq!(intent_family(&doc(odd)), Some(Family::Gray));
        assert_eq!(
            intent_profile(&doc(odd)).map(|(_, f, l)| (f, l)),
            Some((Family::Gray, "Dot Gain 15%".to_owned()))
        );
        assert_eq!(
            intent_family(&doc("/OutputIntents [<< /DestOutputProfile << /N 99 0 R >> >>]")),
            None
        );
        assert_eq!(intent_family(&doc("/OutputIntents [<< /DestOutputProfile 5 >>]")), None);
        let rootless = Document::open(
            &crate::document::test_pdf::classic(&[(1, b"5")], "<< /Size SIZE /Root 1 0 R >>"),
            &crate::pipeline::Limits::default(),
        )
        .unwrap();
        assert_eq!(
            (intent_family(&rootless), intent_profile(&rootless)),
            (None, None),
            "no catalog"
        );
    }

    #[test]
    fn more_jobs() {
        let resources = "/Resources << /ExtGState << /A << /ca 0.5 >> >> /ColorSpace << /P [/ICCBased 12 0 R] /Q [/ICCBased 13 0 R] \
            /R [/ICCBased 12 0 R] /C [/ICCBased 14 0 R] /D 99 0 R >> /Pattern << /T 15 0 R >> >>";
        let more = [
            (12, profile(*b"GRAY", "Dot Gain 15%", "/N 1")),
            (13, profile(*b"GRAY", "Dot Gain 20%", "/N 1")),
            (14, profile(*b"CMYK", "Coated", "/N 4")),
            (
                15,
                b"<< /PatternType 1 /PaintType 1 /Resources << >> /Length 12 >>\nstream\n0 0 1 1 re f\nendstream"
                    .to_vec(),
            ),
            (20, profile(*b"CMYK", "Uncoated", "/N 4")),
        ];
        let doc = paged(
            &[
                (
                    &format!("{resources} /Group << /CS /DeviceGray >>"),
                    "/A gs /Pattern cs /T scn 0 0 1 1 re f /Nope Do",
                ),
                (&format!("{resources} /Group << /CS /Lab >>"), "/A gs"),
            ],
            "/OutputIntents [<< /DestOutputProfile 20 0 R >>]",
            &more,
        );
        let gray = Params {
            colour_mode: ColourMode::Gray,
            ..Params::default()
        };
        assert_eq!(
            run(&doc, gray, icc_profile)
                .into_iter()
                .map(|f| f.2)
                .collect::<Vec<_>>(),
            ["the gray job mixes 2 gray profiles: Dot Gain 15%, Dot Gain 20%"]
        );
        assert_eq!(
            run(&doc, Params::default(), icc_profile),
            [],
            "the CMYK intent's own profile is not used"
        );
        assert_eq!(
            run(&doc, Params::default(), blend_space)
                .into_iter()
                .map(|f| f.2)
                .collect::<Vec<_>>(),
            [
                "transparency (opacity) blends in GRAY but the output intent is CMYK: overlapping colours will shift",
                "transparency (opacity) blends in OTHER but the output intent is CMYK: overlapping colours will shift"
            ]
        );
        let rgb = Params {
            colour_mode: ColourMode::Rgb,
            ..Params::default()
        };
        assert_eq!(
            run(&paged(&[("", "")], "", &[]), rgb, rgb_only),
            [],
            "no CMYK intent, nothing painted"
        );
    }

    #[test]
    fn spot_definitions() {
        let tint = |c1: &str| format!("<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [{c1}] /N 1 >>");
        let (a, b) = (tint("0 0.3 1 0"), tint("0 0.6 1 0"));
        let gray = "<< /FunctionType 2 /Domain [0 1] /C1 [0.5] /N 1 >>";
        let spaces = [
            format!("[/Separation /Foil /DeviceCMYK {a}]"),
            format!("[/Separation /Foil /DeviceCMYK {a}]"),
            format!("[/Separation /Foil /DeviceCMYK {b}]"),
            format!("[/Separation /Foil [/ICCBased 7 0 R] {b}]"),
            format!("[/Separation /Gold /Lab {gray}]"),
            "[/Separation /Gold /Lab << /FunctionType 2 /Domain [0 1] /C1 [0.25] /N 1 >>]".to_string(),
            format!("[/Separation /Ink [/ICCBased 8 0 R] {gray}]"),
            "[/Separation /Ink [/ICCBased 8 0 R] << /FunctionType 2 /Domain [0 1] /C1 [0.75] /N 1 >>]".to_string(),
            format!("[/Separation /Odd [/Foo] {gray}] [/Separation /Odd [/Foo] {a}]"),
            format!("[/Separation /Bare 5 {gray}] [/Separation /Bare 5 {a}]"),
            format!("[/DeviceN [/A /B] /DeviceRGB {gray}] [/DeviceN [/A /B] /DeviceRGB {a}]"),
            format!("[/Separation /All /DeviceCMYK {a}] [/Separation /All /DeviceCMYK {b}] [/Separation /None /DeviceGray {gray}]"),
            "[/Separation /Broken /DeviceCMYK << /FunctionType 9 >>] [/Separation 5 /DeviceCMYK 6] [/DeviceN 5 /DeviceCMYK 6] [/Separation]".to_owned(),
            format!("[/Separation /Bad [/ICCBased 5] {gray}] [/Separation /Ugly [/ICCBased << /N /X >>] {gray}] [/Indexed /DeviceRGB 1 <00>]"),
            format!("[/Separation /Q [/ICCBased << >>] {gray}] [/Separation /Q [/ICCBased << >>] {a}] [/Separation /R [/ICCBased << /N 3.0 >>] {gray}]"),
            format!("[/DeviceN 99 0 R /DeviceCMYK {gray}] [/Separation 99 0 R /DeviceCMYK {gray}] [/Separation /#FF /DeviceCMYK {gray}]"),
            format!("[/Separation /Lb /#FE {gray}] [/Separation /Lc [/ICCBased] {gray}] [/DeviceN [/#FF] /DeviceCMYK {gray}]"),
            format!("[/Separation /Ld 99 0 R {gray}] [/Separation /Le [5] {gray}] [/Separation /Lf [/ICCBased 99 0 R] {gray}]"),
            format!("[/Separation /Two [/ICCBased << /N 2.0 >>] {gray}] [/Separation /Two [/ICCBased << /N 2.0 >>] {a}]"),
            format!("[/Separation /Empty [] {gray}] [/Separation /Empty [] {a}]"),
            "[/DeviceN [/A /B] /DeviceGray 9 0 R] [/DeviceN [/A /B] /DeviceGray 10 0 R]".to_owned(),
        ];
        let more = [
            (7, b"<< /N 4 /Length 3 >>\nstream\nabc\nendstream".to_vec()),
            (8, profile(*b"Lab ", "Lab", "/N 3")),
            (
                9,
                b"<< /FunctionType 4 /Domain [0 1 0 1] /Range [0 1] /Length 7 >>\nstream\n{ pop }\nendstream".to_vec(),
            ),
            (
                10,
                b"<< /FunctionType 4 /Domain [0 1 0 1] /Range [0 1] /Length 15 >>\nstream\n{ pop pop 0.5 }\nendstream"
                    .to_vec(),
            ),
        ];
        let doc = paged(&[("", "")], &format!("/S [{}]", spaces.join(" ")), &more);
        let found: Vec<String> = run(&doc, Params::default(), spot_definition)
            .into_iter()
            .map(|f| f.2)
            .collect();
        assert_eq!(
            found,
            [
                "spot ink A + B is defined 2 different ways in DeviceGray: [0.5]; [1.0]",
                "spot ink Bare is defined 2 different ways in None: [0.0, 0.3, 1.0, 0.0]; [0.5]",
                "spot ink Empty is defined 2 different ways in None: [0.0, 0.3, 1.0, 0.0]; [0.5]",
                "spot ink Foil is defined 2 different ways in DeviceCMYK: [0.0, 0.6, 1.0, 0.0]; [0.0, 0.3, 1.0, 0.0]",
                "spot ink Gold is defined 2 different ways in Lab: [0.25]; [0.5]",
                "spot ink Ink is defined 2 different ways in ICCBased(3): [0.75]; [0.5]",
                "spot ink Odd is defined 2 different ways in Foo: [0.0, 0.3, 1.0, 0.0]; [0.5]",
                "spot ink Q is defined 2 different ways in ICCBased(0): [0.0, 0.3, 1.0, 0.0]; [0.5]",
                "spot ink Two is defined 2 different ways in ICCBased(2): [0.0, 0.3, 1.0, 0.0]; [0.5]",
            ]
        );
    }

    #[test]
    fn pdf_x_5n_transparent_pages_name_a_blending_space() {
        let resources = "/Resources << /ExtGState << /A << /ca 0.5 >> >> >>";
        let doc = paged(
            &[
                (resources, "/A gs"),
                (&format!("{resources} /Group << /S /Transparency >>"), "/A gs"),
                (&format!("{resources} /Group << /CS /DeviceCMYK >>"), "/A gs"),
                (resources, ""),
            ],
            "",
            &[],
        );
        let found: Vec<String> = x5n_group_cs(&doc, &Level::X5n.into())
            .into_iter()
            .map(|f| f.message)
            .collect();
        assert_eq!(
            found,
            [
                "the page uses transparency (opacity) but has no /Group",
                "the page uses transparency (opacity) but has a /Group without /CS"
            ]
        );
    }
}
