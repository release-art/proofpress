//! PDF/X-4 (ISO 15930-7) 6.4.3.4, spot colours, and 6.20's blend modes.
//!
//! The `PrintingOrder` and `Solidities` rows read every object, as the clause states them for the whole file. The rest
//! read the spaces that print elements use (X4-Q3), the same-name rule for Separations included.

use super::Settings;
use std::collections::BTreeMap;

use super::intent;
use super::walk::dicts;
use crate::common::family::{self, Family};
use crate::common::function;
use crate::common::lab::{self, Lab, Model};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{
    Finding, Location, X4_SPOT_COLORANTS_ENTRY, X4_SPOT_COLORANTS_MATCH_DEVICEN, X4_SPOT_CONFUSING_NAMES,
    X4_SPOT_CONSISTENT_DEFINITION, X4_SPOT_NO_DOTGAIN, X4_SPOT_PRINTING_ORDER_CONSISTENT, X4_SPOT_PROCESS_COLORSPACE,
    X4_SPOT_SOLIDITIES_CONSISTENT, X4_TRANSPARENCY_BLEND_MODES, X5_N_DEVICEN_NO_PROCESS, X5_N_DEVICEN_SUBTYPE,
};
use crate::graphics::{self, Event};

/// Names in a `DeviceN` that are not spots (X4-Q20): the process inks and `None`. `All` counts as a spot.
const NOT_SPOTS: [&[u8]; 5] = [b"Cyan", b"Magenta", b"Yellow", b"Black", b"None"];

/// Spot names 6.4.3.4 discourages: Red, Green, Blue, Gray, and the process inks and gray in the languages print
/// workflows most often meet (German, French, Spanish, Italian, Dutch). The clause says "any translated names", which
/// no list can hold; these are the ones a checker can name.
const CONFUSING: [&str; 26] = [
    "Red", "Green", "Blue", "Gray", "Grey", "Grau", "Gris", "Grigio", "Grijs", "Cian", "Ciano", "Zyan", "Gelb",
    "Jaune", "Amarillo", "Giallo", "Geel", "Schwarz", "Noir", "Negro", "Nero", "Zwart", "Rouge", "Rot", "Rojo",
    "Rosso",
];

/// The blend modes of the PDF 1.6 Reference, 7.2.4, Tables 7.2 and 7.3.
const BLEND_MODES: [&[u8]; 17] = [
    b"Normal",
    b"Compatible",
    b"Multiply",
    b"Screen",
    b"Overlay",
    b"Darken",
    b"Lighten",
    b"ColorDodge",
    b"ColorBurn",
    b"HardLight",
    b"SoftLight",
    b"Difference",
    b"Exclusion",
    b"Hue",
    b"Saturation",
    b"Color",
    b"Luminosity",
];

fn text(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

/// Every `DeviceN` space print elements use, resolved, with where it was painted.
fn used_device_n(doc: &Document) -> Vec<(Vec<Object>, Vec<Location>)> {
    let mut out = Vec::new();
    for event in graphics::walk(doc) {
        let Event::Paint { uses, frames, .. } = event else {
            continue;
        };
        for used in uses {
            let mut stack = vec![used.space];
            // Bounded: a space may name itself as its own base.
            let mut steps = 0;
            while let Some(space) = stack.pop().filter(|_| steps < 64) {
                steps += 1;
                let Some(items) = doc.resolve(&space).and_then(Object::array).map(<[Object]>::to_vec) else {
                    continue;
                };
                match items.first().and_then(Object::name) {
                    Some(b"DeviceN") => out.push((items, frames.clone())),
                    Some(b"Indexed" | b"Pattern") => stack.extend(items.get(1).cloned()),
                    _ => {}
                }
            }
        }
    }
    out
}

/// A `DeviceN` array's names and attributes dictionary.
fn names_and_attributes<'a>(doc: &'a Document, items: &'a [Object]) -> (Vec<&'a [u8]>, Option<&'a Dict>) {
    let names = items
        .get(1)
        .and_then(|n| doc.resolve(n))
        .and_then(Object::array)
        .map_or_default(|n| n.iter().filter_map(Object::name).collect());
    (names, items.get(4).and_then(|a| doc.resolve(a)).and_then(Object::dict))
}

fn entry<'a>(doc: &'a Document, dict: Option<&'a Dict>, key: &[u8]) -> Option<&'a Object> {
    doc.resolve(dict?.get(key)?)
}

/// 6.4.3.4: every spot in a `DeviceN` has an entry in its attributes' `/Colorants`.
pub(super) fn colorants_entry(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for (items, frames) in used_device_n(doc) {
        let (names, attributes) = names_and_attributes(doc, &items);
        let colorants = entry(doc, attributes, b"Colorants").and_then(Object::dict);
        for name in names.into_iter().filter(|n| !NOT_SPOTS.contains(n)) {
            if colorants.and_then(|c| c.get(name)).is_none() {
                let message = format!("the spot {} of a DeviceN space has no /Colorants entry", text(name));
                out.push(
                    Finding::new(X4_SPOT_COLORANTS_ENTRY, message)
                        .citing("6.4.3.4")
                        .at(frames.clone()),
                );
            }
        }
    }
    dedup(out)
}

