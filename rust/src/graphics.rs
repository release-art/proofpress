//! Tier 7: the page model and its paint events (ISO 32000-2, clause 8).
//!
//! `walk` interprets each page's content with a graphics state and reports what is painted: for every fill, stroke,
//! text run, image, image mask and shading, what kind of paint it is, the colour space each part of it uses and
//! whether that part overprints, and where it lands on the page; and for every transparency group its blend space.
//! Forms and coloured tiling patterns are walked where they are used, with one set of ancestors, so a form that draws
//! itself is a cycle and a gap; a form without `/Resources` uses the page's (7.8.3), with a problem noted.
//!
//! A form starts from the graphics state of the `Do` that paints it, its `/Matrix` added to the CTM (8.10.1); a
//! tiling pattern's cell starts from the default state, and what it paints is not placed on the page.
//!
//! A device colour space with a matching `Default` space in the resources in effect is reported as that default
//! (8.6.5.6), since PDF/X-4 6.4.3.1 judges it so.
//!
//! Where a paint lands is its bounding box in default user space: a path's points (control points included), a text
//! run's advance by its font's ascent and descent, an image's unit square, each through the CTM. A clip (`W`, `W*`)
//! is kept as the bounding box of its path, cut to the clip before it, and narrows what a paint shows but not where
//! it is.
//
// ponytail: clips are bounding boxes, not paths, and text render modes 4 to 7, a form's `/BBox`, soft-mask content,
// Type 3 glyph procedures and annotation appearances are not walked; exact clip geometry comes with the checks that
// need it (the architecture note's "Clipping").

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::common::family::{self, Family};
use crate::common::font::{self, Font};
use crate::common::rect::Rect;
use crate::content::{self, InlineImage, Item, Op};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::Location;
use crate::pipeline::Limits;

/// A 2-D affine matrix as PDF writes it, `[a b c d e f]` (8.3.4).
pub(crate) type Matrix = [f64; 6];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

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

/// A colour as today's print checks read it: its family and components, the spot inks a `Separation` or `DeviceN`
/// colour selects, the families a pattern paints in, and whether it was set through a profile or other non-device
/// space.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fill {
    pub(crate) family: Family,
    pub(crate) value: Vec<f64>,
    pub(crate) spot: Vec<Vec<u8>>,
    pub(crate) pattern: BTreeSet<Family>,
    pub(crate) profiled: bool,
}

impl Fill {
    /// The colour a space of `family` starts with (8.6.8): black, for the device families.
    fn initial(family: Family) -> Self {
        let value = match family {
            Family::Gray => vec![0.0],
            Family::Rgb => vec![0.0; 3],
            Family::Cmyk => vec![0.0, 0.0, 0.0, 1.0],
            Family::Other => Vec::new(),
        };
        Self {
            family,
            value,
            spot: Vec::new(),
            pattern: BTreeSet::new(),
            profiled: false,
        }
    }
}

/// What today's print checks read from a paint.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ink {
    pub(crate) fill: Fill,
    pub(crate) stroke: Fill,
    /// Whether overprint is on: `OP` or `op`, whichever an `ExtGState` last set (the print checks do not tell fills
    /// from strokes here).
    pub(crate) overprint: bool,
    /// A text run's size on the page: its font size by the text rendering matrix's vertical scale; 0 for the rest.
    pub(crate) size: f64,
    /// An image's or shading's own colour family; `None` for the rest, or a shading that is not there.
    pub(crate) space: Option<Family>,
    /// An image's or shading's own `/ColorSpace` entry, as its dictionary writes it.
    pub(crate) own_space: Option<Object>,
    /// Whether it is inside a tiling pattern's cell, which the print checks see only through the pattern's families.
    pub(crate) cell: bool,
    /// An image's or shading's resource name, `(inline)` for an inline image; empty for the rest.
    pub(crate) name: Vec<u8>,
    /// An image `XObject`'s object number.
    pub(crate) object: Option<u32>,
    /// The CTM an image is placed with, which maps its unit square onto the page.
    pub(crate) matrix: Option<Matrix>,
    /// An image's `/Width` and `/Height` in samples, when both are positive numbers.
    pub(crate) pixels: Option<(i64, i64)>,
    /// A text run's font, and how many of its codes it shows and how many of them show `.notdef`.
    pub(crate) font: Option<Rc<Font>>,
    pub(crate) glyphs: usize,
    pub(crate) notdef: usize,
    /// A text run's character codes, in the order shown.
    pub(crate) codes: Vec<u32>,
    /// A text run's characters, when its font tells them (`Font::text`), a space for each kern wider than
    /// `WORD_GAP`.
    pub(crate) text: Option<String>,
    /// A text run's direction, in degrees counter-clockwise from the page's x axis.
    pub(crate) rotation: f64,
    /// A text run's horizontal scaling (`Tz`) as a factor, and its character spacing (`Tc`) in ems of its font size.
    pub(crate) h_scale: f64,
    pub(crate) char_spacing: f64,
    /// The marked-content id in force in the page's own content, which a tagged file's structure tree refers to.
    pub(crate) mcid: Option<i64>,
    /// What page furniture this is, when marked content says it is an artifact (`Header`, `Pagination`...).
    pub(crate) artifact: Option<String>,
    /// A stroked path's line width on the page: the line width by the CTM's scale (the square root of its
    /// determinant).
    pub(crate) stroke_width: Option<f64>,
}

/// What kind of thing a paint is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A path, filled, stroked or both.
    Path { filled: bool, stroked: bool },
    /// A text run in its render mode (9.3.6); modes 3 and 7 paint nothing.
    Text(i64),
    /// An image `XObject`, sampled or a mask.
    Image,
    /// An inline image, sampled or a mask.
    InlineImage,
    /// `sh`: a shading over the whole clip.
    Shading,
}

/// What tier 7 reports.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    /// Something painted, and where: the page, then each form or pattern on the way.
    Paint {
        kind: Kind,
        /// The colour spaces it paints with; none for invisible text or an image with no `/ColorSpace`.
        uses: Vec<Use>,
        /// Where it is, in the page's default user space; `None` for a shading, an empty path, or anything inside a
        /// tiling pattern's cell.
        bbox: Option<Rect>,
        /// `bbox` cut to the clip, `None` when the clip leaves nothing of it; for a shading, the clip itself.
        visible: Option<Rect>,
        ink: Box<Ink>,
        frames: Vec<Location>,
    },
    /// A transparency group, with its `/CS` when it has one.
    Group {
        space: Option<Object>,
        frames: Vec<Location>,
    },
    /// A `Do` whose name gives no `XObject` stream in the resources in effect: nothing is painted there.
    Missing { name: Vec<u8>, frames: Vec<Location> },
    /// Spot inks a page selects, as the print checks read them: a `Separation` or `DeviceN` space set with `cs` or
    /// `CS` (or underlying a pattern space), a shading's or image's own, or one a pattern set with `scn` paints in.
    Spots { names: Vec<Vec<u8>>, frames: Vec<Location> },
    /// Why a page needs a blend space, outside tiling pattern cells: `opacity`, `blend mode` or `soft mask`.
    Transparency {
        reason: &'static str,
        frames: Vec<Location>,
    },
    /// Something that kept part of a page from being walked, or that the walk had to stand in for.
    Problem { problem: Problem, frames: Vec<Location> },
    /// A page's deepest graphics-state nesting: states saved by `q` and, around them, by the forms and tiling patterns
    /// being walked (each saves the state as if by `q`, 8.10.1). Only for a page that uses `q`.
    Nesting {
        explicit: usize,
        implicit: usize,
        frames: Vec<Location>,
    },
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

