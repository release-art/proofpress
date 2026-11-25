//! Tier 7: the page model and its paint events (ISO 32000-2, clause 8).
//!
//! `walk` interprets each page's content with a graphics state and reports what is painted: for every fill, stroke,
//! text, image, image mask and shading, the colour space each part of it uses and whether that part overprints, and
//! for every transparency group its blend space. Forms and coloured tiling patterns are walked where they are used,
//! with one set of ancestors, so a form that draws itself is a cycle and a gap; a form without `/Resources` uses the
//! page's (7.8.3), with a problem noted.
//!
//! A device colour space with a matching `Default` space in the resources in effect is reported as that default
//! (8.6.5.6), since PDF/X-4 6.4.3.1 judges it so. Geometry (the CTM, bounding boxes, clips) comes with the print checks
//! that need it.
//
// ponytail: geometry, clips, soft-mask content, Type 3 glyph procedures and annotation appearances are not walked yet;
// they come with the checks that read them (tasks 4.6 and phase 5).

use std::collections::BTreeSet;

use crate::content::{self, Item};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::Location;
use crate::pipeline::Limits;

/// What a colour space is used for in a paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Fill,
    Stroke,
    /// A sampled image's own colour space.
    Image,
    /// An image mask, painted in the fill colour.
    ImageMask,
    /// A shading's colour space, from `sh` or a shading pattern.
    Shading,
}

/// One use of a colour space in a paint.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Use {
    pub(crate) role: Role,
    /// The colour space: a name, an array, or a reference to one, as the resources give it; after `Default`
    /// substitution.
    pub(crate) space: Object,
    /// Whether a `Default` space replaced a device space.
    pub(crate) defaulted: bool,
    /// Whether this part overprints: `op` for fills, text fills and image masks, `OP` for strokes.
    pub(crate) overprint: bool,
    /// The overprint mode in effect.
    pub(crate) opm: i64,
}

/// What tier 7 reports.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    /// Something painted, and where: the page, then each form or pattern on the way.
    Paint { uses: Vec<Use>, frames: Vec<Location> },
    /// A transparency group, with its `/CS` when it has one.
    Group {
        space: Option<Object>,
        frames: Vec<Location>,
    },
    /// Something that kept part of a page from being walked, or that the walk had to stand in for.
    Problem { problem: Problem, frames: Vec<Location> },
}

/// What tier 7 found wrong while walking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Problem {
    /// A form or pattern that draws itself, directly or not; it is not walked again.
    Cycle,
    /// Forms and patterns nested deeper than the limit; the deeper ones are not walked.
    TooDeep,
    /// A form without `/Resources`, walked with the page's (7.8.3).
    NoResources,
}

/// The graphics state tier 7 keeps.
#[derive(Clone)]
struct State {
    fill: Object,
    stroke: Object,
    fill_pattern: Option<Vec<u8>>,
    op_fill: bool,
    op_stroke: bool,
    opm: i64,
    text_mode: i64,
}

impl Default for State {
    fn default() -> Self {
        let gray = Object {
            value: Value::Name(b"DeviceGray".to_vec()),
            start: 0,
            end: 0,
        };
        Self {
            fill: gray.clone(),
            stroke: gray,
            fill_pattern: None,
            op_fill: false,
            op_stroke: false,
            opm: 0,
            text_mode: 0,
        }
    }
}

/// A walk in progress.
struct Walker<'a> {
    doc: &'a Document,
    limits: Limits,
    events: Vec<Event>,
    /// The forms and patterns being walked, by object number.
    ancestors: BTreeSet<u32>,
}

/// Every page's events, in page order.
pub(crate) fn walk(doc: &Document) -> Vec<Event> {
    let mut walker = Walker {
        doc,
        limits: Limits::default(),
        events: Vec::new(),
        ancestors: BTreeSet::new(),
    };
    for (i, page) in doc.pages().iter().enumerate() {
        let frames = vec![Location::Page(u32::try_from(i + 1).unwrap_or(u32::MAX))];
        let resources = page
            .get(b"Resources")
            .and_then(|r| doc.resolve(r))
            .and_then(Object::dict);
        if let Some(group) = page
            .dict
            .get(b"Group")
            .and_then(|g| doc.resolve(g))
            .and_then(Object::dict)
        {
            walker.group(group, &frames);
        }
        let streams = stream_numbers(doc, page.get(b"Contents"));
        let data: Vec<Vec<u8>> = streams.iter().filter_map(|&n| doc.stream_data(n)).collect();
        walker.content(&data, resources, resources, &frames, State::default());
    }
    walker.events
}