/// 6.4.3.4 (X4-Q5): a `DeviceN`'s `/Process` colour space is a device space of the intent's family, or an `ICCBased`
/// space with as many components.
pub(super) fn process_space(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(intent) = intent::space(doc, settings) else {
        return Vec::new();
    };
    let (device, channels): (&[u8], usize) = match &intent {
        b"GRAY" => (b"DeviceGray", 1),
        b"RGB " => (b"DeviceRGB", 3),
        b"CMYK" => (b"DeviceCMYK", 4),
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for (items, frames) in used_device_n(doc) {
        let (_, attributes) = names_and_attributes(doc, &items);
        let Some(space) = entry(
            doc,
            entry(doc, attributes, b"Process").and_then(Object::dict),
            b"ColorSpace",
        ) else {
            continue;
        };
        let family = space
            .array()
            .and_then(|a| a.first())
            .unwrap_or(space)
            .name()
            .unwrap_or_default();
        let fits = family == device || (family == b"ICCBased" && graphics::components(doc, space) == Some(channels));
        if !fits {
            let message = format!(
                "a DeviceN's /Process colour space is {}, not {}",
                text(family),
                text(device)
            );
            out.push(
                Finding::new(X4_SPOT_PROCESS_COLORSPACE, message)
                    .citing("6.4.3.4")
                    .at(frames),
            );
        }
    }
    dedup(out)
}

/// 6.4.3.4: an `NChannel` space's `/MixingHints` has no `/DotGain`.
pub(super) fn no_dot_gain(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for (items, frames) in used_device_n(doc) {
        let (_, attributes) = names_and_attributes(doc, &items);
        let hints = entry(doc, attributes, b"MixingHints").and_then(Object::dict);
        if hints.and_then(|h| h.get(b"DotGain")).is_some() {
            let message = "an NChannel space's /MixingHints has /DotGain".to_owned();
            out.push(Finding::new(X4_SPOT_NO_DOTGAIN, message).citing("6.4.3.4").at(frames));
        }
    }
    dedup(out)
}

/// 6.4.3.4 discourages spot names that read as process or primary colours.
pub(super) fn confusing_names(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for (items, frames) in used_device_n(doc) {
        let (names, _) = names_and_attributes(doc, &items);
        for name in names {
            let name = text(name);
            if CONFUSING.iter().any(|c| c.eq_ignore_ascii_case(&name)) {
                let message = format!("the spot name {name} may be taken for a process or primary colour");
                out.push(
                    Finding::new(X4_SPOT_CONFUSING_NAMES, message)
                        .citing("6.4.3.4")
                        .at(frames.clone()),
                );
            }
        }
    }
    for (name, _, _, frames) in separations(doc) {
        let name = text(&name);
        if CONFUSING.iter().any(|c| c.eq_ignore_ascii_case(&name)) {
            let message = format!("the spot name {name} may be taken for a process or primary colour");
            out.push(
                Finding::new(X4_SPOT_CONFUSING_NAMES, message)
                    .citing("6.4.3.4")
                    .at(frames),
            );
        }
    }
    dedup(out)
}

/// Every `Separation` array print elements use (X4-Q3 governs 6.4.3.4 too): reached from
/// the spaces of paints through `Indexed` and `Pattern` bases and `DeviceN` `/Colorants`, with its name, alternate,
/// tint transform and where it was painted.
fn separations(doc: &Document) -> Vec<(Vec<u8>, Object, Object, Vec<Location>)> {
    let mut out = Vec::new();
    for event in graphics::walk(doc) {
        let Event::Paint { uses, frames, .. } = event else {
            continue;
        };
        let mut stack: Vec<Object> = uses.into_iter().map(|u| u.space).collect();
        // Bounded: each step goes one level into a colour space, and real ones nest two or three levels.
        let mut steps = 0;
        while let Some(space) = stack.pop().filter(|_| steps < 64) {
            steps += 1;
            let Some(items) = doc.resolve(&space).and_then(Object::array) else {
                continue;
            };
            match items {
                [first, name, alternate, tint, ..] if first.name() == Some(b"Separation") => {
                    if let Some(name) = name.name() {
                        out.push((name.to_vec(), alternate.clone(), tint.clone(), frames.clone()));
                    }
                }
                [first, base, ..] if matches!(first.name(), Some(b"Indexed" | b"I" | b"Pattern")) => {
                    stack.push(base.clone());
                }
                [first, ..] if first.name() == Some(b"DeviceN") => {
                    let colorants = items
                        .get(4)
                        .and_then(|a| doc.resolve(a))
                        .and_then(Object::dict)
                        .and_then(|a| a.get(b"Colorants"))
                        .and_then(|c| doc.resolve(c))
                        .and_then(Object::dict);
                    stack.extend(
                        colorants.map_or_default(|c| c.entries().iter().map(|(_, v)| v.clone()).collect::<Vec<_>>()),
                    );
                }
                _ => {}
            }
        }
    }
    out
}

/// A number's value, integer or real.
#[expect(
    clippy::cast_precision_loss,
    reason = "PDF 1.6 caps integers at 2^31, well inside f64's exact range"
)]
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(i) => Some(*i as f64),
        Value::Real(r) => Some(*r),
        _ => None,
    }
}

/// Whether two numbers are the same value: 6.4.3.4 asks for the same value, not a close one.
#[expect(
    clippy::float_cmp,
    reason = "the clause compares values written in the file, exactly"
)]
fn equal(left: f64, right: f64) -> bool {
    left == right
}