/// The graphics state tier 7 keeps, text state included.
#[derive(Clone)]
struct State {
    fill: Object,
    stroke: Object,
    fill_pattern: Option<Vec<u8>>,
    /// The fill and stroke colours as the print checks read them.
    ink: Fill,
    ink_stroke: Fill,
    /// Overprint as the print checks read it: `OP` or `op`.
    overprint: bool,
    op_fill: bool,
    op_stroke: bool,
    opm: i64,
    /// The CTM; `None` inside a tiling pattern's cell.
    ctm: Option<Matrix>,
    /// The clip's extent on the page, once one is set; an empty one keeps its corners crossed.
    clip: Option<Rect>,
    font: Rc<Font>,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
    /// `Tz` as a factor.
    scale: f64,
    rise: f64,
    leading: f64,
    text_mode: i64,
    line_width: f64,
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
            ink: Fill::initial(Family::Gray),
            ink_stroke: Fill::initial(Family::Gray),
            overprint: false,
            op_fill: false,
            op_stroke: false,
            opm: 0,
            ctm: Some(IDENTITY),
            clip: None,
            font: Rc::new(Font::default()),
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            scale: 1.0,
            rise: 0.0,
            leading: 0.0,
            text_mode: 0,
            line_width: 1.0,
        }
    }
}

/// What one content's walk keeps outside the graphics state: the text and line matrices (9.4.2), the path being
/// built, and whether a clip waits for the path's end.
struct Cursor {
    text: Matrix,
    line: Matrix,
    path: Vec<(f64, f64)>,
    clip: bool,
}

/// A walk in progress.
struct Walker<'a> {
    doc: &'a Document,
    limits: Limits,
    events: Vec<Event>,
    /// The forms and patterns being walked, by object number.
    ancestors: BTreeSet<u32>,
    /// Fonts read so far, by object number.
    fonts: BTreeMap<u32, Rc<Font>>,
    /// The tiling patterns whose families are being worked out, by object number.
    patterns: BTreeSet<u32>,
    /// Graphics states saved by `q` in the contents being walked, enclosing ones included.
    q_depth: usize,
    /// The page's deepest nesting so far: states saved by `q`, and by the forms and patterns around them.
    deepest: (usize, usize),
    /// Open marked-content sequences, innermost last: each one's MCID (in the page's own content only) and artifact.
    marked: Vec<(Option<i64>, Option<String>)>,
}

/// Every page's events, in page order.
pub(crate) fn walk(doc: &Document) -> Vec<Event> {
    let mut walker = Walker {
        doc,
        limits: Limits::default(),
        events: Vec::new(),
        ancestors: BTreeSet::new(),
        fonts: BTreeMap::new(),
        patterns: BTreeSet::new(),
        q_depth: 0,
        marked: Vec::new(),
        deepest: (0, 0),
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
        walker.deepest = (0, 0);
        walker.content(&data, resources, resources, &frames, State::default());
        let (explicit, implicit) = walker.deepest;
        if explicit > 0 {
            walker.events.push(Event::Nesting {
                explicit,
                implicit,
                frames,
            });
        }
    }
    walker.events
}

/// `m1` then `m2`: the product `m1 × m2` (8.3.4).
fn mul(m1: &Matrix, m2: &Matrix) -> Matrix {
    let [a1, b1, c1, d1, e1, f1] = *m1;
    let [a2, b2, c2, d2, e2, f2] = *m2;
    [
        a1 * a2 + b1 * c2,
        a1 * b2 + b1 * d2,
        c1 * a2 + d1 * c2,
        c1 * b2 + d1 * d2,
        e1 * a2 + f1 * c2 + e2,
        e1 * b2 + f1 * d2 + f2,
    ]
}

fn translation(tx: f64, ty: f64) -> Matrix {
    [1.0, 0.0, 0.0, 1.0, tx, ty]
}

fn real(object: &Object) -> Option<f64> {
    match object.value {
        #[expect(clippy::cast_precision_loss, reason = "operands far below 2^53")]
        Value::Integer(n) => Some(n as f64),
        Value::Real(r) => Some(r),
        _ => None,
    }
}

/// An operand as a number; 0 when it is missing or not a number.
fn number(operand: Option<&Object>) -> f64 {
    operand.and_then(real).unwrap_or(0.0)
}

/// Six numbers as a matrix; the identity when there are not exactly six.
fn matrix(numbers: impl Iterator<Item = Option<f64>>) -> Matrix {
    let numbers: Option<Vec<f64>> = numbers.collect();
    numbers.and_then(|n| <Matrix>::try_from(n).ok()).unwrap_or(IDENTITY)
}

/// The bounding box of `points` through `m`; `None` without a matrix or points.
fn bounds(m: Option<&Matrix>, points: &[(f64, f64)]) -> Option<Rect> {
    let [sx, ry, rx, sy, tx, ty] = *m?;
    let mut mapped = points
        .iter()
        .map(|p| (sx * p.0 + rx * p.1 + tx, ry * p.0 + sy * p.1 + ty));
    let (x, y) = mapped.next()?;
    Some(mapped.fold(
        Rect {
            x0: x,
            y0: y,
            x1: x,
            y1: y,
        },
        |r, (x, y)| Rect {
            x0: r.x0.min(x),
            y0: r.y0.min(y),
            x1: r.x1.max(x),
            y1: r.y1.max(y),
        },
    ))
}

/// `rect` cut to `clip` (none is unbounded); an empty result keeps crossed corners, which stay empty when cut again.
fn intersect(clip: Option<Rect>, rect: Rect) -> Rect {
    clip.map_or(rect, |c| Rect {
        x0: c.x0.max(rect.x0),
        y0: c.y0.max(rect.y0),
        x1: c.x1.min(rect.x1),
        y1: c.y1.min(rect.y1),
    })
}

fn non_empty(rect: Option<Rect>) -> Option<Rect> {
    rect.filter(|r| r.x0 <= r.x1 && r.y0 <= r.y1)
}

/// The unit square an image fills (8.9.4).
const UNIT_SQUARE: [(f64, f64); 4] = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];

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

impl Cursor {
    /// `Td`, `TD` and `T*`: start a new line `tx`, `ty` from the current line's start.
    fn move_line(&mut self, tx: f64, ty: f64) {
        self.line = mul(&translation(tx, ty), &self.line);
        self.text = self.line;
    }

    /// End the path: a pending clip takes its extent, `bbox` when the paint already measured it.
    fn end(&mut self, state: &mut State, bbox: Option<Rect>) {
        if std::mem::take(&mut self.clip)
            && let Some(extent) = bbox.or_else(|| bounds(state.ctm.as_ref(), &self.path))
        {
            state.clip = Some(intersect(state.clip, extent));
        }
        self.path.clear();
    }
}

/// Apply a path construction operator (8.5.2.1); `false` when `op` is not one.
fn path(op: &Op, cursor: &mut Cursor) -> bool {
    let n = |i: usize| number(op.operands.get(i));
    let count = match op.name.as_slice() {
        b"m" | b"l" => 1,
        b"c" => 3,
        b"v" | b"y" => 2,
        b"re" => {
            let (x, y) = (n(0), n(1));
            cursor.path.extend([(x, y), (x + n(2), y + n(3))]);
            return true;
        }
        _ => return false,
    };
    cursor.path.extend((0..count).map(|i| (n(2 * i), n(2 * i + 1))));
    true
}