/// The object numbers of the streams `contents` names: one stream, or an array of them, direct or by reference.
pub(crate) fn stream_numbers(doc: &Document, contents: Option<&Object>) -> Vec<u32> {
    let Some(contents) = contents else { return Vec::new() };
    match (&contents.value, doc.resolve(contents).map(|o| &o.value)) {
        (_, Some(Value::Array(items))) => items
            .iter()
            .filter_map(|i| match i.value {
                Value::Reference(num, _) => Some(num),
                _ => None,
            })
            .collect(),
        (Value::Reference(num, _), _) => vec![*num],
        _ => Vec::new(),
    }
}

/// The number of components of a colour space (8.6), when it can be told.
pub(crate) fn components(doc: &Document, space: &Object) -> Option<usize> {
    let space = doc.resolve(space)?;
    let (family, rest) = match &space.value {
        Value::Name(name) => (name.as_slice(), &[][..]),
        Value::Array(items) => (items.first()?.name()?, items.get(1..).unwrap_or_default()),
        _ => return None,
    };
    let arg = |i: usize| rest.get(i).and_then(|o| doc.resolve(o));
    match family {
        b"DeviceGray" | b"CalGray" | b"Indexed" | b"Separation" => Some(1),
        b"DeviceRGB" | b"CalRGB" | b"Lab" => Some(3),
        b"DeviceCMYK" => Some(4),
        b"ICCBased" => arg(0)?
            .dict()?
            .get(b"N")?
            .integer()
            .and_then(|n| usize::try_from(n).ok()),
        b"DeviceN" => arg(0)?.array().map(<[Object]>::len),
        _ => None,
    }
}

/// The resource `name` of `category` in `resources`, resolved.
fn resource<'a>(doc: &'a Document, resources: Option<&'a Dict>, category: &[u8], name: &[u8]) -> Option<&'a Object> {
    let entries = doc.resolve(resources?.get(category)?)?.dict()?;
    doc.resolve(entries.get(name)?)
}

/// The resource `name` of `category` with its object number, when it is a reference.
fn resource_ref<'a>(
    doc: &'a Document,
    resources: Option<&'a Dict>,
    category: &[u8],
    name: &[u8],
) -> Option<(Option<u32>, &'a Dict)> {
    let entry = doc.resolve(resources?.get(category)?)?.dict()?.get(name)?;
    let num = match entry.value {
        Value::Reference(num, _) => Some(num),
        _ => None,
    };
    Some((num, doc.resolve(entry)?.dict()?))
}