/// Whether two objects are the same, comparing what they hold: references followed, streams by their decoded data
/// and dictionaries without `/Length`, `/Filter` and `/DecodeParms` (6.4.3.4: compression and indirection ignored).
fn same(doc: &Document, left: &Object, right: &Object, depth: usize) -> bool {
    let stream = |object: &Object| match object.value {
        Value::Reference(num, _) => doc.stream_data(num),
        _ => None,
    };
    let (Some(lhs), Some(rhs)) = (doc.resolve(left), doc.resolve(right)) else {
        return false;
    };
    if depth > 16 || stream(left) != stream(right) {
        return false;
    }
    match (&lhs.value, &rhs.value) {
        (Value::Array(lefts), Value::Array(rights)) => {
            lefts.len() == rights.len() && lefts.iter().zip(rights).all(|(l, r)| same(doc, l, r, depth + 1))
        }
        (Value::Dict(lefts), Value::Dict(rights)) => {
            let skip = |key: &[u8]| matches!(key, b"Length" | b"Filter" | b"DecodeParms");
            let keys = |dict: &Dict| -> Vec<Vec<u8>> {
                let mut keys: Vec<Vec<u8>> = dict
                    .entries()
                    .iter()
                    .map(|(k, _)| k.clone())
                    .filter(|k| !skip(k))
                    .collect();
                keys.sort();
                keys
            };
            keys(lefts) == keys(rights)
                && lefts
                    .entries()
                    .iter()
                    .filter(|(key, _)| !skip(key))
                    .all(|(key, value)| rights.get(key).is_some_and(|other| same(doc, value, other, depth + 1)))
        }
        (lefts, rights) => match (number(lefts), number(rights)) {
            (Some(l), Some(r)) => equal(l, r),
            _ => lefts == rights,
        },
    }
}

/// 6.4.3.4: every `Separation` array of one name that print elements use has the same alternate and tint transform,
/// whatever the name, process inks included.
pub(super) fn consistent_definitions(doc: &Document, _: &Settings) -> Vec<Finding> {
    let used = separations(doc);
    let mut first: BTreeMap<&[u8], (&Object, &Object)> = BTreeMap::new();
    let mut out = Vec::new();
    for (name, alternate, tint, frames) in &used {
        match first.get(name.as_slice()) {
            None => {
                first.insert(name, (alternate, tint));
            }
            Some((a, t)) if same(doc, a, alternate, 0) && same(doc, t, tint, 0) => {}
            Some(_) => {
                let message = format!(
                    "the spot {} has another alternate or tint transform than where it is first used",
                    text(name)
                );
                out.push(
                    Finding::new(X4_SPOT_CONSISTENT_DEFINITION, message)
                        .citing("6.4.3.4")
                        .at(frames.clone()),
                );
            }
        }
    }
    dedup(out)
}

/// Every `/MixingHints` dictionary in the file, with the object it is in.
fn mixing_hints(doc: &Document) -> Vec<(u32, &Dict)> {
    dicts(doc)
        .into_iter()
        .filter(|n| n.under(b"MixingHints"))
        .map(|n| (n.num, n.dict))
        .collect()
}

/// 6.4.3.4: all `/PrintingOrder` arrays agree: names that two arrays share come in the same
/// order in both.
pub(super) fn printing_order(doc: &Document, _: &Settings) -> Vec<Finding> {
    let orders: Vec<(u32, Vec<&[u8]>)> = mixing_hints(doc)
        .into_iter()
        .filter_map(|(num, hints)| {
            let order = doc.resolve(hints.get(b"PrintingOrder")?)?.array()?;
            Some((num, order.iter().filter_map(Object::name).collect()))
        })
        .collect();
    let mut out = Vec::new();
    for (i, (num, order)) in orders.iter().enumerate() {
        let clash = orders.iter().take(i).any(|(_, earlier)| {
            let shared: Vec<&&[u8]> = earlier.iter().filter(|n| order.contains(n)).collect();
            let mine: Vec<&&[u8]> = order.iter().filter(|n| earlier.contains(n)).collect();
            shared != mine
        });
        if clash {
            let message = "a /PrintingOrder puts shared inks in another order than an earlier one".to_owned();
            out.push(
                Finding::new(X4_SPOT_PRINTING_ORDER_CONSISTENT, message)
                    .citing("6.4.3.4")
                    .at(vec![Location::Object(*num, 0)]),
            );
        }
    }
    out
}

/// The colours a `/Colorants` Separation and its `DeviceN` must stay within, as CIEDE2000 (6.4.3.4 says "consistent",
/// not equal). Both sides go through the same 8-bit Lab conversion, which alone can part two
/// identical colours by about one unit.
const COLORANTS_DELTA_E: f64 = 2.0;
/// The tints the two are compared at: full tint alone misses a different curve.
const TINTS: [f64; 4] = [0.25, 0.5, 0.75, 1.0];
/// How far apart two alternate components may be, when neither side converts to Lab and the alternates are the same.
const COMPONENT_TOLERANCE: f64 = 0.01;

/// An alternate space colours can be measured in: Lab itself, or device or ICC values through a profile.
enum Measure {
    Lab,
    Gray(Model),
    Rgb(Model),
    Cmyk(Model),
}

/// What an alternate space is measured through, as [`Measure::of`] reads it: the key under which one check builds each
/// [`Measure`] once, since building a `LittleCMS` transform costs far more than using it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Lab,
    Gray,
    Rgb,
    Cmyk,
    /// An `ICCBased` space, by its profile stream's object number.
    Icc(u32),
}

/// The measures one check has built, by source; `None` where a profile does not load or convert.
type Measures = BTreeMap<Source, Option<Measure>>;