/// Apply a text state or positioning operator (9.3, 9.4.2), `Tf` aside; `false` when `op` is not one.
fn text_state(op: &Op, state: &mut State, cursor: &mut Cursor) -> bool {
    let n = |i: usize| number(op.operands.get(i));
    match op.name.as_slice() {
        b"BT" => {
            cursor.text = IDENTITY;
            cursor.line = IDENTITY;
        }
        b"Tm" => {
            cursor.line = matrix(op.operands.iter().map(real));
            cursor.text = cursor.line;
        }
        b"Td" => cursor.move_line(n(0), n(1)),
        b"TD" => {
            state.leading = -n(1);
            cursor.move_line(n(0), n(1));
        }
        b"T*" => cursor.move_line(0.0, -state.leading),
        b"TL" => state.leading = n(0),
        b"Tc" => state.char_spacing = n(0),
        b"Tw" => state.word_spacing = n(0),
        b"Tz" => state.scale = n(0) / 100.0,
        b"Ts" => state.rise = n(0),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a render mode is a small integer; a real is truncated"
        )]
        b"Tr" => state.text_mode = n(0) as i64,
        _ => return false,
    }
    true
}

/// Show a text run (9.4.3): where it lands on the page, with the text matrix moved past it.
/// A text run shown: where it lands, its size on the page, how many codes it shows and how many show `.notdef`.
struct Run {
    bbox: Option<Rect>,
    size: f64,
    glyphs: usize,
    notdef: usize,
    codes: Vec<u32>,
    text: Option<String>,
    rotation: f64,
}

/// A kern wider than this, in thousandths of the font size, reads as a word space: TeX and most layout engines space
/// words with kerns, not space glyphs.
const WORD_GAP: f64 = 150.0;

fn show(op: &Op, state: &mut State, cursor: &mut Cursor) -> Run {
    let name = op.name.as_slice();
    if name == b"\"" {
        state.word_spacing = number(op.operands.first());
        state.char_spacing = number(op.operands.get(1));
    }
    if matches!(name, b"'" | b"\"") {
        cursor.move_line(0.0, -state.leading);
    }
    let items: &[Object] = if name == b"TJ" {
        op.operands.first().and_then(Object::array).unwrap_or_default()
    } else {
        op.operands.last().map_or_default(std::slice::from_ref)
    };
    let (font, size, scale) = (&state.font, state.size, state.scale);
    let mut advance = 0.0;
    let (mut glyphs, mut notdef, mut codes) = (0, 0, Vec::new());
    let mut parts: Vec<Option<String>> = Vec::new();
    for item in items {
        if let Value::String(bytes, _) = &item.value {
            let shown = font.codes(bytes);
            parts.push(font.text(&shown));
            for code in shown {
                codes.push(code);
                glyphs += 1;
                notdef += usize::from(font.is_notdef(code));
                advance += (font.width(code) / 1000.0 * size + state.char_spacing) * scale;
                if font.code_bytes == 1 && code == 32 {
                    advance += state.word_spacing * scale;
                }
            }
        } else {
            let kern = number(Some(item));
            advance -= kern / 1000.0 * size * scale;
            if kern < -WORD_GAP {
                parts.push(Some(" ".to_owned()));
            }
        }
    }
    let text = parts
        .iter()
        .any(Option::is_some)
        .then(|| parts.into_iter().flatten().collect());
    let low = font.descent / 1000.0 * size + state.rise;
    let high = font.ascent / 1000.0 * size + state.rise;
    let trm = state.ctm.map(|ctm| mul(&cursor.text, &ctm));
    let bbox = bounds(
        trm.as_ref(),
        &[(0.0, low), (advance, low), (0.0, high), (advance, high)],
    );
    let vertical = trm.map_or(1.0, |m| m[2].hypot(m[3]));
    let rotation = trm.map_or(0.0, |m| m[1].atan2(m[0]).to_degrees());
    cursor.text = mul(&translation(advance, 0.0), &cursor.text);
    Run {
        bbox,
        size: size * vertical,
        glyphs,
        notdef,
        codes,
        text,
        rotation,
    }
}

/// What a text run painted in `state` shows.
fn text_ink(state: &State, run: Run) -> Ink {
    Ink {
        font: Some(Rc::clone(&state.font)),
        glyphs: run.glyphs,
        notdef: run.notdef,
        codes: run.codes,
        text: run.text,
        rotation: run.rotation,
        h_scale: state.scale,
        char_spacing: if state.size == 0.0 {
            0.0
        } else {
            state.char_spacing / state.size
        },
        ..ink(state, run.size, None)
    }
}

/// What the print checks read from a paint made in `state`.
fn ink(state: &State, size: f64, space: Option<Family>) -> Ink {
    Ink {
        fill: state.ink.clone(),
        stroke: state.ink_stroke.clone(),
        overprint: state.overprint,
        size,
        space,
        own_space: None,
        cell: state.ctm.is_none(),
        name: Vec::new(),
        object: None,
        matrix: None,
        pixels: None,
        font: None,
        glyphs: 0,
        notdef: 0,
        codes: Vec::new(),
        text: None,
        rotation: 0.0,
        h_scale: 1.0,
        char_spacing: 0.0,
        mcid: None,
        artifact: None,
        stroke_width: None,
    }
}

/// An image's `/Width` and `/Height`, each truncated to a whole number and positive.
fn pixels(doc: &Document, image: &Dict) -> Option<(i64, i64)> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a sample count is truncated to a whole number"
    )]
    let side = |key: &[u8]| match doc.resolve(image.get(key)?)?.value {
        Value::Integer(n) => Some(n),
        Value::Real(r) => Some(r as i64),
        _ => None,
    };
    Some((side(b"Width")?, side(b"Height")?)).filter(|(w, h)| *w > 0 && *h > 0)
}

/// The colours a paint puts down, as the print checks read them: a stencil mask's fill, a text run's fill (and its
/// stroke in render modes 1, 2, 5 and 6), a path's fill and stroke as it is painted; none for an image or shading.
pub(crate) fn colours<'a>(kind: Kind, uses: &[Use], ink: &'a Ink) -> Vec<&'a Fill> {
    if uses.iter().any(|u| u.role == Role::ImageMask) {
        return vec![&ink.fill];
    }
    match kind {
        Kind::Text(1 | 2 | 5 | 6) => vec![&ink.fill, &ink.stroke],
        Kind::Text(_) => vec![&ink.fill],
        Kind::Path { filled, stroked } => [(filled, &ink.fill), (stroked, &ink.stroke)]
            .into_iter()
            .filter_map(|(on, fill)| on.then_some(fill))
            .collect(),
        _ => Vec::new(),
    }
}

/// Every colour family a paint uses, looking through patterns: an image's or shading's own, else its colours'.
pub(crate) fn families(kind: Kind, uses: &[Use], ink: &Ink) -> BTreeSet<Family> {
    let mask = uses.iter().any(|u| u.role == Role::ImageMask);
    let own = ink.space.filter(|_| !mask);
    let colours = colours(kind, uses, ink).into_iter();
    own.into_iter()
        .chain(colours.flat_map(|c| std::iter::once(c.family).chain(c.pattern.iter().copied())))
        .collect()
}