impl Walker<'_> {
    /// A transparency group dictionary's event.
    fn group(&mut self, group: &Dict, frames: &[Location]) {
        if group.get(b"S").and_then(Object::name) == Some(b"Transparency") {
            let space = group.get(b"CS").and_then(|c| self.doc.resolve(c)).cloned();
            self.events.push(Event::Group {
                space,
                frames: frames.to_vec(),
            });
        }
    }

    fn problem(&mut self, problem: Problem, frames: &[Location]) {
        self.events.push(Event::Problem {
            problem,
            frames: frames.to_vec(),
        });
    }

    /// A colour space operand or resource name made a colour space: the standard families by name, others from the
    /// resources.
    fn space(&self, resources: Option<&Dict>, operand: Option<&Object>) -> Option<Object> {
        let operand = operand?;
        match operand.name()? {
            b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK" | b"Pattern" => Some(operand.clone()),
            other => resource(self.doc, resources, b"ColorSpace", other).cloned(),
        }
    }

    /// `space` with its `Default` space when it is a device space and the resources have one (8.6.5.6).
    fn defaulted(&self, resources: Option<&Dict>, space: &Object) -> (Object, bool) {
        let default: &[u8] = match space.name() {
            Some(b"DeviceGray") => b"DefaultGray",
            Some(b"DeviceRGB") => b"DefaultRGB",
            Some(b"DeviceCMYK") => b"DefaultCMYK",
            _ => return (space.clone(), false),
        };
        match resource(self.doc, resources, b"ColorSpace", default) {
            Some(found) => (found.clone(), true),
            None => (space.clone(), false),
        }
    }

    fn use_of(&self, resources: Option<&Dict>, role: Role, space: &Object, state: &State) -> Use {
        let (space, defaulted) = self.defaulted(resources, space);
        let overprint = if role == Role::Stroke {
            state.op_stroke
        } else {
            state.op_fill
        };
        Use {
            role,
            space,
            defaulted,
            overprint,
            opm: state.opm,
        }
    }

    /// Paint with the current fill or stroke colour; a fill with a pattern also paints what the pattern does.
    fn paint(
        &mut self,
        resources: Option<&Dict>,
        roles: &[Role],
        state: &State,
        frames: &[Location],
        page: Option<&Dict>,
    ) {
        let mut uses = Vec::new();
        for &role in roles {
            let space = if role == Role::Stroke {
                &state.stroke
            } else {
                &state.fill
            };
            uses.push(self.use_of(resources, role, space, state));
            let pattern = state
                .fill_pattern
                .as_ref()
                .filter(|_| role != Role::Stroke && self.is_pattern(space));
            if let Some((num, dict)) = pattern.and_then(|p| resource_ref(self.doc, resources, b"Pattern", p)) {
                self.pattern(num, dict, resources, state, frames, page, &mut uses);
            }
        }
        if !uses.is_empty() {
            self.events.push(Event::Paint {
                uses,
                frames: frames.to_vec(),
            });
        }
    }

    /// Whether `space` is the Pattern family, alone or with an underlying space.
    fn is_pattern(&self, space: &Object) -> bool {
        let resolved = self.doc.resolve(space);
        let family = resolved
            .and_then(Object::array)
            .and_then(|a| a.first())
            .or(resolved)
            .and_then(Object::name);
        family == Some(b"Pattern")
    }

    /// What a pattern paints: a shading pattern its shading's colour space; a coloured tiling pattern its content.
    #[expect(
        clippy::too_many_arguments,
        reason = "the walk's context, passed down as the other paint methods take it"
    )]
    fn pattern(
        &mut self,
        num: Option<u32>,
        dict: &Dict,
        resources: Option<&Dict>,
        state: &State,
        frames: &[Location],
        page: Option<&Dict>,
        uses: &mut Vec<Use>,
    ) {
        match dict.get(b"PatternType").and_then(Object::integer) {
            Some(2) => {
                let shading = dict
                    .get(b"Shading")
                    .and_then(|s| self.doc.resolve(s))
                    .and_then(Object::dict);
                if let Some(space) = shading.and_then(|s| s.get(b"ColorSpace")) {
                    uses.push(self.use_of(resources, Role::Shading, space, state));
                }
            }
            Some(1) if dict.get(b"PaintType").and_then(Object::integer) == Some(1) => {
                if let Some(num) = num {
                    self.nested(num, dict, frames, page);
                }
            }
            _ => {}
        }
    }

    /// Walk the form or coloured tiling pattern `num`, whose dictionary is `dict`.
    fn nested(&mut self, num: u32, dict: &Dict, frames: &[Location], page: Option<&Dict>) {
        let mut inner = frames.to_vec();
        inner.push(Location::Object(num, 0));
        if self.ancestors.contains(&num) {
            self.problem(Problem::Cycle, &inner);
            return;
        }
        if self.ancestors.len() >= self.limits.nesting {
            self.problem(Problem::TooDeep, &inner);
            return;
        }
        let resources = if let Some(own) = dict
            .get(b"Resources")
            .and_then(|r| self.doc.resolve(r))
            .and_then(Object::dict)
        {
            Some(own)
        } else {
            self.problem(Problem::NoResources, &inner);
            page
        };
        if let Some(group) = dict
            .get(b"Group")
            .and_then(|g| self.doc.resolve(g))
            .and_then(Object::dict)
        {
            self.group(group, &inner);
        }
        let data: Vec<Vec<u8>> = self.doc.stream_data(num).into_iter().collect();
        self.ancestors.insert(num);
        self.content(&data, resources, page, &inner, State::default());
        self.ancestors.remove(&num);
    }

    /// Interpret one content with `resources` in effect.
    fn content(
        &mut self,
        data: &[Vec<u8>],
        resources: Option<&Dict>,
        page: Option<&Dict>,
        frames: &[Location],
        initial: State,
    ) {
        let doc = self.doc;
        let slices: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
        let named = |n: &[u8]| components(doc, resource(doc, resources, b"ColorSpace", n)?);
        let content = content::read(&slices, &self.limits, &named);
        let mut stack: Vec<State> = Vec::new();
        let mut state = initial;
        for item in &content.items {
            let op = match item {
                Item::Image(image) => {
                    let mask = matches!(image.dict.get(b"ImageMask").map(|v| &v.value), Some(Value::Bool(true)));
                    if mask {
                        self.paint(resources, &[Role::ImageMask], &state, frames, page);
                    } else if let Some(space) = image
                        .dict
                        .get(b"ColorSpace")
                        .and_then(|cs| self.inline_space(resources, cs))
                    {
                        let used = self.use_of(resources, Role::Image, &space, &state);
                        self.events.push(Event::Paint {
                            uses: vec![used],
                            frames: frames.to_vec(),
                        });
                    }
                    continue;
                }
                Item::Op(op) => op,
            };
            let first = op.operands.first();
            let last = op.operands.last();
            match op.name.as_slice() {
                b"q" => stack.push(state.clone()),
                b"Q" => state = stack.pop().unwrap_or_default(),
                b"CS" => state.stroke = self.space(resources, last).unwrap_or_else(|| state.stroke.clone()),
                b"cs" => {
                    state.fill = self.space(resources, last).unwrap_or_else(|| state.fill.clone());
                    state.fill_pattern = None;
                }
                b"G" | b"g" | b"RG" | b"rg" | b"K" | b"k" => {
                    let family: &[u8] = match op.name.as_slice() {
                        b"G" | b"g" => b"DeviceGray",
                        b"RG" | b"rg" => b"DeviceRGB",
                        _ => b"DeviceCMYK",
                    };
                    let space = Object {
                        value: Value::Name(family.to_vec()),
                        start: 0,
                        end: 0,
                    };
                    if op.name.first().is_some_and(u8::is_ascii_uppercase) {
                        state.stroke = space;
                    } else {
                        state.fill = space;
                    }
                }
                b"scn" => state.fill_pattern = last.and_then(Object::name).map(<[u8]>::to_vec),
                b"gs" => {
                    if let Some(gs) = last
                        .and_then(Object::name)
                        .and_then(|n| resource(self.doc, resources, b"ExtGState", n))
                        .and_then(Object::dict)
                    {
                        self.ext_gstate(gs, &mut state);
                    }
                }
                b"Tr" => state.text_mode = first.and_then(Object::integer).unwrap_or(0),
                b"f" | b"F" | b"f*" => self.paint(resources, &[Role::Fill], &state, frames, page),
                b"S" | b"s" => self.paint(resources, &[Role::Stroke], &state, frames, page),
                b"B" | b"B*" | b"b" | b"b*" => self.paint(resources, &[Role::Fill, Role::Stroke], &state, frames, page),
                b"Tj" | b"TJ" | b"'" | b"\"" => {
                    let roles: &[Role] = match state.text_mode {
                        0 | 4 => &[Role::Fill],
                        1 | 5 => &[Role::Stroke],
                        2 | 6 => &[Role::Fill, Role::Stroke],
                        _ => &[],
                    };
                    self.paint(resources, roles, &state, frames, page);
                }
                b"sh" => {
                    let shading = last
                        .and_then(Object::name)
                        .and_then(|n| resource(self.doc, resources, b"Shading", n))
                        .and_then(Object::dict);
                    if let Some(space) = shading.and_then(|s| s.get(b"ColorSpace")) {
                        let used = self.use_of(resources, Role::Shading, space, &state);
                        self.events.push(Event::Paint {
                            uses: vec![used],
                            frames: frames.to_vec(),
                        });
                    }
                }
                b"Do" => self.xobject(resources, last, &state, frames, page),
                _ => {}
            }
        }
    }

    /// An inline image's colour space: abbreviated names, a resource, or an array.
    fn inline_space(&self, resources: Option<&Dict>, space: &Object) -> Option<Object> {
        let family: &[u8] = match space.name() {
            Some(b"G") => b"DeviceGray",
            Some(b"RGB") => b"DeviceRGB",
            Some(b"CMYK") => b"DeviceCMYK",
            Some(other @ (b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK")) => other,
            Some(other) => return resource(self.doc, resources, b"ColorSpace", other).cloned(),
            None => return Some(space.clone()),
        };
        Some(Object {
            value: Value::Name(family.to_vec()),
            start: 0,
            end: 0,
        })
    }

    /// Apply an `ExtGState`'s overprint settings (8.4.5): `OP` sets both unless `op` is given.
    fn ext_gstate(&self, gs: &Dict, state: &mut State) {
        let flag = |key: &[u8]| match gs.get(key).and_then(|v| self.doc.resolve(v)).map(|v| &v.value) {
            Some(Value::Bool(b)) => Some(*b),
            _ => None,
        };
        if let Some(op) = flag(b"OP") {
            state.op_stroke = op;
            state.op_fill = op;
        }
        if let Some(op) = flag(b"op") {
            state.op_fill = op;
        }
        if let Some(mode) = gs
            .get(b"OPM")
            .and_then(|v| self.doc.resolve(v))
            .and_then(Object::integer)
        {
            state.opm = mode;
        }
    }

    /// `Do`: an image paints its colour space (or the fill colour, for a mask); a form is walked.
    fn xobject(
        &mut self,
        resources: Option<&Dict>,
        operand: Option<&Object>,
        state: &State,
        frames: &[Location],
        page: Option<&Dict>,
    ) {
        let Some(name) = operand.and_then(Object::name) else {
            return;
        };
        let Some(entries) = resources
            .and_then(|r| r.get(b"XObject"))
            .and_then(|x| self.doc.resolve(x))
            .and_then(Object::dict)
        else {
            return;
        };
        let Some(reference) = entries.get(name) else { return };
        let Some(dict) = self.doc.resolve(reference).and_then(Object::dict) else {
            return;
        };
        match dict.get(b"Subtype").and_then(Object::name) {
            Some(b"Image") => {
                if matches!(
                    dict.get(b"ImageMask")
                        .and_then(|v| self.doc.resolve(v))
                        .map(|v| &v.value),
                    Some(Value::Bool(true))
                ) {
                    self.paint(resources, &[Role::ImageMask], state, frames, page);
                } else if let Some(space) = dict.get(b"ColorSpace") {
                    let used = self.use_of(resources, Role::Image, space, state);
                    let mut inner = frames.to_vec();
                    if let Value::Reference(num, _) = reference.value {
                        inner.push(Location::Object(num, 0));
                    }
                    self.events.push(Event::Paint {
                        uses: vec![used],
                        frames: inner,
                    });
                }
            }
            Some(b"Form") => {
                if let Value::Reference(num, _) = reference.value {
                    self.nested(num, dict, frames, page);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use pretty_assertions::assert_eq;

    fn stream(entries: &str, data: &str) -> Vec<u8> {
        format!("<< {entries} /Length {} >>\nstream\n{data}\nendstream", data.len()).into_bytes()
    }

    fn events(objects: &[(u32, &[u8])]) -> Vec<Event> {
        walk(&Document::open(&classic(objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap())
    }

    /// A use as role, the space's name or array's first name, defaulted, overprint, OPM.
    type Shape = (Role, String, bool, bool, i64);

    /// Each paint as its uses.
    fn paints(events: &[Event]) -> Vec<Vec<Shape>> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Paint { uses, .. } => Some(
                    uses.iter()
                        .map(|u| {
                            let space = u.space.array().and_then(|a| a.first()).unwrap_or(&u.space);
                            let name = String::from_utf8_lossy(space.name().unwrap_or(b"?")).into_owned();
                            (u.role, name, u.defaulted, u.overprint, u.opm)
                        })
                        .collect(),
                ),
                _ => None,
            })
            .collect()
    }

    fn page(resources: &str) -> [(u32, Vec<u8>); 3] {
        [
            (1, b"<< /Pages 2 0 R >>".to_vec()),
            (
                2,
                format!("<< /Type /Pages /Kids [3 0 R] /Resources {resources} >>").into_bytes(),
            ),
            (3, "<< /Type /Page /Contents [4 0 R] >>".to_string().into_bytes()),
        ]
    }

    fn run(resources: &str, data: &str, more: &[(u32, Vec<u8>)]) -> Vec<Event> {
        let mut objects: Vec<(u32, Vec<u8>)> = page(resources).to_vec();
        objects.push((4, stream("", data)));
        objects.extend(more.iter().cloned());
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        events(&refs)
    }

    fn use_of(role: Role, name: &str) -> Shape {
        (role, name.to_owned(), false, false, 0)
    }

    #[test]
    fn colour_operators_and_the_graphics_state() {
        let resources = "<< /ColorSpace << /S [/Separation /Spot /DeviceCMYK 9 0 R] /DefaultRGB [/CalRGB << >>] >> \
            /ExtGState << /O << /OP true /OPM 1 >> /F << /op false >> /N << /OP 5 >> >> >>";
        let data = "0 0 1 rg f 1 G S 0 0 0 1 k 0 K B q /S cs /S CS 1 sc b Q f* /O gs 2 Tr (x) Tj /F gs (y) ' 3 Tr (z) TJ \
            1 Tr (w) \" 0 Tr F 7 Tr (v) Tj /N gs /Nope gs 1 RG s n /Nope cs f /DeviceGray cs f";
        let got = paints(&run(resources, data, &[]));
        assert_eq!(
            got,
            [
                vec![(Role::Fill, "CalRGB".to_owned(), true, false, 0)],
                vec![use_of(Role::Stroke, "DeviceGray")],
                vec![use_of(Role::Fill, "DeviceCMYK"), use_of(Role::Stroke, "DeviceCMYK")],
                vec![use_of(Role::Fill, "Separation"), use_of(Role::Stroke, "Separation")],
                vec![use_of(Role::Fill, "DeviceCMYK")],
                vec![
                    (Role::Fill, "DeviceCMYK".to_owned(), false, true, 1),
                    (Role::Stroke, "DeviceCMYK".to_owned(), false, true, 1)
                ],
                vec![
                    (Role::Fill, "DeviceCMYK".to_owned(), false, false, 1),
                    (Role::Stroke, "DeviceCMYK".to_owned(), false, true, 1)
                ],
                vec![(Role::Stroke, "DeviceCMYK".to_owned(), false, true, 1)],
                vec![(Role::Fill, "DeviceCMYK".to_owned(), false, false, 1)],
                vec![(Role::Stroke, "CalRGB".to_owned(), true, true, 1)],
                vec![(Role::Fill, "DeviceCMYK".to_owned(), false, false, 1)],
                vec![(Role::Fill, "DeviceGray".to_owned(), false, false, 1)],
            ]
        );
    }

    #[test]
    fn images_shadings_patterns_and_forms() {
        let resources = "<< /XObject << /I 5 0 R /M 6 0 R /F 7 0 R /X 8 0 R /N 99 /J 12 0 R >> \
            /Shading << /Sh << /ColorSpace /DeviceRGB >> /No << >> >> \
            /Pattern << /P 10 0 R /T 11 0 R /U << /PatternType 1 /PaintType 2 >> /D << /PatternType 2 /Shading << /ColorSpace /DeviceGray >> >> /E << /PatternType 2 >> >> \
            /ColorSpace << /C /DeviceCMYK /PU [/Pattern /DeviceCMYK] >> >>";
        let data = "/I Do /M Do /F Do /X Do /N Do /J Do /Missing Do 5 Do /Sh sh /No sh /Pattern cs /P scn f /T scn f /U scn f \
            /D scn f /E scn f /PU cs /T scn f \
            BI /IM true /W 1 /H 1 ID a EI BI /W 1 /H 1 /BPC 8 /CS /CMYK ID abcd EI BI /W 1 /H 1 /BPC 8 /CS /C ID abcd EI \
            BI /W 1 /H 1 /BPC 8 /CS /RGB ID abc EI BI /W 1 /H 1 /BPC 8 /CS /G ID a EI BI /W 1 /H 1 /BPC 8 /CS /DeviceRGB ID abc EI \
            BI /W 1 /H 1 /BPC 8 /CS [/Indexed /DeviceRGB 0 <000000>] ID a EI";
        let more = [
            (5, stream("/Subtype /Image /ColorSpace /DeviceGray", "")),
            (6, stream("/Subtype /Image /ImageMask true", "")),
            (
                7,
                stream(
                    "/Subtype /Form /Resources << >> /Group << /S /Transparency /CS /DeviceCMYK >>",
                    "0 1 0 rg f",
                ),
            ),
            (8, stream("/Subtype /PS", "")),
            (
                10,
                b"<< /PatternType 2 /Shading << /ColorSpace /DeviceCMYK >> >>".to_vec(),
            ),
            (11, stream("/PatternType 1 /PaintType 1 /Resources << >>", "1 g f")),
            (12, stream("/Subtype /Image", "")),
        ];
        let got = run(resources, data, &more);
        let shapes = paints(&got);
        assert_eq!(
            shapes,
            [
                vec![use_of(Role::Image, "DeviceGray")],
                vec![use_of(Role::ImageMask, "DeviceGray")],
                vec![use_of(Role::Fill, "DeviceRGB")],
                vec![use_of(Role::Shading, "DeviceRGB")],
                vec![use_of(Role::Fill, "Pattern"), use_of(Role::Shading, "DeviceCMYK")],
                vec![use_of(Role::Fill, "DeviceGray")],
                vec![use_of(Role::Fill, "Pattern")],
                vec![use_of(Role::Fill, "Pattern")],
                vec![use_of(Role::Fill, "Pattern"), use_of(Role::Shading, "DeviceGray")],
                vec![use_of(Role::Fill, "Pattern")],
                vec![use_of(Role::Fill, "DeviceGray")],
                vec![use_of(Role::Fill, "Pattern")],
                vec![use_of(Role::Fill, "DeviceGray")],
                vec![use_of(Role::ImageMask, "Pattern")],
                vec![use_of(Role::Image, "DeviceCMYK")],
                vec![use_of(Role::Image, "DeviceCMYK")],
                vec![use_of(Role::Image, "DeviceRGB")],
                vec![use_of(Role::Image, "DeviceGray")],
                vec![use_of(Role::Image, "DeviceRGB")],
                vec![use_of(Role::Image, "Indexed")],
            ]
        );
        assert!(
            got.iter()
                .any(|e| matches!(e, Event::Group { space: Some(_), frames } if frames.len() == 2))
        );
    }

    #[test]
    fn cycles_depth_and_missing_resources() {
        let resources = "<< /XObject << /A 5 0 R /B 6 0 R >> >>";
        let more = [
            (
                5,
                stream("/Subtype /Form /Resources << /XObject << /A 5 0 R >> >>", "/A Do"),
            ),
            (6, stream("/Subtype /Form", "/A Do")),
        ];
        let problems: Vec<Problem> = run(resources, "/A Do /B Do 0 g f", &more)
            .into_iter()
            .filter_map(|e| match e {
                Event::Problem { problem, .. } => Some(problem),
                _ => None,
            })
            .collect();
        assert_eq!(problems, [Problem::Cycle, Problem::NoResources, Problem::Cycle]);
        // A chain of forms deeper than the nesting limit.
        let depth = Limits::default().nesting + 1;
        let mut chain: Vec<(u32, Vec<u8>)> = (0..=depth)
            .map(|i| {
                let n = u32::try_from(10 + i).unwrap();
                (
                    n,
                    stream(
                        &format!("/Subtype /Form /Resources << /XObject << /A {} 0 R >> >>", n + 1),
                        "/A Do",
                    ),
                )
            })
            .collect();
        chain.push((
            u32::try_from(11 + depth).unwrap(),
            stream("/Subtype /Form /Resources << >>", ""),
        ));
        let deep = run("<< /XObject << /A 10 0 R >> >>", "/A Do", &chain);
        assert!(deep.iter().any(|e| matches!(
            e,
            Event::Problem {
                problem: Problem::TooDeep,
                ..
            }
        )));
        let group = run(
            "<< >>",
            "",
            &[(
                3,
                b"<< /Type /Page /Contents 4 0 R /Group << /S /Transparency >> >>".to_vec(),
            )],
        );
        assert_eq!(group.len(), 1);
    }

    #[test]
    fn stream_numbers_and_components() {
        let pdf = classic(
            &[
                (1, b"<< /A [4 0 R 5] /B 6 0 R /C 7 0 R /D 3 >>"),
                (6, b"[4 0 R]"),
                (7, b"<< >>"),
            ],
            "<< /Size SIZE /Root 1 0 R >>",
        );
        let doc = Document::open(&pdf, &Limits::default()).unwrap();
        let catalog = doc.catalog().unwrap();
        let numbers = |k: &[u8]| stream_numbers(&doc, catalog.get(k));
        assert_eq!(
            (
                numbers(b"A"),
                numbers(b"B"),
                numbers(b"C"),
                numbers(b"D"),
                numbers(b"E")
            ),
            (vec![4], vec![4], vec![7], vec![], vec![])
        );
        let space = |text: &[u8]| {
            let pdf = classic(
                &[(1, &[b"<< /X ".as_slice(), text, b" >>"].concat()), (2, b"<< /N 4 >>")],
                "<< /Size SIZE /Root 1 0 R >>",
            );
            let d = Document::open(&pdf, &Limits::default()).unwrap();
            components(&d, d.catalog().unwrap().get(b"X").unwrap())
        };
        assert_eq!(
            [
                b"/DeviceGray".as_slice(),
                b"/Lab",
                b"/DeviceCMYK",
                b"[/ICCBased 2 0 R]",
                b"[/DeviceN [/A /B] /DeviceGray 3]",
                b"/Pattern",
                b"5",
                b"[5]",
                b"9 0 R"
            ]
            .map(space),
            [Some(1), Some(3), Some(4), Some(4), Some(2), None, None, None, None]
        );
    }

    #[test]
    fn resources_that_are_not_there() {
        let data = "/X cs /X CS 5 CS /G gs /F Do /Sh sh /P scn f S BI /W 1 /H 1 /BPC 8 /CS /Q ID a EI";
        let bare = run("<< >>", data, &[]);
        assert_eq!(
            paints(&bare),
            [
                vec![use_of(Role::Fill, "DeviceGray")],
                vec![use_of(Role::Stroke, "DeviceGray")]
            ]
        );
        let odd = "<< /ColorSpace 5 /XObject << /F 9 0 R /G 5 >> /Pattern << /P << /PatternType 1 /PaintType 1 >> >> /Shading 5 >>";
        let got = run(odd, "/Pattern cs /P scn f /F Do /G Do", &[]);
        assert_eq!(
            paints(&got),
            [vec![use_of(Role::Fill, "Pattern")]],
            "a pattern written directly is not walked"
        );
        let page_without = run("<< >>", "q", &[(3, b"<< /Type /Page /Contents 4 0 R >>".to_vec())]);
        assert_eq!(page_without, []);
        let pdf = classic(
            &[(1, b"<< /R << /ColorSpace 5 /XObject << /F 9 0 R /K << /Subtype /Form >> >> /Pattern << /P 5 >> >> /S << /ColorSpace 9 0 R >> >>")],
            "<< /Size SIZE /Root 1 0 R >>",
        );
        let doc = Document::open(&pdf, &Limits::default()).unwrap();
        let catalog = doc.catalog().unwrap();
        let r = catalog.get(b"R").and_then(Object::dict);
        let s = catalog.get(b"S").and_then(Object::dict);
        assert!(resource(&doc, None, b"ColorSpace", b"A").is_none());
        assert!(
            resource(&doc, r, b"ColorSpace", b"A").is_none(),
            "a category that is not a dictionary"
        );
        assert!(
            resource(&doc, s, b"ColorSpace", b"A").is_none(),
            "a category that refers to nothing"
        );
        assert!(
            resource(&doc, r, b"XObject", b"Z").is_none(),
            "a name that is not there"
        );
        assert!(
            resource(&doc, r, b"Shading", b"A").is_none(),
            "a category that is not there"
        );
        assert!(resource_ref(&doc, None, b"Pattern", b"P").is_none());
        assert!(resource_ref(&doc, r, b"ColorSpace", b"P").is_none());
        assert!(resource_ref(&doc, s, b"ColorSpace", b"P").is_none());
        assert!(resource_ref(&doc, r, b"Shading", b"P").is_none());
        assert!(resource_ref(&doc, r, b"Pattern", b"Z").is_none());
        assert!(
            resource_ref(&doc, r, b"XObject", b"F").is_none(),
            "an entry that refers to nothing"
        );
        assert!(
            resource_ref(&doc, r, b"Pattern", b"P").is_none(),
            "an entry that is not a dictionary"
        );
        let odd = "<< /XObject << /K << /Subtype /Form >> /J 5 0 R /L << /Subtype /Image /ColorSpace /DeviceCMYK >> >> /Pattern << /Q 6 0 R >> >>";
        let more = [
            (5, stream("/Subtype /Image", "")),
            (6, b"<< /PatternType 3 >>".to_vec()),
        ];
        let got = run(
            odd,
            "CS /K Do /J Do /L Do /Pattern cs /Missing scn f /Q scn f BI /W 1 /H 1 /BPC 8 /F /DCT /L 1 ID a EI",
            &more,
        );
        assert_eq!(
            paints(&got),
            [
                vec![use_of(Role::Image, "DeviceCMYK")],
                vec![use_of(Role::Fill, "Pattern")],
                vec![use_of(Role::Fill, "Pattern")]
            ]
        );
        let other = run(
            "<< >>",
            "",
            &[(3, b"<< /Type /Page /Contents 4 0 R /Group << /S /Other >> >>".to_vec())],
        );
        assert_eq!(other, []);
    }
}