impl Source {
    fn of(doc: &Document, alternate: &Object) -> Option<Self> {
        let space = doc.resolve(alternate)?;
        let head = match &space.value {
            Value::Array(items) => items.first().and_then(|h| doc.resolve(h)).and_then(Object::name),
            _ => space.name(),
        };
        match head? {
            b"Lab" => Some(Self::Lab),
            b"DeviceGray" => Some(Self::Gray),
            b"DeviceRGB" => Some(Self::Rgb),
            b"DeviceCMYK" => Some(Self::Cmyk),
            b"ICCBased" => match space.array().and_then(|s| s.get(1)).map(|p| &p.value) {
                Some(Value::Reference(num, _)) => Some(Self::Icc(*num)),
                _ => None,
            },
            _ => None,
        }
    }
}

impl Measure {
    /// How `alternate` is measured: `DeviceCMYK` through the PDF/X output intent's profile, gray as the K plate through
    /// it too (else through sRGB, as RGB is), `ICCBased` RGB and CMYK through their own profile; `None` for anything
    /// else. Built once per source in `measures`.
    fn of<'m>(
        doc: &Document,
        alternate: &Object,
        intent: Option<&[u8]>,
        measures: &'m mut Measures,
    ) -> Option<&'m Self> {
        let source = Source::of(doc, alternate)?;
        measures
            .entry(source)
            .or_insert_with(|| Self::build(doc, source, intent))
            .as_ref()
    }

    fn build(doc: &Document, source: Source, intent: Option<&[u8]>) -> Option<Self> {
        match source {
            Source::Lab => Some(Self::Lab),
            Source::Gray => Model::absolute(intent, None).map(Self::Gray),
            Source::Rgb => Model::absolute(None, None).map(Self::Rgb),
            Source::Cmyk => intent.and_then(|i| Model::absolute(Some(i), None)).map(Self::Cmyk),
            Source::Icc(num) => {
                let (data, dict) = doc.stream_data(num).zip(doc.stream_dict(num))?;
                match dict.get(b"N").and_then(|n| doc.resolve(n)).map(|n| &n.value) {
                    Some(Value::Integer(3)) => Model::absolute(None, Some(&data)).map(Self::Rgb),
                    Some(Value::Integer(4)) => Model::absolute(Some(&data), None).map(Self::Cmyk),
                    _ => None,
                }
            }
        }
    }

    fn lab(&self, values: &[f64]) -> Option<Lab> {
        match (self, values) {
            (Self::Lab, [l, a, b, ..]) => Some([*l, *a, *b]),
            (Self::Gray(model), [g, ..]) => Some(
                model
                    .cmyk([0.0, 0.0, 0.0, 1.0 - g])
                    .unwrap_or_else(|| model.rgb([*g; 3])),
            ),
            (Self::Rgb(model), [r, g, b, ..]) => Some(model.rgb([*r, *g, *b])),
            (Self::Cmyk(model), [c, m, y, k, ..]) => model.cmyk([*c, *m, *y, *k]),
            _ => None,
        }
    }
}

/// How one colorant of a `DeviceN` differs from its `/Colorants` Separation at `tint`, in words; `None` when they agree.
fn colorant_differs(
    doc: &Document,
    device_n: (&[Object], usize, usize),
    separation: &[Object],
    tint: f64,
    intent: Option<&[u8]>,
    measures: &mut Measures,
) -> Option<String> {
    let (items, at, count) = device_n;
    let unevaluated = "could not be evaluated, so their consistency was not checked".to_owned();
    let ([_, _, alternate, own, ..], [_, _, other, theirs, ..]) = (items, separation) else {
        return Some(unevaluated);
    };
    let inputs: Vec<f64> = (0..count).map(|i| if i == at { tint } else { 0.0 }).collect();
    let (Some(one), Some(two)) = (
        function::evaluate(doc, own, &inputs),
        function::evaluate(doc, theirs, &[tint]),
    ) else {
        return Some(unevaluated);
    };
    let percent = tint * 100.0;
    let mut lab_of = |space: &Object, values: &[f64]| Measure::of(doc, space, intent, measures)?.lab(values);
    match (lab_of(alternate, &one), lab_of(other, &two)) {
        (Some(a), Some(b)) => {
            let delta = lab::ciede2000(a, b);
            (delta > COLORANTS_DELTA_E).then(|| format!("are {delta:.1} ΔE00 apart at {percent:.0}% tint"))
        }
        _ if same(doc, alternate, other, 0) => {
            let apart =
                one.len() != two.len() || one.iter().zip(&two).any(|(x, y)| (x - y).abs() > COMPONENT_TOLERANCE);
            apart.then(|| format!("differ at {percent:.0}% tint"))
        }
        _ => Some("are in alternate spaces that cannot be compared".to_owned()),
    }
}