/// A colour after `sc`, `scn`, `SC` or `SCN` with these operands: new components in a device family, new tints for a
/// spot, else unchanged.
fn with_components(current: &Fill, operands: &[Object]) -> Fill {
    let value = operands.iter().map(|o| number(Some(o))).collect();
    if matches!(current.family, Family::Gray | Family::Rgb | Family::Cmyk) {
        return Fill {
            value,
            profiled: current.profiled,
            ..Fill::initial(current.family)
        };
    }
    if current.spot.is_empty() {
        return current.clone();
    }
    Fill {
        value,
        spot: current.spot.clone(),
        ..Fill::initial(Family::Other)
    }
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

    /// Report a paint of `kind` at `bbox`.
    /// `BMC` and `BDC` open a marked-content sequence (14.6), which may carry an MCID (14.7.4.2, meant only in a
    /// page's own content) or mark an artifact (14.8.2.2) of the kind its `/Subtype` or `/Type` names; `EMC` closes
    /// the innermost.
    fn marked_content(&mut self, op: &Op, resources: Option<&Dict>) {
        if op.name == b"EMC" {
            self.marked.pop();
            return;
        }
        let tag = op.operands.first().and_then(Object::name);
        let properties = op
            .operands
            .get(1)
            .filter(|_| op.name == b"BDC")
            .and_then(|p| match &p.value {
                Value::Name(n) => resource(self.doc, resources, b"Properties", n),
                _ => self.doc.resolve(p),
            });
        let properties = properties.and_then(Object::dict);
        let mcid = properties
            .filter(|_| self.ancestors.is_empty())
            .and_then(|p| p.get(b"MCID"))
            .and_then(|m| self.doc.resolve(m))
            .and_then(Object::integer);
        let artifact = (tag == Some(b"Artifact")).then(|| {
            properties
                .and_then(|p| p.get(b"Subtype").or_else(|| p.get(b"Type")))
                .and_then(|k| self.doc.resolve(k))
                .and_then(Object::name)
                .map_or_else(|| "Artifact".to_owned(), |k| String::from_utf8_lossy(k).into_owned())
        });
        self.marked.push((mcid, artifact));
    }

    fn push(&mut self, kind: Kind, uses: Vec<Use>, bbox: Option<Rect>, state: &State, frames: &[Location], ink: Ink) {
        let mut ink = ink;
        ink.mcid = self.marked.iter().rev().find_map(|(m, _)| *m);
        ink.artifact = self.marked.iter().rev().find_map(|(_, a)| a.clone());
        let visible = if kind == Kind::Shading {
            non_empty(state.clip)
        } else {
            non_empty(bbox.map(|b| intersect(state.clip, b)))
        };
        self.events.push(Event::Paint {
            kind,
            uses,
            bbox,
            visible,
            ink: Box::new(ink),
            frames: frames.to_vec(),
        });
    }

    /// Paint `kind` at `bbox` with the current fill or stroke colour for each of `roles`; a fill with a pattern also
    /// paints what the pattern does.
    #[expect(
        clippy::too_many_arguments,
        reason = "the walk's context, passed down as the other paint methods take it"
    )]
    fn paint(
        &mut self,
        resources: Option<&Dict>,
        kind: Kind,
        roles: &[Role],
        state: &State,
        frames: &[Location],
        page: Option<&Dict>,
        bbox: Option<Rect>,
        ink: Ink,
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
        self.push(kind, uses, bbox, state, frames, ink);
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
                    let cell = State {
                        ctm: None,
                        ..State::default()
                    };
                    self.nested(num, dict, frames, page, cell);
                }
            }
            _ => {}
        }
    }

    /// Walk the form or coloured tiling pattern `num`, whose dictionary is `dict`, from `initial`.
    fn nested(&mut self, num: u32, dict: &Dict, frames: &[Location], page: Option<&Dict>, initial: State) {
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
        self.content(&data, resources, page, &inner, initial);
        self.ancestors.remove(&num);
    }

    /// Interpret one content with `resources` in effect, from the graphics state `initial`.
    /// `q`: the state saved, and the page's deepest nesting updated.
    fn save(&mut self, stack: &mut Vec<State>, state: &State) {
        stack.push(state.clone());
        self.q_depth += 1;
        let (explicit, implicit) = self.deepest;
        if self.q_depth + self.ancestors.len() > explicit + implicit {
            self.deepest = (self.q_depth, self.ancestors.len());
        }
    }

    /// `Q`: the saved state back; an unbalanced `Q` keeps the state but puts the CTM back to where this content began.
    fn restore(&mut self, stack: &mut Vec<State>, state: &mut State, base: Option<Matrix>) {
        match stack.pop() {
            Some(saved) => {
                *state = saved;
                self.q_depth -= 1;
            }
            None => state.ctm = base,
        }
    }

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
        let base = initial.ctm;
        let mut state = initial;
        let mut cursor = Cursor {
            text: IDENTITY,
            line: IDENTITY,
            path: Vec::new(),
            clip: false,
        };
        for item in &content.items {
            let op = match item {
                Item::Image(image) => {
                    self.inline_image(image, resources, &state, frames, page);
                    continue;
                }
                Item::Op(op) => op,
            };
            if self.colour(op, resources, frames, &mut state)
                || text_state(op, &mut state, &mut cursor)
                || path(op, &mut cursor)
            {
                continue;
            }
            let last = op.operands.last();
            match op.name.as_slice() {
                b"q" => self.save(&mut stack, &state),
                b"Q" => self.restore(&mut stack, &mut state, base),
                b"cm" => state.ctm = state.ctm.map(|ctm| mul(&matrix(op.operands.iter().map(real)), &ctm)),
                b"Tf" => self.font(op, resources, &mut state),
                b"w" => state.line_width = number(op.operands.first()),
                b"f" | b"F" | b"f*" | b"S" | b"s" | b"B" | b"B*" | b"b" | b"b*" => {
                    let roles: &[Role] = match op.name.as_slice() {
                        b"f" | b"F" | b"f*" => &[Role::Fill],
                        b"S" | b"s" => &[Role::Stroke],
                        _ => &[Role::Fill, Role::Stroke],
                    };
                    let bbox = bounds(state.ctm.as_ref(), &cursor.path);
                    let kind = Kind::Path {
                        filled: roles.contains(&Role::Fill),
                        stroked: roles.contains(&Role::Stroke),
                    };
                    let scale = state
                        .ctm
                        .map_or(1.0, |[sx, ry, rx, sy, _, _]| (sx * sy - ry * rx).abs().sqrt());
                    let painted = Ink {
                        stroke_width: roles.contains(&Role::Stroke).then_some(state.line_width * scale),
                        ..ink(&state, 0.0, None)
                    };
                    self.paint(resources, kind, roles, &state, frames, page, bbox, painted);
                    cursor.end(&mut state, bbox);
                }
                b"n" => cursor.end(&mut state, None),
                b"BMC" | b"BDC" | b"EMC" => self.marked_content(op, resources),
                b"W" | b"W*" => cursor.clip = true,
                b"Tj" | b"TJ" | b"'" | b"\"" => {
                    let run = show(op, &mut state, &mut cursor);
                    let roles: &[Role] = match state.text_mode {
                        0 | 4 => &[Role::Fill],
                        1 | 5 => &[Role::Stroke],
                        2 | 6 => &[Role::Fill, Role::Stroke],
                        _ => &[],
                    };
                    let (kind, bbox) = (Kind::Text(state.text_mode), run.bbox);
                    self.paint(
                        resources,
                        kind,
                        roles,
                        &state,
                        frames,
                        page,
                        bbox,
                        text_ink(&state, run),
                    );
                }
                b"sh" => {
                    let shading = last
                        .and_then(Object::name)
                        .and_then(|n| resource(self.doc, resources, b"Shading", n))
                        .and_then(Object::dict);
                    let space = shading.and_then(|s| s.get(b"ColorSpace"));
                    let family = shading.map(|_| family::classify(self.doc, space, resources));
                    let inks = shading.map_or_default(|_| family::spot_names(self.doc, space, resources));
                    self.spots(inks, frames);
                    let uses = space.map(|s| self.use_of(resources, Role::Shading, s, &state));
                    let painted = Ink {
                        name: last.and_then(Object::name).map_or_default(<[u8]>::to_vec),
                        own_space: space.cloned(),
                        ..ink(&state, 0.0, family)
                    };
                    self.push(Kind::Shading, uses.into_iter().collect(), None, &state, frames, painted);
                }
                b"Do" => self.xobject(resources, last, &state, frames, page),
                _ => {}
            }
        }
        // States this content saved and never restored end with it.
        self.q_depth -= stack.len();
    }

    /// An inline image: its unit square, in its colour space or, for a mask, the fill colour.
    fn inline_image(
        &mut self,
        image: &InlineImage,
        resources: Option<&Dict>,
        state: &State,
        frames: &[Location],
        page: Option<&Dict>,
    ) {
        let bbox = bounds(state.ctm.as_ref(), &UNIT_SQUARE);
        let mask = matches!(image.dict.get(b"ImageMask").map(|v| &v.value), Some(Value::Bool(true)));
        let space = image
            .dict
            .get(b"ColorSpace")
            .and_then(|cs| self.inline_space(resources, cs));
        let family = family::classify(self.doc, space.as_ref(), resources);
        self.spots(
            family::spot_names(self.doc, image.dict.get(b"ColorSpace"), resources),
            frames,
        );
        let painted = Ink {
            name: b"(inline)".to_vec(),
            own_space: image.dict.get(b"ColorSpace").cloned(),
            matrix: state.ctm,
            pixels: pixels(self.doc, &image.dict),
            ..ink(state, 0.0, Some(family))
        };
        if mask {
            let roles = &[Role::ImageMask];
            self.paint(resources, Kind::InlineImage, roles, state, frames, page, bbox, painted);
        } else {
            let uses = space.map(|s| self.use_of(resources, Role::Image, &s, state));
            self.push(
                Kind::InlineImage,
                uses.into_iter().collect(),
                bbox,
                state,
                frames,
                painted,
            );
        }
    }

    /// Apply a colour or graphics state parameter operator; `false` when `op` is not one.
    fn colour(&mut self, op: &Op, resources: Option<&Dict>, frames: &[Location], state: &mut State) -> bool {
        let last = op.operands.last();
        let stroke = op.name.first().is_some_and(u8::is_ascii_uppercase);
        match op.name.as_slice() {
            b"CS" | b"cs" => {
                let operand = op.operands.first();
                let ink = self.ink_space(operand, resources);
                let doc = self.doc;
                let mut names = Vec::new();
                if family::classify(doc, operand, resources) == Family::Other {
                    names.extend(family::spot_names(doc, operand, resources));
                }
                if let Some(base) = family::pattern_base(doc, operand, resources) {
                    names.extend(family::spot_names(doc, Some(base), resources));
                }
                self.spots(names, frames);
                if stroke {
                    state.stroke = self.space(resources, last).unwrap_or_else(|| state.stroke.clone());
                    state.ink_stroke = ink;
                } else {
                    state.fill = self.space(resources, last).unwrap_or_else(|| state.fill.clone());
                    state.fill_pattern = None;
                    state.ink = ink;
                }
            }
            b"G" | b"g" | b"RG" | b"rg" | b"K" | b"k" => {
                let (name, family, count): (&[u8], Family, usize) = match op.name.as_slice() {
                    b"G" | b"g" => (b"DeviceGray", Family::Gray, 1),
                    b"RG" | b"rg" => (b"DeviceRGB", Family::Rgb, 3),
                    _ => (b"DeviceCMYK", Family::Cmyk, 4),
                };
                let space = Object {
                    value: Value::Name(name.to_vec()),
                    start: 0,
                    end: 0,
                };
                let ink = Fill {
                    value: (0..count).map(|i| number(op.operands.get(i))).collect(),
                    ..Fill::initial(family)
                };
                if stroke {
                    state.stroke = space;
                    state.ink_stroke = ink;
                } else {
                    state.fill = space;
                    state.ink = ink;
                }
            }
            b"sc" | b"scn" | b"SC" | b"SCN" => {
                if op.name == b"scn" {
                    state.fill_pattern = last.and_then(Object::name).map(<[u8]>::to_vec);
                }
                let current = if stroke { &state.ink_stroke } else { &state.ink };
                let ink = match last.and_then(Object::name) {
                    Some(name) => {
                        let pattern = resource_ref(self.doc, resources, b"Pattern", name);
                        let (families, names) = self.pattern_families(pattern, &current.pattern);
                        self.spots(names, frames);
                        Fill {
                            pattern: families,
                            ..Fill::initial(Family::Other)
                        }
                    }
                    None => with_components(current, &op.operands),
                };
                if stroke {
                    state.ink_stroke = ink;
                } else {
                    state.ink = ink;
                }
            }
            b"gs" => {
                if let Some(gs) = last
                    .and_then(Object::name)
                    .and_then(|n| resource(self.doc, resources, b"ExtGState", n))
                    .and_then(Object::dict)
                {
                    self.ext_gstate(gs, state, frames);
                }
            }
            _ => return false,
        }
        true
    }

    /// The colour `cs` or `CS` sets, as the print checks read it: a pattern space's base family, a spot's inks at full
    /// tint, else the family's initial colour, marked when the space is not a device space.
    fn ink_space(&self, space: Option<&Object>, resources: Option<&Dict>) -> Fill {
        let doc = self.doc;
        if let Some(base) = family::pattern_base(doc, space, resources) {
            return Fill {
                pattern: BTreeSet::from([family::classify(doc, Some(base), resources)]),
                ..Fill::initial(Family::Other)
            };
        }
        let found = family::classify(doc, space, resources);
        if found == Family::Other {
            let spot = family::spot_names(doc, space, resources);
            return Fill {
                value: vec![1.0; spot.len()],
                spot,
                ..Fill::initial(Family::Other)
            };
        }
        Fill {
            profiled: !family::is_device(doc, space, resources),
            ..Fill::initial(found)
        }
    }

    /// The families a pattern paints in: a shading pattern its shading's, an uncoloured tiling pattern `base` (the
    /// families of the colour set with it), a coloured one whatever its cell paints, worked out four patterns deep.
    fn pattern_families(
        &self,
        pattern: Option<(Option<u32>, &Dict)>,
        base: &BTreeSet<Family>,
    ) -> (BTreeSet<Family>, Vec<Vec<u8>>) {
        let doc = self.doc;
        let Some((num, dict)) = pattern else {
            return (BTreeSet::new(), Vec::new());
        };
        let integer = |key: &[u8]| dict.get(key).and_then(|v| doc.resolve(v)).and_then(Object::integer);
        match integer(b"PatternType") {
            Some(2) => {
                let shading = dict.get(b"Shading").and_then(|s| doc.resolve(s)).and_then(Object::dict);
                let space = shading.and_then(|s| s.get(b"ColorSpace"));
                let families = shading
                    .map(|_| family::classify(doc, space, None))
                    .into_iter()
                    .collect();
                let names = shading.map_or_default(|_| family::spot_names(doc, space, None));
                (families, names)
            }
            Some(1) if integer(b"PaintType") == Some(2) => (base.clone(), Vec::new()),
            Some(1) => {
                let Some(num) = num.filter(|n| !self.patterns.contains(n) && self.patterns.len() < 4) else {
                    return (BTreeSet::new(), Vec::new());
                };
                let mut inner = Walker {
                    doc,
                    limits: Limits::default(),
                    events: Vec::new(),
                    ancestors: BTreeSet::new(),
                    fonts: BTreeMap::new(),
                    patterns: self.patterns.iter().copied().chain([num]).collect(),
                    q_depth: 0,
                    marked: Vec::new(),
                    deepest: (0, 0),
                };
                let cell = State {
                    ctm: None,
                    ..State::default()
                };
                inner.nested(num, dict, &[], None, cell);
                let mut families_seen = BTreeSet::new();
                let mut names = Vec::new();
                for event in inner.events {
                    match event {
                        Event::Paint { kind, uses, ink, .. } => families_seen.extend(families(kind, &uses, &ink)),
                        Event::Spots { names: found, .. } => names.extend(found),
                        _ => {}
                    }
                }
                (families_seen, names)
            }
            _ => (BTreeSet::new(), Vec::new()),
        }
    }

    /// Report the spot inks `names`, if any.
    fn spots(&mut self, names: Vec<Vec<u8>>, frames: &[Location]) {
        if !names.is_empty() {
            self.events.push(Event::Spots {
                names,
                frames: frames.to_vec(),
            });
        }
    }

    /// Report a reason the page needs a blend space, unless inside a tiling pattern's cell.
    fn transparency(&mut self, reason: &'static str, state: &State, frames: &[Location]) {
        if state.ctm.is_some() {
            self.events.push(Event::Transparency {
                reason,
                frames: frames.to_vec(),
            });
        }
    }

    /// `Tf`: the font, read once per font object, and the size.
    fn font(&mut self, op: &Op, resources: Option<&Dict>, state: &mut State) {
        let doc = self.doc;
        state.size = number(op.operands.get(1));
        let named = op.operands.first().and_then(Object::name);
        state.font = match named.and_then(|n| resource_ref(doc, resources, b"Font", n)) {
            Some((Some(num), dict)) => Rc::clone(self.fonts.entry(num).or_insert_with(|| {
                let mut font = font::load(doc, dict);
                font.object = Some(num);
                Rc::new(font)
            })),
            Some((None, dict)) => Rc::new(font::load(doc, dict)),
            None => Rc::new(Font::default()),
        };
    }

    /// An inline image's colour space: abbreviated names (8.9.7, Table 93) spelled out, in an array too, or a
    /// resource.
    fn inline_space(&self, resources: Option<&Dict>, space: &Object) -> Option<Object> {
        let spelled = |name: &[u8]| -> Option<&'static [u8]> {
            match name {
                b"G" => Some(b"DeviceGray"),
                b"RGB" => Some(b"DeviceRGB"),
                b"CMYK" => Some(b"DeviceCMYK"),
                b"I" => Some(b"Indexed"),
                _ => None,
            }
        };
        let named = |name: &'static [u8]| Object {
            value: Value::Name(name.to_vec()),
            start: 0,
            end: 0,
        };
        match &space.value {
            Value::Name(name) => match spelled(name) {
                Some(full) => Some(named(full)),
                None if matches!(name.as_slice(), b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK") => Some(space.clone()),
                None => resource(self.doc, resources, b"ColorSpace", name).cloned(),
            },
            Value::Array(items) => Some(Object {
                value: Value::Array(
                    items
                        .iter()
                        .map(|item| item.name().and_then(spelled).map_or_else(|| item.clone(), named))
                        .collect(),
                ),
                ..space.clone()
            }),
            _ => Some(space.clone()),
        }
    }

    /// Apply an `ExtGState`'s overprint settings (8.4.5): `OP` sets both unless `op` is given.
    fn ext_gstate(&mut self, gs: &Dict, state: &mut State, frames: &[Location]) {
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
        if gs.get(b"OP").is_some() || gs.get(b"op").is_some() {
            state.overprint = flag(b"OP") == Some(true) || flag(b"op") == Some(true);
        }
        if let Some(mode) = gs
            .get(b"OPM")
            .and_then(|v| self.doc.resolve(v))
            .and_then(Object::integer)
        {
            state.opm = mode;
        }
        if let Some(width) = gs.get(b"LW").and_then(|w| self.doc.resolve(w)) {
            state.line_width = real(width).unwrap_or(0.0);
        }
        let doc = self.doc;
        let get = |key: &[u8]| gs.get(key).and_then(|v| doc.resolve(v));
        // A constant alpha that is not a number reads as 0, as the print checks always have.
        if [b"ca".as_slice(), b"CA"]
            .iter()
            .any(|key| get(key).is_some_and(|a| real(a).unwrap_or(0.0) < 1.0))
        {
            self.transparency("opacity", state, frames);
        }
        let modes = get(b"BM").map(|m| {
            m.array()
                .map_or_else(|| vec![m], |a| a.iter().filter_map(|i| doc.resolve(i)).collect())
        });
        if modes.is_some_and(|m| m.iter().any(|m| !matches!(m.name(), Some(b"Normal" | b"Compatible")))) {
            self.transparency("blend mode", state, frames);
        }
        if get(b"SMask").is_some_and(|m| m.name() != Some(b"None")) {
            self.transparency("soft mask", state, frames);
        }
    }

    /// `Do`: an image paints its unit square in its colour space (or the fill colour, for a mask); a form is walked
    /// from the current state with its `/Matrix`; a name that gives no `XObject` stream is missing (7.8.3).
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
        let found = resources
            .and_then(|r| r.get(b"XObject"))
            .and_then(|x| self.doc.resolve(x))
            .and_then(Object::dict)
            .and_then(|entries| entries.get(name))
            .and_then(|entry| match entry.value {
                Value::Reference(num, _) => Some((num, self.doc.stream_dict(num)?)),
                _ => None,
            });
        let Some((num, dict)) = found else {
            self.events.push(Event::Missing {
                name: name.to_vec(),
                frames: frames.to_vec(),
            });
            return;
        };
        match dict.get(b"Subtype").and_then(Object::name) {
            Some(b"Image") => {
                let bbox = bounds(state.ctm.as_ref(), &UNIT_SQUARE);
                let space = dict.get(b"ColorSpace");
                self.spots(family::spot_names(self.doc, space, None), frames);
                if dict.get(b"SMask").is_some() {
                    self.transparency("soft mask", state, frames);
                }
                let painted = Ink {
                    name: name.to_vec(),
                    object: Some(num),
                    own_space: space.cloned(),
                    matrix: state.ctm,
                    pixels: pixels(self.doc, dict),
                    ..ink(state, 0.0, Some(family::classify(self.doc, space, None)))
                };
                if matches!(
                    dict.get(b"ImageMask")
                        .and_then(|v| self.doc.resolve(v))
                        .map(|v| &v.value),
                    Some(Value::Bool(true))
                ) {
                    let roles = &[Role::ImageMask];
                    self.paint(resources, Kind::Image, roles, state, frames, page, bbox, painted);
                } else {
                    let uses = dict
                        .get(b"ColorSpace")
                        .map(|space| self.use_of(resources, Role::Image, space, state));
                    let mut inner = frames.to_vec();
                    inner.push(Location::Object(num, 0));
                    self.push(Kind::Image, uses.into_iter().collect(), bbox, state, &inner, painted);
                }
            }
            Some(b"Form") => {
                let doc = self.doc;
                let numbers = dict.get(b"Matrix").and_then(|m| doc.resolve(m)).and_then(Object::array);
                let form = numbers.map_or(IDENTITY, |n| matrix(n.iter().map(|i| doc.resolve(i).and_then(real))));
                let inner = State {
                    ctm: state.ctm.map(|ctm| mul(&form, &ctm)),
                    ..state.clone()
                };
                self.nested(num, dict, frames, page, inner);
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
                Event::Paint { uses, .. } if !uses.is_empty() => Some(
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

    #[test]
    fn marked_content_and_text_runs() {
        // MCIDs and artifacts of the page's own content, a property list by name, sequences closed by EMC; inside a
        // form an MCID means nothing.
        let resources = "<< /Properties << /P << /MCID 7 >> >> /XObject << /F 5 0 R >> /Font << /H << /Subtype /Type1 \
            /ToUnicode 6 0 R >> >> >>";
        let data = "/P /P BDC 0 0 1 1 re f EMC /Span << /MCID 3 >> BDC /Artifact << /Subtype /Header >> BDC \
            0 0 1 1 re f EMC /Artifact BMC /Artifact << /Type /Pagination >> BDC 0 0 1 1 re f EMC EMC EMC EMC \
            /Artifact << >> BDC 0 0 1 1 re f EMC /Q << /MCID 4 >> BDC /F Do EMC 0 0 1 1 re f /Span /Nowhere BDC \
            0 0 1 1 re f EMC BT /H 10 Tf 2 Tc 50 Tz [(A) -200 (B) -100 (C)] TJ /H 0 Tf (A) Tj ET";
        let cmap = "beginbfchar <41> <0061> endbfchar";
        let more = [
            (
                5,
                stream(
                    "/Subtype /Form /BBox [0 0 1 1]",
                    "/S << /MCID 9 >> BDC 0 0 1 1 re f EMC",
                ),
            ),
            (6, stream("", cmap)),
        ];
        let marks: Vec<(Option<i64>, Option<String>)> = run(resources, data, &more)
            .into_iter()
            .filter_map(|e| match e {
                Event::Paint { ink, .. } => Some((ink.mcid, ink.artifact.clone())),
                _ => None,
            })
            .collect();
        let artifact = |kind: &str| Some(kind.to_owned());
        assert_eq!(
            marks,
            [
                (Some(7), None),
                (Some(3), artifact("Header")),
                (Some(3), artifact("Pagination")),
                (None, artifact("Artifact")),
                (Some(4), None),
                (None, None),
                (None, None),
                (None, None),
                (None, None),
            ]
        );
        let texts: Vec<(Option<String>, f64, f64, f64)> = run(resources, data, &more)
            .into_iter()
            .filter_map(|e| match e {
                Event::Paint {
                    kind: Kind::Text(_),
                    ink,
                    ..
                } => Some((ink.text.clone(), ink.rotation, ink.h_scale, ink.char_spacing)),
                _ => None,
            })
            .collect();
        // A kern wider than a word gap reads as a space; B and C have no mapping. At size 0 spacing has no ems.
        assert_eq!(
            texts,
            [
                (Some("a \u{FFFD}\u{FFFD}".to_owned()), 0.0, 0.5, 0.2),
                (Some("a".to_owned()), 0.0, 0.5, 0.0)
            ]
        );
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
        // Only the first page's `q`, as its nesting; the page without contents adds nothing.
        assert!(matches!(
            page_without.as_slice(),
            [Event::Nesting {
                explicit: 1,
                implicit: 0,
                ..
            }]
        ));
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
            [vec![use_of(Role::Fill, "Pattern")], vec![use_of(Role::Fill, "Pattern")]],
            "a direct dictionary is no XObject: an XObject is a stream"
        );
        let other = run(
            "<< >>",
            "",
            &[(3, b"<< /Type /Page /Contents 4 0 R /Group << /S /Other >> >>".to_vec())],
        );
        assert_eq!(other, []);
    }

    /// Each paint as its kind, uses count, and bbox and visible box as `[x0, y0, x1, y1]`.
    type Place = (Kind, usize, Option<[f64; 4]>, Option<[f64; 4]>);

    fn places(events: &[Event]) -> Vec<Place> {
        let corners = |r: &Option<Rect>| r.map(|r| [r.x0, r.y0, r.x1, r.y1]);
        events
            .iter()
            .filter_map(|e| match e {
                Event::Paint {
                    kind,
                    uses,
                    bbox,
                    visible,
                    ..
                } => Some((*kind, uses.len(), corners(bbox), corners(visible))),
                _ => None,
            })
            .collect()
    }

    const FILL: Kind = Kind::Path {
        filled: true,
        stroked: false,
    };
    const STROKE: Kind = Kind::Path {
        filled: false,
        stroked: true,
    };

    #[test]
    fn where_paths_images_and_shadings_land() {
        let resources = "<< /XObject << /Fm 6 0 R /Im 7 0 R /Bare 9 0 R /Odd 10 0 R >> \
            /Shading << /Sh << /ColorSpace /DeviceGray >> >> /Pattern << /T 8 0 R >> >>";
        let data = "10 20 m 30 40 l 50 0 60 10 70 5 c 1 1 2 2 v 3 3 4 4 y S \
            q 2 0 0 2 0 0 cm 0 0 5 5 re f Q \
            q 0 0 4 4 re W n 0 0 10 10 re f Q \
            q 0 0 1 1 re W n 5 5 1 1 re W n 0 0 1 1 re f Q \
            W n f 1 2 3 4 5 cm \
            q 2 0 0 2 0 0 cm /Fm Do Q /Odd Do /Im Do /Bare Do \
            BI /W 1 /H 1 /BPC 8 /CS /G ID a EI BI /IM true /W 1 /H 1 ID a EI \
            q 0 0 3 3 re W n /Sh sh Q /Sh sh /Pattern cs /T scn 0 0 1 1 re f /Nope Do 5 Do";
        let more = [
            (
                6,
                stream(
                    "/Subtype /Form /Matrix [1 0 0 1 1000 0] /Resources << >>",
                    "5 0 0 5 0 0 cm Q 0 0 1 1 re f",
                ),
            ),
            (7, stream("/Subtype /Image /ColorSpace /DeviceGray", "")),
            (
                8,
                stream(
                    "/PatternType 1 /PaintType 1 /Resources << >>",
                    "2 0 0 2 0 0 cm 0 0 1 1 re f",
                ),
            ),
            (9, stream("/Subtype /Image", "")),
            (
                10,
                stream("/Subtype /Form /Matrix [1 2] /Resources << >>", "0 0 1 1 re f"),
            ),
        ];
        let got = run(resources, data, &more);
        let unit = Some([0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            places(&got),
            [
                (STROKE, 1, Some([1.0, 0.0, 70.0, 40.0]), Some([1.0, 0.0, 70.0, 40.0])),
                (FILL, 1, Some([0.0, 0.0, 10.0, 10.0]), Some([0.0, 0.0, 10.0, 10.0])),
                (FILL, 1, Some([0.0, 0.0, 10.0, 10.0]), Some([0.0, 0.0, 4.0, 4.0])),
                (FILL, 1, unit, None),
                (FILL, 1, None, None),
                (
                    FILL,
                    1,
                    Some([2000.0, 0.0, 2002.0, 2.0]),
                    Some([2000.0, 0.0, 2002.0, 2.0])
                ),
                (FILL, 1, Some([0.0, 0.0, 1.0, 1.0]), unit),
                (Kind::Image, 1, unit, unit),
                (Kind::Image, 0, unit, unit),
                (Kind::InlineImage, 1, unit, unit),
                (Kind::InlineImage, 1, unit, unit),
                (Kind::Shading, 1, None, Some([0.0, 0.0, 3.0, 3.0])),
                (Kind::Shading, 1, None, None),
                (FILL, 1, None, None),
                (FILL, 1, unit, unit),
            ]
        );
        let missing: Vec<&[u8]> = got
            .iter()
            .filter_map(|e| match e {
                Event::Missing { name, .. } => Some(name.as_slice()),
                _ => None,
            })
            .collect();
        assert_eq!(missing, [b"Nope".as_slice()]);
    }

    #[test]
    fn where_text_lands() {
        let resources = "<< /Font << /F 5 0 R /D << /Subtype /Type1 /FirstChar 33 /Widths [400] >> /B 6 >> >>";
        let data = "BT /F 10 Tf 12 TL 100 100 Td ( !) Tj (!) ' 1 2 (!) \" [(!) -1000 (!)] TJ \
            1 0 0 1 200 200 Tm 0 Tc 0 Tw 50 Tz 3 Ts (!) Tj 0 -5 TD T* 3 Tr (!) Tj 0 Tr 0 Ts 100 Tz \
            /D 10 Tf 0 0 Td (!) Tj /F 10 Tf 500 Tj 5 TJ Tj /Nope 10 Tf (!) Tj /B 10 Tf (!) Tj ET";
        let more = [(
            5,
            b"<< /Subtype /Type1 /FirstChar 32 /Widths [250 600] /FontDescriptor << /Ascent 700 /Descent -200 >> >>"
                .to_vec(),
        )];
        let got: Vec<(Kind, Option<[f64; 4]>)> = places(&run(resources, data, &more))
            .into_iter()
            .map(|(kind, _, bbox, _)| (kind, bbox))
            .collect();
        let text = |x0: f64, y0: f64, x1: f64, y1: f64| (Kind::Text(0), Some([x0, y0, x1, y1]));
        assert_eq!(
            got,
            [
                text(100.0, 98.0, 108.5, 107.0),
                text(100.0, 86.0, 106.0, 95.0),
                text(100.0, 74.0, 108.0, 83.0),
                text(108.0, 74.0, 134.0, 83.0),
                text(200.0, 201.0, 203.0, 210.0),
                (Kind::Text(3), Some([200.0, 191.0, 203.0, 200.0])),
                text(200.0, 188.0, 204.0, 198.0),
                text(199.0, 188.0, 204.0, 197.0),
                text(199.0, 188.0, 199.0, 197.0),
                text(199.0, 188.0, 199.0, 197.0),
                text(199.0, 188.0, 204.0, 198.0),
                text(204.0, 188.0, 209.0, 198.0),
            ]
        );
    }

    /// Each paint outside pattern cells as the fill or stroke it puts down: family, components, spot names, pattern
    /// families, profiled, overprint.
    type Inked = (Family, Vec<f64>, Vec<String>, Vec<Family>, bool, bool);

    fn inked(events: &[Event], stroke: bool) -> Vec<Inked> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Paint { ink, .. } if !ink.cell => {
                    let f = if stroke { &ink.stroke } else { &ink.fill };
                    let spot = f.spot.iter().map(|s| String::from_utf8_lossy(s).into_owned()).collect();
                    Some((
                        f.family,
                        f.value.clone(),
                        spot,
                        f.pattern.iter().copied().collect(),
                        f.profiled,
                        ink.overprint,
                    ))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_colours_print_checks_read() {
        use Family::{Cmyk, Gray, Other, Rgb};
        let resources = "<< /ColorSpace << /I [/ICCBased 20 0 R] /S [/Separation /Gold /DeviceCMYK 21 0 R] /P [/Pattern /DeviceRGB] >> \
            /Pattern << /Sh << /PatternType 2 /Shading << /ColorSpace /DeviceCMYK >> >> /U << /PatternType 1 /PaintType 2 >> \
            /T 22 0 R /X << /PatternType 9 >> /Self 23 0 R /D << /PatternType 1 >> >> \
            /ExtGState << /O << /OP true >> /F << /op false >> /M << /OPM 1 >> >> >>";
        let f = "0 0 1 1 re f";
        let data = format!(
            "/I cs 0.5 0.5 0.5 sc {f} /S cs 0.3 scn {f} /P cs /U scn {f} /Pattern cs /Sh scn {f} /T scn {f} /X scn {f} \
             /Nope scn {f} /Self scn {f} /D scn {f} /DeviceGray cs 0.4 sc {f} /Pattern cs 0.5 sc {f} \
             0.1 0.2 0.3 0.4 K 1 0 0 RG 0.5 G /O gs 0 0 1 1 re S /F gs /M gs {f} /DeviceRGB CS 0.1 0.2 0.3 SC 0 0 1 1 re S"
        );
        let more = [
            (20, stream("/N 3", "abc")),
            (21, b"<< >>".to_vec()),
            (
                22,
                stream(
                    "/PatternType 1 /PaintType 1 /Resources << /ExtGState << /A << /ca 0.5 >> >> /ColorSpace << /S [/Separation /Gold /DeviceCMYK << >>] >> >>",
                    "0 1 0 rg 0 0 1 1 re f BI /W 1 /H 1 /BPC 8 /CS /G ID a EI BI /IM true /W 1 /H 1 ID a EI /A gs /Nope Do /S cs",
                ),
            ),
            (
                23,
                stream(
                    "/PatternType 1 /PaintType 1 /Resources << /Pattern << /Self 23 0 R >> >>",
                    "/Pattern cs /Self scn 0 0 1 1 re f",
                ),
            ),
        ];
        let events = run(resources, &data, &more);
        let plain = |family, value: &[f64]| (family, value.to_vec(), vec![], vec![], false, false);
        let pattern = |families: &[Family]| (Other, vec![], vec![], families.to_vec(), false, false);
        let fills = inked(&events, false);
        assert_eq!(
            fills,
            [
                (Rgb, vec![0.5; 3], vec![], vec![], true, false),
                (Other, vec![0.3], vec!["Gold".to_owned()], vec![], false, false),
                pattern(&[Rgb]),
                pattern(&[Cmyk]),
                pattern(&[Gray, Rgb]),
                pattern(&[]),
                pattern(&[]),
                pattern(&[Other]),
                pattern(&[]),
                plain(Gray, &[0.4]),
                plain(Other, &[]),
                (Other, vec![], vec![], vec![], false, true),
                plain(Other, &[]),
                plain(Other, &[]),
            ]
        );
        let strokes = inked(&events, true);
        assert_eq!(strokes.get(11), Some(&(Gray, vec![0.5], vec![], vec![], false, true)));
        assert_eq!(
            strokes.get(13),
            Some(&(Rgb, vec![0.1, 0.2, 0.3], vec![], vec![], false, false))
        );
    }

    #[test]
    fn inline_spaces_spots_and_soft_masks() {
        let resources = "<< /XObject << /S 5 0 R >> /ColorSpace << /G [/Separation /Gold /DeviceCMYK << >>] >> >>";
        let data = "BI /W 1 /H 1 /BPC 8 /CS [/I /RGB 0 <000000>] ID a EI BI /W 1 /H 1 /BPC 8 /CS [/Indexed /DeviceRGB 0 <000000>] ID a EI \
            BI /W 1 /H 1 /BPC 8 /CS /G ID a EI /S Do";
        let more = [(5, stream("/Subtype /Image /ColorSpace /DeviceGray /SMask 6 0 R", ""))];
        let got = run(resources, data, &more);
        let spaces: Vec<Option<Family>> = got
            .iter()
            .filter_map(|e| match e {
                Event::Paint { ink, .. } => Some(ink.space),
                _ => None,
            })
            .collect();
        assert_eq!(
            spaces,
            [
                Some(Family::Rgb),
                Some(Family::Rgb),
                Some(Family::Gray),
                Some(Family::Gray)
            ]
        );
        assert!(got.iter().any(|e| matches!(
            e,
            Event::Transparency {
                reason: "soft mask",
                ..
            }
        )));
        let doc = Document::open(
            &classic(&[(1, b"<< >>")], "<< /Size SIZE /Root 1 0 R >>"),
            &Limits::default(),
        )
        .unwrap();
        let walker = Walker {
            doc: &doc,
            limits: Limits::default(),
            events: Vec::new(),
            ancestors: BTreeSet::new(),
            fonts: BTreeMap::new(),
            patterns: BTreeSet::new(),
            q_depth: 0,
            marked: Vec::new(),
            deepest: (0, 0),
        };
        let five = Object {
            value: Value::Integer(5),
            start: 0,
            end: 0,
        };
        assert_eq!(
            walker.inline_space(None, &five),
            Some(five),
            "a space that is neither a name nor an array"
        );
    }
}