/// 6.4.3.4 (a should): each `/Colorants` Separation of a `DeviceN` that print elements use looks like that colorant
/// of the `DeviceN` alone (the others at no ink), across the tint range. The process colorants of an `NChannel` space
/// are skipped. A space is not judged when it has a `None` colorant, or is an `NChannel` space whose process space is
/// not CMYK, where "no ink" is not zero.
pub(super) fn colorants_match(doc: &Document, _: &Settings) -> Vec<Finding> {
    let intent = intent::profile_stream(doc).map(|(_, data)| data);
    let mut measures = Measures::new();
    let mut out = Vec::new();
    for (items, frames) in used_device_n(doc) {
        let (names, attributes) = names_and_attributes(doc, &items);
        let process = entry(doc, attributes, b"Process").and_then(Object::dict);
        let process_names: Vec<&[u8]> = entry(doc, process, b"Components")
            .and_then(Object::array)
            .map_or_default(|c| c.iter().filter_map(|n| doc.resolve(n)?.name()).collect());
        let additive = entry(doc, process, b"ColorSpace")
            .is_some_and(|space| family::classify(doc, Some(space), None) != Family::Cmyk);
        let colorants = entry(doc, attributes, b"Colorants").and_then(Object::dict);
        // With a `None` colorant the transform may draw the look from the `None` components alone, as a duotone's
        // lookup table does, so no colorant has a look of its own to compare.
        if additive || names.contains(&b"None".as_slice()) {
            continue;
        }
        for (at, name) in names.iter().enumerate() {
            let Some(separation) = colorants
                .and_then(|c| entry(doc, Some(c), name))
                .and_then(Object::array)
                .filter(|_| !process_names.contains(name))
            else {
                continue;
            };
            let differs = TINTS.iter().find_map(|tint| {
                colorant_differs(
                    doc,
                    (&items, at, names.len()),
                    separation,
                    *tint,
                    intent.as_deref(),
                    &mut measures,
                )
            });
            if let Some(how) = differs {
                let message = format!(
                    "the DeviceN colorant {} and its /Colorants Separation {how}",
                    text(name)
                );
                out.push(
                    Finding::new(X4_SPOT_COLORANTS_MATCH_DEVICEN, message)
                        .citing("6.4.3.4")
                        .at(frames.clone()),
                );
            }
        }
    }
    dedup(out)
}

/// 6.4.3.4: every `/Solidities` value for one ink is the same across the file.
pub(super) fn solidities(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut seen: BTreeMap<Vec<u8>, f64> = BTreeMap::new();
    let mut out = Vec::new();
    for (num, hints) in mixing_hints(doc) {
        let Some(values) = hints
            .get(b"Solidities")
            .and_then(|s| doc.resolve(s))
            .and_then(Object::dict)
        else {
            continue;
        };
        for (ink, value) in values.entries() {
            let Some(value) = doc.resolve(value).and_then(|v| number(&v.value)) else {
                continue;
            };
            if seen.get(ink).is_some_and(|v| !equal(*v, value)) {
                let message = format!(
                    "the ink {} has another /Solidities value than an earlier /MixingHints gives it",
                    text(ink)
                );
                out.push(
                    Finding::new(X4_SPOT_SOLIDITIES_CONSISTENT, message)
                        .citing("6.4.3.4")
                        .at(vec![Location::Object(num, 0)]),
                );
            }
            seen.entry(ink.clone()).or_insert(value);
        }
    }
    out
}

/// 6.20 (X4-Q19): an `ExtGState`'s `/BM`, a name or an array of names, uses only the PDF Reference's blend modes.
pub(super) fn blend_modes(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for node in dicts(doc)
        .iter()
        .filter(|n| n.inside(b"ExtGState") || n.name(b"Type") == Some(b"ExtGState"))
    {
        let Some(bm) = node.dict.get(b"BM").and_then(|b| doc.resolve(b)) else {
            continue;
        };
        let modes: Vec<&Object> = match &bm.value {
            Value::Array(items) => items.iter().collect(),
            _ => vec![bm],
        };
        for mode in modes {
            if !mode.name().is_some_and(|m| BLEND_MODES.contains(&m)) {
                let shown = mode.name().map_or_else(|| "not a name".to_owned(), text);
                let message = format!("the blend mode {shown} is not in the PDF Reference");
                out.push(
                    Finding::new(X4_TRANSPARENCY_BLEND_MODES, message)
                        .citing("6.20")
                        .at(vec![Location::Object(node.num, 0)]),
                );
            }
        }
    }
    out
}

/// One finding per distinct message and place.
fn dedup(findings: Vec<Finding>) -> Vec<Finding> {
    let mut seen = std::collections::BTreeSet::new();
    findings
        .into_iter()
        .filter(|f| seen.insert((f.message.clone(), format!("{:?}", f.chain))))
        .collect()
}

/// ISO 15930-8 7.3: a `DeviceN` space's attributes give `/Subtype /DeviceN`, if any: no `NChannel` under PDF/X-5n.
pub(super) fn x5n_devicen_subtype(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for (items, frames) in used_device_n(doc) {
        let (_, attributes) = names_and_attributes(doc, &items);
        let subtype = entry(doc, attributes, b"Subtype").and_then(Object::name);
        if let Some(other) = subtype.filter(|s| *s != b"DeviceN") {
            let message = format!(
                "a DeviceN space's attributes have /Subtype /{}, not /DeviceN",
                text(other)
            );
            out.push(
                Finding::new(X5_N_DEVICEN_SUBTYPE, message)
                    .citing("7.3")
                    .at(frames.clone()),
            );
        }
    }
    dedup(out)
}

/// ISO 15930-8 7.3: no `DeviceN` space's attributes have `/Process` under PDF/X-5n.
pub(super) fn x5n_devicen_no_process(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    for (items, frames) in used_device_n(doc) {
        let (_, attributes) = names_and_attributes(doc, &items);
        if entry(doc, attributes, b"Process").is_some() {
            let message = "a DeviceN space's attributes have /Process";
            out.push(
                Finding::new(X5_N_DEVICEN_NO_PROCESS, message)
                    .citing("7.3")
                    .at(frames.clone()),
            );
        }
    }
    dedup(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    const RUNS: [fn(&Document, &Settings) -> Vec<Finding>; 8] = [
        colorants_entry,
        process_space,
        no_dot_gain,
        confusing_names,
        consistent_definitions,
        printing_order,
        solidities,
        blend_modes,
    ];

    fn profile(space: &[u8]) -> Vec<u8> {
        let mut icc = vec![0u8; 128];
        icc.get_mut(16..20).unwrap().copy_from_slice(space);
        icc.get_mut(36..40).unwrap().copy_from_slice(b"acsp");
        let mut out = format!("<< /N 4 /Length {} >>\nstream\n", icc.len()).into_bytes();
        out.extend(icc);
        out.extend(b"\nendstream");
        out
    }

    fn ids(intent: &[u8], resources: &str, data: &str, more: &[(u32, &[u8])]) -> Vec<&'static str> {
        let catalog = b"<< /Pages 2 0 R /OutputIntents [<< /S /GTS_PDFX /DestOutputProfile 8 0 R >>] >>";
        let page = format!("<< /Type /Page /Contents 4 0 R /Group << /S /Transparency >> /Resources {resources} >>");
        let content = format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len());
        let mut objects: Vec<(u32, &[u8])> = vec![
            (1, catalog),
            (2, b"<< /Type /Pages /Kids [3 0 R] >>"),
            (3, page.as_bytes()),
            (4, content.as_bytes()),
            (8, intent),
        ];
        objects.extend_from_slice(more);
        let doc = Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        RUNS.iter()
            .flat_map(|run| run(&doc, &Level::X4.into()))
            .map(|f| f.check.id)
            .collect()
    }

    #[test]
    fn spot_rows_on_odd_and_good_spaces() {
        let cmyk = profile(b"CMYK");
        let spaces = "<< /ColorSpace << \
            /A [/DeviceN [/Cyan /All /None] /DeviceCMYK 9 0 R << /Colorants << /All [/Separation /All /DeviceCMYK 9 0 R] >> \
                /Process << /ColorSpace [/ICCBased 10 0 R] >> >>] \
            /B [/Indexed [/DeviceN [/Grün] /DeviceCMYK 9 0 R << /Colorants 5 /Process << /ColorSpace [/ICCBased 11 0 R] >> >>] 0 <00>] \
            /C [/DeviceN 5 /DeviceCMYK 9 0 R << /Process 5 /MixingHints << >> >>] \
            /D [/Separation /Grey /DeviceCMYK 12 0 R] /E [/Separation /Grey /DeviceCMYK 13 0 R] \
            /F [/Separation /Gray 9 0 R 9 0 R] /G [/Separation /Gray /DeviceCMYK 9 0 R] \
            /H [/Separation /Rot /DeviceCMYK [1 2.0]] /I [/Separation /Rot /DeviceCMYK [1.0 2]] /J [/Separation 5 /X 9 0 R] \
            /K [/Separation /Q /X << /A 1 /Length 9 >>] /L [/Separation /Q /X << /A 1 >>] /M [/Separation /R /X << /A 1 >>] /N [/Separation /R /X << /B 1 >>] >> \
            /ExtGState << /G << /BM [/Normal 5] >> /H << /BM /Multiply >> >> >>";
        let more: [(u32, &[u8]); 5] = [
            (9, b"<< /FunctionType 2 >>"),
            (10, b"<< /N 4 >>"),
            (11, b"<< /N 3 >>"),
            (12, b"<< /Length 1 >>\nstream\na\nendstream"),
            (13, b"<< /Length 1 >>\nstream\nb\nendstream"),
        ];
        let got = ids(
            &cmyk,
            spaces,
            "/A cs f /B cs f /C cs f /D cs f /E cs f /F cs f /G cs f /H cs f /I cs f /J cs f /K cs f /L cs f /M cs f /N cs f",
            &more,
        );
        assert_eq!(
            got,
            [
                "x4.spot.colorants-entry",
                "x4.spot.process-colorspace",
                "x4.spot.confusing-names",
                "x4.spot.confusing-names",
                "x4.spot.confusing-names",
                "x4.spot.consistent-definition",
                "x4.spot.consistent-definition",
                "x4.spot.consistent-definition",
                "x4.transparency.blend-modes",
            ]
        );
        assert!(
            !ids(b"<< >>", spaces, "/A cs f", &more).contains(&"x4.spot.process-colorspace"),
            "no intent"
        );
        for (intent, family) in [
            (b"GRAY", "/DeviceGray"),
            (b"RGB ", "/DeviceRGB"),
            (b"Lab ", "/DeviceRGB"),
        ] {
            let space = format!(
                "<< /ColorSpace << /A [/DeviceN [/S] /DeviceCMYK 9 0 R << /Colorants << /S 9 0 R >> /Process << /ColorSpace {family} >> >>] >> >>"
            );
            assert_eq!(
                ids(&profile(intent), &space, "/A cs f", &more),
                Vec::<&str>::new(),
                "{family}"
            );
        }
    }

    #[test]
    fn mixing_hints_across_the_file() {
        let spaces = "<< /X << /MixingHints << /PrintingOrder [/A /B /C] /Solidities << /A 1 /B 0.5 /C (x) >> >> >> \
            /Y << /MixingHints << /PrintingOrder [/C /D] /Solidities << /A 1.0 /B 0.5 >> >> >> \
            /Z << /MixingHints << /PrintingOrder [/B /A] /Solidities << /B 0.25 >> >> >> /W << /MixingHints << /Solidities 5 /PrintingOrder 5 >> >> >>";
        let got = ids(&profile(b"CMYK"), spaces, "", &[]);
        assert_eq!(
            got,
            ["x4.spot.printing-order-consistent", "x4.spot.solidities-consistent"]
        );
    }

    #[test]
    fn deep_and_odd_definitions() {
        let nest = |depth: usize| "[".repeat(depth) + "1" + &"]".repeat(depth);
        let spaces = format!(
            "<< /ColorSpace << /A [/Separation /S /DeviceCMYK {}] /B [/Separation /S /DeviceCMYK {}] >> \
             /X << /MixingHints << /PrintingOrder 9 0 R >> >> >>",
            nest(20),
            nest(20)
        );
        let group = "<< /Type /Page /Group << /S /Transparency >> >>";
        let more: [(u32, &[u8]); 1] = [(5, group.as_bytes())];
        assert_eq!(
            ids(&profile(b"CMYK"), &spaces, "/A cs f /B cs f", &more),
            ["x4.spot.consistent-definition"],
            "deeper than compared"
        );
        let looped: [(u32, &[u8]); 1] = [(7, b"[/Indexed 7 0 R 0 <00>]")];
        let spaces = "<< /ColorSpace << /Z 7 0 R /Y [/CalRGB << >>] >> >>";
        assert_eq!(
            ids(&profile(b"CMYK"), spaces, "/Z cs f /Y cs f", &looped),
            Vec::<&str>::new(),
            "a space that is its own base"
        );
    }

    #[test]
    fn the_rest_of_the_rows() {
        let spaces = "<< /ColorSpace << /A [/DeviceN [/Rouge] /DeviceCMYK 9 0 R << /Colorants << /Rouge 9 0 R >> /Subtype /NChannel \
            /MixingHints << /DotGain << >> >> >>] /O [/Separation /S2 /DeviceCMYK 99 0 R] /P [/Separation /S2 /DeviceCMYK 99 0 R] >> \
            /ExtGState << /G << >> /H << /BM [/Normal /Screen] >> >> /X << /MixingHints << >> >> >>";
        let more: [(u32, &[u8]); 1] = [(9, b"<< /FunctionType 2 >>")];
        let got = ids(&profile(b"CMYK"), spaces, "/A cs f /O cs f /P cs f", &more);
        assert_eq!(
            got,
            [
                "x4.spot.no-dotgain",
                "x4.spot.confusing-names",
                "x4.spot.consistent-definition"
            ],
            "a definition that refers to nothing is not the same as another"
        );
    }

    /// The test CMYK profile as a stream object: the output intent's, and an `ICCBased` space's.
    fn intent_stream() -> Vec<u8> {
        let icc = include_bytes!("../../../tests/fixtures/icc/little-cms-test1.icc");
        let mut intent = format!("<< /N 4 /Length {} >>\nstream\n", icc.len()).into_bytes();
        intent.extend(icc);
        intent.extend(b"\nendstream");
        intent
    }

    /// `DeviceN` spaces with `/Colorants`, consistent or not in the ways the row tells apart.
    fn colorant_spaces() -> Vec<String> {
        let two = |c1: &str| format!("<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [{c1}] /N 1 >>");
        let sep = |name: &str, alt: &str, f: &str| format!("[/Separation /{name} {alt} {f}]");
        let gold_silver = format!(
            "[/DeviceN [/Gold /Silver] /DeviceCMYK 9 0 R << /Colorants << /Gold {} /Silver {} >> >>]",
            sep("Gold", "/DeviceCMYK", &two("0 1 0 0")),
            sep("Silver", "/DeviceCMYK", &two("1 0 0 0")),
        );
        let duotone = format!(
            "[/DeviceN [/Black /None] /DeviceCMYK 99 0 R << /Colorants << /Black {} >> >>]",
            sep("Black", "/DeviceCMYK", &two("0 0 0 1")),
        );
        let gray_k = format!(
            "[/DeviceN [/Ink] /DeviceGray << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >> << /Colorants << /Ink {} >> >>]",
            sep("Ink", "/DeviceCMYK", &two("0 0 0 1"))
        );
        let lab = "[/Lab << /WhitePoint [0.9642 1 0.8249] >>]";
        let lab_pair = format!(
            "[/DeviceN [/L] {lab} << /FunctionType 2 /Domain [0 1] /C0 [100 0 0] /C1 [50 10 0] /N 1 >> << /Colorants << /L {} >> >>]",
            sep(
                "L",
                lab,
                "<< /FunctionType 2 /Domain [0 1] /C0 [100 0 0] /C1 [50 10 0] /N 1 >>"
            )
        );
        let cal = "[/CalRGB << /WhitePoint [1 1 1] >>]";
        let cal_same = format!(
            "[/DeviceN [/P /Q] {cal} 10 0 R << /Colorants << /P {} /Q {} >> >>]",
            sep(
                "P",
                cal,
                "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 0 0] /N 1 >>"
            ),
            sep(
                "Q",
                cal,
                "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 1 0] /N 1 >>"
            ),
        );
        let odd = format!(
            "[/DeviceN [/R /S /T /U /V /W] /DeviceRGB 11 0 R << /Colorants << /R {} /S {} /T {} /U [/Separation /U] /V {} /W {} >> >>]",
            sep(
                "R",
                "[/CalGray << /WhitePoint [1 1 1] >>]",
                "<< /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>"
            ),
            sep("S", "/DeviceRGB", "<< /FunctionType 9 >>"),
            sep(
                "T",
                "/DeviceRGB",
                "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 1 1] /N 1 >>"
            ),
            sep("V", "[/ICCBased 12 0 R]", &two("0 0 0 1")),
            sep("W", "[/ICCBased << /N 4 >>]", &two("0 0 0 1")),
        );
        let icc_kinds = format!(
            "[/DeviceN [/X /Y /Z] /DeviceCMYK 13 0 R << /Colorants << /X {} /Y {} /Z {} >> >>]",
            sep("X", "[/ICCBased 14 0 R]", &two("0 0 0 1")),
            sep(
                "Y",
                "[/ICCBased 15 0 R]",
                "<< /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>"
            ),
            sep("Z", lab, "<< /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>"),
        );
        let nchannel = format!(
            "[/DeviceN [/Cyan /Spot] /DeviceCMYK 16 0 R << /Subtype /NChannel /Process << /ColorSpace /DeviceCMYK /Components [/Cyan 99 0 R] >> /Colorants << /Cyan {} /Spot {} >> >>]",
            sep("Cyan", "/DeviceCMYK", &two("0 0 1 0")),
            sep("Spot", "/DeviceCMYK", &two("1 0 0 0")),
        );
        let additive = format!(
            "[/DeviceN [/A] /DeviceRGB 9 0 R << /Subtype /NChannel /Process << /ColorSpace /DeviceRGB >> /Colorants << /A {} >> >>]",
            sep("A", "/DeviceCMYK", &two("0 0 1 0")),
        );
        vec![
            gold_silver,
            duotone,
            gray_k,
            lab_pair,
            cal_same,
            odd,
            icc_kinds,
            nchannel,
            additive,
            format!(
                "[/DeviceN [/A1 /A2 /A3 /A4 /A5] /DeviceRGB 17 0 R << /Colorants << /A1 {} /A2 {} /A3 {} /A4 {} /A5 {} >> >>]",
                sep("A1", "99 0 R", &two("0 0 0 1")),
                sep("A2", "5", &two("0 0 0 1")),
                sep("A3", "[/ICCBased]", &two("0 0 0 1")),
                sep("A4", "[/ICCBased 99 0 R]", &two("0 0 0 1")),
                sep("A5", "[/ICCBased 18 0 R]", &two("0 0 0 1")),
            ),
        ]
    }

    /// A page painting with each of [`colorant_spaces`], its functions and profiles beside it.
    fn colorant_doc() -> Document {
        let intent = intent_stream();
        let spaces = colorant_spaces();
        let resources: Vec<String> = spaces.iter().enumerate().map(|(i, s)| format!("/C{i} {s}")).collect();
        let resources = format!("<< /ColorSpace << {} >> >>", resources.join(" "));
        let data: Vec<String> = (0..spaces.len()).map(|i| format!("/C{i} cs 0 0 1 1 re f")).collect();
        let calculator = |outputs: usize, program: &str| {
            let range = "0 1 ".repeat(outputs);
            format!("<< /FunctionType 4 /Domain [0 1 0 1 0 1 0 1 0 1 0 1] /Range [{range}] /Length {} >>\nstream\n{program}\nendstream", program.len())
                .into_bytes()
        };
        let objects = [
            (9, calculator(4, "{ pop 0 exch 0 0 }")),
            (10, calculator(3, "{ pop 0 0 }")),
            (11, calculator(3, "{ pop pop pop pop pop pop 0 0 0 }")),
            (12, intent.clone()),
            (13, calculator(4, "{ pop pop 0 0 0 4 -1 roll }")),
            (14, intent.clone()),
            (15, b"<< /N 1 /Length 0 >>\nstream\n\nendstream".to_vec()),
            (16, calculator(4, "{ exch pop 0 0 0 }")),
            (17, calculator(3, "{ pop pop pop pop pop 0 0 0 }")),
            (18, [b"<< /N 3".as_slice(), intent.get(7..).unwrap()].concat()),
        ];
        let more: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        let catalog = b"<< /Pages 2 0 R /OutputIntents [<< /S /GTS_PDFX /DestOutputProfile 8 0 R >>] >>";
        let page = format!("<< /Type /Page /Contents 4 0 R /Resources {resources} >>");
        let content = data.join(" ");
        let content = format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len());
        let mut all: Vec<(u32, &[u8])> = vec![
            (1, catalog),
            (2, b"<< /Type /Pages /Kids [3 0 R] >>"),
            (3, page.as_bytes()),
            (4, content.as_bytes()),
            (8, &intent),
        ];
        all.extend(more);
        Document::open(&classic(&all, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap()
    }

    #[test]
    fn colorants_against_their_device_n() {
        let got: Vec<String> = colorants_match(&colorant_doc(), &Level::X4.into())
            .into_iter()
            .map(|f| f.message)
            .collect();
        let say = |name: &str, how: &str| format!("the DeviceN colorant {name} and its /Colorants Separation {how}");
        let unevaluated = "could not be evaluated, so their consistency was not checked";
        let apart = "are in alternate spaces that cannot be compared";
        assert_eq!(
            got,
            [
                say("Silver", "are 16.8 ΔE00 apart at 25% tint"),
                say("Q", "differ at 25% tint"),
                say("R", apart),
                say("S", unevaluated),
                say("T", "are 17.5 ΔE00 apart at 25% tint"),
                say("U", unevaluated),
                say("V", "are 59.8 ΔE00 apart at 25% tint"),
                say("W", apart),
                say("Y", apart),
                say("Z", apart),
                say("A1", apart),
                say("A2", apart),
                say("A3", apart),
                say("A4", apart),
                say("A5", apart),
            ]
        );
        let srgb = || Model::absolute(None, None).unwrap();
        assert_eq!(
            Measure::Gray(srgb()).lab(&[0.5]),
            Some(srgb().rgb([0.5; 3])),
            "gray with no intent is sRGB"
        );
    }
}
