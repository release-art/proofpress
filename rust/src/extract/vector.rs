//! Vector artwork: every painted path on a page with the graphics state it was painted in, and pieces of artwork
//! written out as one-page PDFs of their own.
//!
//! The Python `proof_pdf._internal.vector` groups the shapes into pieces (by touching boxes, with the text labels
//! over them); this reads the shapes as its collector did, and writes a piece from the shapes and text runs it names:
//! each shape's state operators and path, moved so the piece's corner is the origin, on a page cropped to it, with
//! the resources the operators name copied under new names.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value as Json, json};

use crate::common::family::{self, Family};
use crate::common::pyfmt;
use crate::content::{self, Item, Op};
use crate::document::{Dict, Document, Object, Value};
use crate::graphics::{self, Event, Kind, Matrix, Show};
use crate::pipeline::Limits;
use crate::writer::{Writer, name, real};

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
/// How deep forms are followed, as the Python walk followed them.
const MAX_DEPTH: usize = 12;
/// PDF forbids pages under 3 units: a smaller piece sits in the corner of a 3-unit page.
const MIN_PAGE_PT: f64 = 3.0;
const STROKES: [&[u8]; 6] = [b"S", b"s", b"B", b"B*", b"b", b"b*"];
const PAINTS: [&[u8]; 9] = [b"S", b"s", b"f", b"F", b"f*", b"B", b"B*", b"b", b"b*"];
const PATHS: [&[u8]; 7] = [b"m", b"l", b"c", b"v", b"y", b"re", b"h"];
const LINE_STYLE: [&[u8]; 6] = [b"J", b"j", b"M", b"d", b"ri", b"i"];
const DEVICE_SPACES: [&[u8]; 4] = [b"DeviceGray", b"DeviceRGB", b"DeviceCMYK", b"Pattern"];

/// A resource a state operator names, which a piece must hold: which operand names it, its category, its entry.
#[derive(Clone, Debug)]
struct Ref {
    index: usize,
    category: &'static [u8],
    entry: Object,
}

#[derive(Clone, Debug)]
struct StateOp {
    operator: Vec<u8>,
    operands: Vec<Object>,
    reference: Option<Ref>,
    /// Each operand as the content wrote it, a real as Python's `Decimal` reads its text.
    written: Vec<String>,
}

/// The state a shape is painted in, as operators to repeat: colours, and line style by operator (`ExtGState`s by
/// object), the latest last.
#[derive(Clone, Debug)]
struct State {
    fill: Vec<StateOp>,
    stroke: Vec<StateOp>,
    line: Vec<(Vec<u8>, StateOp)>,
    width: f64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            fill: Vec::new(),
            stroke: Vec::new(),
            line: Vec::new(),
            width: 1.0,
        }
    }
}

/// A painted path.
#[derive(Clone, Debug)]
struct Shape {
    path: Vec<(Vec<u8>, Vec<Object>)>,
    ctm: Matrix,
    state: State,
    bbox: [f64; 4],
    mcid: Option<i64>,
}

/// A shape as the Python grouping reads it.
#[derive(Debug, PartialEq, Serialize)]
pub(super) struct ShapeFacts {
    page: u32,
    index: usize,
    /// Each path operator with its operands as numbers (1 for one that is not), the paint operator last.
    path: Vec<(String, Vec<f64>)>,
    ctm: Matrix,
    bbox: [f64; 4],
    mcid: Option<i64>,
    width: f64,
    /// The fill and stroke colour operators in force, as written (`k 0 0 0 0.65`), a spot colour by name.
    fill: String,
    stroke: String,
}

/// Every page's shapes, in paint order.
pub(super) fn shapes(doc: &Document) -> Vec<ShapeFacts> {
    let mut out = Vec::new();
    for (page, page_number) in doc.pages().iter().zip(1..) {
        for (index, shape) in collect(doc, page).into_iter().enumerate() {
            out.push(ShapeFacts {
                page: page_number,
                index,
                path: shape
                    .path
                    .iter()
                    .map(|(op, operands)| {
                        let numbers = operands.iter().map(|o| number(doc, o).unwrap_or(1.0)).collect();
                        (String::from_utf8_lossy(op).into_owned(), numbers)
                    })
                    .collect(),
                ctm: shape.ctm,
                bbox: shape.bbox,
                mcid: shape.mcid,
                width: shape.state.width,
                fill: colour(doc, &shape.state.fill),
                stroke: colour(doc, &shape.state.stroke),
            });
        }
    }
    out
}

fn number(doc: &Document, object: &Object) -> Option<f64> {
    match doc.resolve(object).map(|o| &o.value) {
        #[expect(clippy::cast_precision_loss, reason = "a content operand, far below 2^53")]
        Some(Value::Integer(i)) => Some(*i as f64),
        Some(Value::Real(r)) => Some(*r),
        _ => None,
    }
}

/// An operand as Python's `str()` of it wrote it: a name with its slash, a number as Python writes it.
fn written(doc: &Document, object: &Object, content: &[u8]) -> String {
    match &object.value {
        Value::Name(n) => format!("/{}", String::from_utf8_lossy(n)),
        Value::Integer(i) => i.to_string(),
        Value::Real(r) => content
            .get(object.start..object.end)
            .map_or(pyfmt::repr_float(*r), |raw| decimal(&String::from_utf8_lossy(raw))),
        _ => super::pdf_str(doc, object).unwrap_or_default(),
    }
}

/// A real's text as Python's `Decimal` writes it back: a sign of `+` dropped, a bare leading or trailing point given
/// its zero or dropped (`.5` is `0.5`, `5.` is `5`).
fn decimal(raw: &str) -> String {
    let (sign, digits) = match raw.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", raw.trim_start_matches('+')),
    };
    let digits = if digits.starts_with('.') {
        format!("0{digits}")
    } else {
        digits.to_owned()
    };
    format!("{sign}{}", digits.strip_suffix('.').unwrap_or(&digits))
}

/// The colour operators in force as written, black (`g 0`) when none were set; a spot colour as `spot NAME TINT`.
fn colour(doc: &Document, ops: &[StateOp]) -> String {
    let Some(first) = ops.first() else {
        return "g 0".to_owned();
    };
    if matches!(first.operator.as_slice(), b"cs" | b"CS")
        && let Some(reference) = &first.reference
    {
        let names = family::spot_names(doc, Some(&reference.entry), None);
        if !names.is_empty() {
            let tint = ops
                .get(1)
                .and_then(|o| o.operands.first())
                .and_then(|t| number(doc, t))
                .unwrap_or(1.0);
            let names: Vec<String> = names.iter().map(|n| String::from_utf8_lossy(n).into_owned()).collect();
            return format!("spot {} {}", names.join("+"), pyfmt::g(tint));
        }
    }
    ops.iter()
        .map(|op| {
            format!("{} {}", String::from_utf8_lossy(&op.operator), op.written.join(" "))
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The shapes `page` paints, forms included.
fn collect(doc: &Document, page: &crate::document::Page<'_>) -> Vec<Shape> {
    let resources = page
        .get(b"Resources")
        .and_then(|r| doc.resolve(r))
        .and_then(Object::dict);
    let data: Vec<Vec<u8>> = graphics::stream_numbers(doc, page.get(b"Contents"))
        .iter()
        .filter_map(|&n| doc.stream_data(n))
        .collect();
    let mut collector = Collector {
        doc,
        page_resources: resources,
        shapes: Vec::new(),
        state: State::default(),
        marked: Vec::new(),
    };
    collector.content(&data, resources, IDENTITY, 0, &BTreeSet::new());
    collector.shapes
}

struct Collector<'a> {
    doc: &'a Document,
    page_resources: Option<&'a Dict>,
    shapes: Vec<Shape>,
    state: State,
    /// Open marked-content sequences' MCIDs.
    marked: Vec<Option<i64>>,
}

/// The entry `name` of `category` in `resources`, as written there.
fn entry<'a>(doc: &'a Document, resources: Option<&'a Dict>, category: &[u8], name: &[u8]) -> Option<&'a Object> {
    resources
        .and_then(|r| r.get(category))
        .and_then(|c| doc.resolve(c))
        .and_then(Object::dict)
        .and_then(|d| d.get(name))
}

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

impl<'a> Collector<'a> {
    fn matrix(&self, operands: &[Object]) -> Matrix {
        let values: Option<Vec<f64>> = operands.iter().map(|o| number(self.doc, o)).collect();
        values.and_then(|v| <Matrix>::try_from(v).ok()).unwrap_or(IDENTITY)
    }

    fn content(
        &mut self,
        data: &[Vec<u8>],
        resources: Option<&'a Dict>,
        base: Matrix,
        depth: usize,
        seen: &BTreeSet<u32>,
    ) {
        let doc = self.doc;
        let slices: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
        let named = |n: &[u8]| {
            entry(doc, resources, b"ColorSpace", n)
                .and_then(|e| doc.resolve(e))
                .and_then(|space| graphics::components(doc, space))
        };
        let content = content::read(&slices, &Limits::default(), &named);
        // The buffer content::read reads, whose offsets the operands carry: each stream and a newline.
        let joined: Vec<u8> = data.iter().flat_map(|d| d.iter().copied().chain(*b"\n")).collect();
        let (mut ctm, mut ctms, mut saved) = (base, Vec::new(), Vec::new());
        let mut path: Vec<(Vec<u8>, Vec<Object>)> = Vec::new();
        let mut points: Vec<(f64, f64)> = Vec::new();
        for item in &content.items {
            let Item::Op(op) = item else { continue };
            let name = op.name.as_slice();
            match name {
                b"q" => {
                    ctms.push(ctm);
                    saved.push(self.state.clone());
                }
                b"Q" => {
                    ctm = ctms.pop().unwrap_or(base);
                    if let Some(state) = saved.pop() {
                        self.state = state;
                    }
                }
                b"cm" => ctm = mul(&self.matrix(&op.operands), &ctm),
                _ if PATHS.contains(&name) => {
                    path.push((name.to_vec(), op.operands.clone()));
                    points.extend(self.points(name, &op.operands));
                }
                _ if PAINTS.contains(&name) => {
                    if !points.is_empty() {
                        path.push((name.to_vec(), Vec::new()));
                        let bbox = bbox(&ctm, &points, name, self.state.width);
                        let mcid = self.marked.iter().rev().find_map(|m| *m);
                        self.shapes.push(Shape {
                            path: std::mem::take(&mut path),
                            ctm,
                            state: self.state.clone(),
                            bbox,
                            mcid,
                        });
                    }
                    path.clear();
                    points.clear();
                }
                b"n" => {
                    path.clear();
                    points.clear();
                }
                b"BMC" | b"BDC" | b"EMC" => self.marked_content(op, resources, depth),
                b"Do" => self.form(op, resources, ctm, depth, seen),
                _ => self.update(op, resources, &joined),
            }
        }
    }

    fn points(&self, name: &[u8], operands: &[Object]) -> Vec<(f64, f64)> {
        let values: Vec<f64> = operands.iter().map(|o| number(self.doc, o).unwrap_or(1.0)).collect();
        if name == b"re" {
            return match values.as_slice() {
                [x, y, w, h] => vec![(*x, *y), (x + w, y + h)],
                _ => Vec::new(),
            };
        }
        let count = match name {
            b"m" | b"l" => 1,
            b"c" => 3,
            b"v" | b"y" => 2,
            _ => 0,
        };
        values
            .as_chunks::<2>()
            .0
            .iter()
            .take(count)
            .map(|[x, y]| (*x, *y))
            .collect()
    }

    fn marked_content(&mut self, op: &Op, resources: Option<&Dict>, depth: usize) {
        if op.name == b"EMC" {
            self.marked.pop();
            return;
        }
        let doc = self.doc;
        let properties = op
            .operands
            .get(1)
            .filter(|_| op.name == b"BDC" && depth == 0)
            .and_then(|p| match &p.value {
                Value::Name(n) => entry(doc, resources, b"Properties", n).and_then(|e| doc.resolve(e)),
                _ => doc.resolve(p),
            })
            .and_then(Object::dict);
        self.marked.push(
            properties
                .and_then(|p| p.get(b"MCID"))
                .and_then(|m| doc.resolve(m))
                .and_then(Object::integer),
        );
    }

    /// `Do` on a form: its content in a copy of the state, which is given back after.
    fn form(&mut self, op: &Op, resources: Option<&'a Dict>, ctm: Matrix, depth: usize, seen: &BTreeSet<u32>) {
        let doc = self.doc;
        let Some(entry) = op
            .operands
            .first()
            .and_then(Object::name)
            .and_then(|n| entry(doc, resources, b"XObject", n))
        else {
            return;
        };
        let Value::Reference(num, _) = entry.value else { return };
        let Some(form) = doc
            .stream_dict(num)
            .filter(|f| f.get(b"Subtype").and_then(Object::name) == Some(b"Form"))
        else {
            return;
        };
        if depth >= MAX_DEPTH || seen.contains(&num) {
            return;
        }
        let matrix = form
            .get(b"Matrix")
            .and_then(|m| doc.resolve(m))
            .and_then(Object::array)
            .unwrap_or_default();
        let ctm = mul(&self.matrix(matrix), &ctm);
        // A form without /Resources uses the page's (ISO 32000-2 7.8.3), as every walk in the core does.
        let inner = form
            .get(b"Resources")
            .and_then(|r| doc.resolve(r))
            .and_then(Object::dict)
            .or(self.page_resources);
        let data: Vec<Vec<u8>> = doc.stream_data(num).into_iter().collect();
        let mut seen = seen.clone();
        seen.insert(num);
        let state = self.state.clone();
        self.content(&data, inner, ctm, depth + 1, &seen);
        self.state = state;
    }

    fn reference(&self, op: &Op, resources: Option<&Dict>, category: &'static [u8], index: usize) -> Option<Ref> {
        let entry = op
            .operands
            .get(index)
            .and_then(Object::name)
            .and_then(|name| entry(self.doc, resources, category, name))?;
        Some(Ref {
            index,
            category,
            entry: entry.clone(),
        })
    }

    fn update(&mut self, op: &Op, resources: Option<&Dict>, content: &[u8]) {
        let name = op.name.as_slice();
        let written: Vec<String> = op.operands.iter().map(|o| written(self.doc, o, content)).collect();
        let plain = || StateOp {
            operator: name.to_vec(),
            operands: op.operands.clone(),
            reference: None,
            written: written.clone(),
        };
        match name {
            b"g" | b"rg" | b"k" => self.state.fill = vec![plain()],
            b"G" | b"RG" | b"K" => self.state.stroke = vec![plain()],
            b"cs" | b"CS" => {
                let named = op
                    .operands
                    .first()
                    .and_then(Object::name)
                    .is_some_and(|n| !DEVICE_SPACES.contains(&n));
                let reference = if named {
                    self.reference(op, resources, b"ColorSpace", 0)
                } else {
                    None
                };
                let set = vec![StateOp { reference, ..plain() }];
                if name == b"cs" {
                    self.state.fill = set;
                } else {
                    self.state.stroke = set;
                }
            }
            b"sc" | b"scn" | b"SC" | b"SCN" => {
                let last = op.operands.len().saturating_sub(1);
                let reference = self.reference(op, resources, b"Pattern", last);
                let colour = StateOp { reference, ..plain() };
                let field = if matches!(name, b"sc" | b"scn") {
                    &mut self.state.fill
                } else {
                    &mut self.state.stroke
                };
                field.truncate(usize::from(
                    field
                        .first()
                        .is_some_and(|f| matches!(f.operator.as_slice(), b"cs" | b"CS")),
                ));
                field.push(colour);
            }
            b"w" => {
                self.state.width = op.operands.first().and_then(|w| number(self.doc, w)).unwrap_or(1.0);
                put(&mut self.state.line, b"w".to_vec(), plain());
            }
            _ if LINE_STYLE.contains(&name) => put(&mut self.state.line, name.to_vec(), plain()),
            b"gs" => {
                if let Some(reference) = self.reference(op, resources, b"ExtGState", 0) {
                    let key = match reference.entry.value {
                        Value::Reference(num, _) => format!("gs {num}"),
                        _ => format!("gs {:?}", reference.entry),
                    };
                    put(
                        &mut self.state.line,
                        key.into_bytes(),
                        StateOp {
                            reference: Some(reference),
                            ..plain()
                        },
                    );
                }
            }
            _ => {}
        }
    }
}

/// `line` with `op` set under `key` as the latest: a state set again must take effect after the others.
fn put(line: &mut Vec<(Vec<u8>, StateOp)>, key: Vec<u8>, op: StateOp) {
    line.retain(|(k, _)| *k != key);
    line.push((key, op));
}

/// The page-space box of `points` through `ctm`, widened by half the line width for a stroke.
fn bbox(ctm: &Matrix, points: &[(f64, f64)], paint: &[u8], width: f64) -> [f64; 4] {
    let mapped = points
        .iter()
        .map(|(x, y)| (ctm[0] * x + ctm[2] * y + ctm[4], ctm[1] * x + ctm[3] * y + ctm[5]));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x, y) in mapped {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    let pad = if STROKES.contains(&paint) {
        width / 2.0 * (ctm[0] * ctm[3] - ctm[1] * ctm[2]).abs().sqrt()
    } else {
        0.0
    };
    [x0 - pad, y0 - pad, x1 + pad, y1 + pad]
}

/// A text run to show again: how, and its fill colour's family and components.
type Label = (Option<Show>, Family, Vec<f64>);

/// What a piece holds, as the Python grouping asks for it.
#[derive(Debug, serde::Deserialize)]
pub(super) struct Piece {
    page: u32,
    bbox: [f64; 4],
    /// Shapes by their index on the page.
    shapes: Vec<usize>,
    /// Text runs by their index in the page walk's paints (`extract`'s `paints`).
    labels: Vec<usize>,
    /// An image `XObject`'s object number: the piece is that image alone, filling the box (a thumbnail), and the
    /// page, shapes and labels are not read.
    #[serde(default)]
    image: Option<u32>,
}

/// Each piece written out: its content stream, the resources it names (category and object number, for a digest
/// that does not depend on where the piece sits), and the one-page PDF.
pub(super) fn pieces(doc: &Document, pieces: &[Piece], blobs: &mut Vec<Vec<u8>>) -> Vec<Json> {
    let pages = doc.pages();
    let mut shapes_of: BTreeMap<u32, Vec<Shape>> = BTreeMap::new();
    let shows: Vec<Label> = graphics::walk(doc)
        .into_iter()
        .filter_map(|e| match e {
            Event::Paint { kind, ink, .. }
                if !ink.cell && (matches!(kind, Kind::Text(_)) || (kind == Kind::Image && ink.object.is_some())) =>
            {
                Some((ink.show.map(|s| *s), ink.fill.family, ink.fill.value.clone()))
            }
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for piece in pieces {
        if let Some(num) = piece.image {
            out.push(image(doc, num, piece.bbox, blobs));
            continue;
        }
        let Some(page) = piece.page.checked_sub(1).and_then(|i| pages.get(i as usize)) else {
            out.push(Json::Null);
            continue;
        };
        let shapes = shapes_of.entry(piece.page).or_insert_with(|| collect(doc, page));
        let chosen: Vec<&Shape> = piece.shapes.iter().filter_map(|&i| shapes.get(i)).collect();
        let labels: Vec<_> = piece.labels.iter().filter_map(|&i| shows.get(i)).collect();
        out.push(write(doc, piece.bbox, &chosen, &labels, blobs));
    }
    out
}

fn write(doc: &Document, bbox: [f64; 4], shapes: &[&Shape], labels: &[&Label], blobs: &mut Vec<Vec<u8>>) -> Json {
    let mut piece = PieceWriter {
        writer: Writer::new(doc),
        used: Vec::new(),
        ops: Vec::new(),
        origin: [bbox[0], bbox[1]],
    };
    for shape in shapes {
        piece.shape(shape);
    }
    for label in labels {
        piece.label(label);
    }
    piece.finish(bbox, blobs)
}

/// Image `num` alone, scaled to fill the box `bbox`.
fn image(doc: &Document, num: u32, bbox: [f64; 4], blobs: &mut Vec<Vec<u8>>) -> Json {
    let mut piece = PieceWriter {
        writer: Writer::new(doc),
        used: Vec::new(),
        ops: Vec::new(),
        origin: [bbox[0], bbox[1]],
    };
    let entry = Object {
        value: Value::Reference(num, 0),
        start: 0,
        end: 0,
    };
    let name = piece.named(b"XObject", &entry);
    let size = [bbox[2] - bbox[0], bbox[3] - bbox[1]].map(round4);
    piece
        .ops
        .push(format!("q {} 0 0 {} 0 0 cm {name} Do Q", size[0], size[1]));
    piece.finish(bbox, blobs)
}

/// One piece being written: the new file, the resources its content names (category, key, entry), its operators.
struct PieceWriter<'a> {
    writer: Writer<'a>,
    used: Vec<(&'static [u8], String, Object)>,
    ops: Vec<String>,
    /// The piece's corner, which becomes the origin.
    origin: [f64; 2],
}

/// A number rounded as the Python rounded matrix entries (4 places, ties to even), as PDF syntax.
fn round4(v: f64) -> String {
    num(round(v, 4))
}

/// A number as pikepdf writes one into content: a whole number without a point, any other as its shortest text.
fn num(v: f64) -> String {
    let v = v + 0.0;
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        real(v)
    }
}

impl PieceWriter<'_> {
    /// The new name of resource `entry` of `category`, the same for the same object.
    fn named(&mut self, category: &'static [u8], entry: &Object) -> String {
        let key = match entry.value {
            Value::Reference(num, _) => format!("{num}"),
            _ => format!("{entry:?}"),
        };
        let at = self
            .used
            .iter()
            .position(|(c, k, _)| *c == category && *k == key)
            .unwrap_or_else(|| {
                self.used.push((category, key, entry.clone()));
                self.used.len() - 1
            });
        format!("/R{at}")
    }

    fn value(&mut self, object: &Object) -> String {
        String::from_utf8_lossy(&self.writer.value(object)).into_owned()
    }

    /// An operator with its operands, a resource it names under its new name.
    fn emit(&mut self, op: &StateOp) {
        let operands: Vec<String> = op
            .operands
            .iter()
            .enumerate()
            .map(|(i, o)| match &op.reference {
                Some(r) if r.index == i => self.named(r.category, &r.entry),
                _ => self.value(o),
            })
            .collect();
        let line = format!("{} {}", operands.join(" "), String::from_utf8_lossy(&op.operator));
        self.ops.push(line.trim().to_owned());
    }

    fn shape(&mut self, shape: &Shape) {
        let moved = mul(&shape.ctm, &[1.0, 0.0, 0.0, 1.0, -self.origin[0], -self.origin[1]]);
        self.ops.push("q".to_owned());
        self.ops.push(format!("{} cm", moved.map(round4).join(" ")));
        let state = &shape.state;
        for op in state
            .line
            .iter()
            .map(|(_, op)| op)
            .chain(&state.fill)
            .chain(&state.stroke)
        {
            self.emit(op);
        }
        for (operator, operands) in &shape.path {
            let op = StateOp {
                operator: operator.clone(),
                operands: operands.clone(),
                reference: None,
                written: Vec::new(),
            };
            self.emit(&op);
        }
        self.ops.push("Q".to_owned());
    }

    /// A text run shown again in its font, size, text state and fill colour.
    fn label(&mut self, (show, family, fill): &Label) {
        let Some(show) = show else { return };
        let (Some(font), Some(trm)) = (&show.font, show.trm) else {
            return;
        };
        let moved = [
            trm[0],
            trm[1],
            trm[2],
            trm[3],
            trm[4] - self.origin[0],
            trm[5] - self.origin[1],
        ];
        self.ops.push("q".to_owned());
        let colour = match family {
            Family::Gray => Some("g"),
            Family::Rgb => Some("rg"),
            Family::Cmyk => Some("k"),
            Family::Other => None,
        };
        if let Some(colour) = colour.filter(|_| !fill.is_empty()) {
            self.ops.push(format!(
                "{} {colour}",
                fill.iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ")
            ));
        }
        let font = self.named(b"Font", font);
        self.ops.extend([
            "BT".to_owned(),
            format!("{font} {} Tf", num(show.size)),
            format!("{} Tc", num(show.char_spacing)),
            format!("{} Tw", num(show.word_spacing)),
            format!("{} Tz", num(round(show.h_scale * 100.0, 4))),
            format!("{} Ts", num(show.rise)),
            format!("{} Tr", show.render_mode),
            format!("{} Tm", moved.map(round4).join(" ")),
        ]);
        let operand = show.operand.as_ref().map_or_else(String::new, |o| self.value(o));
        self.ops.push(
            format!("{operand} {}", String::from_utf8_lossy(show.operator))
                .trim()
                .to_owned(),
        );
        self.ops.extend(["ET".to_owned(), "Q".to_owned()]);
    }

    /// The finished one-page file, the page `bbox`'s size; its content and file are added to `blobs`.
    fn finish(mut self, bbox: [f64; 4], blobs: &mut Vec<Vec<u8>>) -> Json {
        let data = self.ops.join("\n").into_bytes();
        let mut resources: BTreeMap<&'static [u8], Vec<String>> = BTreeMap::new();
        let mut keys = Vec::new();
        for (i, (category, key, entry)) in std::mem::take(&mut self.used).into_iter().enumerate() {
            let body = self.value(&entry);
            resources.entry(category).or_default().push(format!("/R{i} {body}"));
            keys.push(json!([format!("/{}", String::from_utf8_lossy(category)), key]));
        }
        let resources: Vec<String> = resources
            .iter()
            .map(|(category, entries)| {
                format!(
                    "{} << {} >>",
                    String::from_utf8_lossy(&name(category)),
                    entries.join(" ")
                )
            })
            .collect();
        let writer = &mut self.writer;
        let (catalog, pages, page, contents) = (writer.reserve(), writer.reserve(), writer.reserve(), writer.reserve());
        writer.set(catalog, format!("<< /Type /Catalog /Pages {pages} 0 R >>").into_bytes());
        writer.set(
            pages,
            format!("<< /Type /Pages /Kids [{page} 0 R] /Count 1 >>").into_bytes(),
        );
        let size = |v: f64| num(v.max(MIN_PAGE_PT));
        let (width, height) = (size(bbox[2] - bbox[0]), size(bbox[3] - bbox[1]));
        let page_dict = format!(
            "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 {width} {height}] /Contents {contents} 0 R \
             /Resources << {} >> >>",
            resources.join(" ")
        );
        writer.set(page, page_dict.into_bytes());
        let mut stream = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
        stream.extend(&data);
        stream.extend(b"\nendstream");
        writer.set(contents, stream);
        blobs.push(data);
        blobs.push(self.writer.finish(catalog));
        json!({"content": blobs.len() - 2, "pdf": blobs.len() - 1, "keys": keys})
    }
}

/// Python's `round(x, places)`: to the nearest, ties to even, on the exact binary value.
pub(super) fn round(x: f64, places: usize) -> f64 {
    format!("{x:.places$}").parse().unwrap_or(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;

    /// A one-page document whose page has `resources` and content `data`, with objects `more`.
    fn doc(resources: &str, data: &str, more: &[(u32, String)]) -> Document {
        let mut objects = vec![
            (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
            (
                3,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources {resources} /Contents 4 0 R >>"
                ),
            ),
            (4, format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len())),
        ];
        objects.extend(more.iter().cloned());
        let refs: Vec<(u32, &[u8])> = objects.iter().map(|(n, b)| (*n, b.as_bytes())).collect();
        Document::open(&classic(&refs, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap()
    }

    fn form(dict: &str, data: &str) -> String {
        format!(
            "<< /Subtype /Form {dict} /Length {} >>\nstream\n{data}\nendstream",
            data.len()
        )
    }

    #[test]
    fn shapes_with_their_state() {
        let resources = "<< /ColorSpace << /Gold [/Separation /Gold /DeviceCMYK 9 0 R] /C [/ICCBased 9 0 R] >> \
            /Pattern << /P 9 0 R >> /ExtGState << /G 9 0 R /D << /LW 2 >> >> /Properties << /M << /MCID 5 >> >> \
            /XObject << /F 10 0 R /Self 11 0 R /Img 12 0 R >> >>";
        let data = "q 2 0 0 2 10 10 cm .5 0 +1.50 5. k 0 0 m 1 1 l 2 0 2 1 3 1 c 1 1 2 2 v 1 1 2 2 y h S Q \
            /Gold cs -.5 scn 0 0 5 5 re f /DeviceRGB CS 1 0 0 SC 0 0 m n /Pattern cs /P scn 0 0 1 1 re f \
            3 w 1 J [2 1] 0 d /G gs /D gs 2 w /G gs /Span /M BDC 0 0 1 1 re B EMC /Span << /MCID 6 >> BDC /F Do EMC \
            Q Q /Self Do /Img Do /Nowhere Do 0 0 m S f 1 0 0 RG 0 g 0 0 m 1 0 l b* /C cs 0.1 sc 0 0 1 1 re f* \
            1 2 3 cm 0 0 1 1 re F 0 0 1 re f BI /W 1 /H 1 /CS /C /BPC 8 ID x EI";
        let more = [
            (9, "<< >>".to_owned()),
            (
                10,
                form(
                    "/Matrix [1 0 0 1 50 50]",
                    "0 G 0 0 m 1 1 l S /Span << /MCID 7 >> BDC 0 0 2 2 re f EMC",
                ),
            ),
            (
                11,
                form("/Resources << /XObject << /Self 11 0 R >> >>", "/Self Do 0 0 3 3 re f"),
            ),
            (
                12,
                "<< /Subtype /Image /Width 1 /Height 1 /Length 1 >>\nstream\nx\nendstream".to_owned(),
            ),
        ];
        let doc = doc(resources, data, &more);
        let shapes = serde_json::to_value(shapes(&doc)).unwrap();
        let summary: Vec<Json> = shapes
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                json!([
                    s["index"],
                    s["path"].as_array().unwrap().last().unwrap()[0],
                    s["fill"],
                    s["stroke"],
                    s["mcid"],
                    s["width"]
                ])
            })
            .collect();
        assert_eq!(
            summary,
            [
                json!([0, "S", "k 0.5 0 1.50 5", "g 0", null, 1.0]),
                json!([1, "f", "spot Gold -0.5", "g 0", null, 1.0]),
                json!([2, "f", "cs /Pattern scn /P", "CS /DeviceRGB SC 1 0 0", null, 1.0]),
                json!([3, "B", "cs /Pattern scn /P", "CS /DeviceRGB SC 1 0 0", 5, 2.0]),
                json!([4, "S", "cs /Pattern scn /P", "G 0", 6, 2.0]),
                json!([5, "f", "cs /Pattern scn /P", "G 0", 6, 2.0]),
                // A Q with nothing saved restores nothing: the state stays as it is.
                json!([6, "f", "cs /Pattern scn /P", "CS /DeviceRGB SC 1 0 0", null, 2.0]),
                json!([7, "S", "cs /Pattern scn /P", "CS /DeviceRGB SC 1 0 0", null, 2.0]),
                json!([8, "b*", "g 0", "RG 1 0 0", null, 2.0]),
                json!([9, "f*", "cs /C sc 0.1", "RG 1 0 0", null, 2.0]),
                json!([10, "F", "cs /C sc 0.1", "RG 1 0 0", null, 2.0]),
            ]
        );
        // The first shape: points through the CTM, widened by half the line width for a stroke.
        assert_eq!(shapes[0]["bbox"], json!([9.0, 9.0, 17.0, 15.0]));
        assert_eq!(shapes[0]["path"][0], json!(["m", [0.0, 0.0]]));
        // In the form: its /Matrix on the CTM; the one that draws itself is entered once.
        assert_eq!(shapes[4]["bbox"], json!([49.0, 49.0, 52.0, 52.0]));
        assert_eq!(shapes[6]["bbox"], json!([0.0, 0.0, 3.0, 3.0]));
        // A rectangle with three operands has no points, so it paints nothing.
        assert_eq!(shapes.as_array().unwrap().len(), 11);
    }

    #[test]
    fn resources_that_are_not_there() {
        // No resources at all, then resources without the categories, then categories without the names: every named
        // state is set without its resource, and a direct or missing XObject draws nothing.
        let data = "/Gold cs 1 scn /P scn /G gs cs 5 cs 0 0 1 1 re f /D Do /Missing Do \
            /Span /Props BDC 0 0 1 1 re f EMC 0 /x m S BI /W 1 /H 1 /CS /Named /BPC 8 ID x EI";
        for resources in [
            "null",
            "<< >>",
            "<< /ColorSpace << >> /Pattern << >> /ExtGState << >> /Properties << >> \
            /XObject << /D << /Subtype /Form >> >> >>",
        ] {
            let page = doc(resources, data, &[]);
            let shapes = serde_json::to_value(shapes(&page)).unwrap();
            assert_eq!(shapes[0]["fill"], json!("cs 5"), "{resources}");
            assert_eq!(shapes[1]["mcid"], json!(null), "{resources}");
            assert_eq!(shapes[2]["path"][0], json!(["m", [0.0, 1.0]]), "{resources}");
        }
    }

    #[test]
    fn written_reals_and_nested_forms() {
        assert_eq!(
            [".5", "-.5", "+1.50", "5.", "-2"].map(decimal),
            ["0.5", "-0.5", "1.50", "5", "-2"]
        );
        // Forms nested deeper than the limit are not entered.
        let more: Vec<(u32, String)> = (0..14)
            .map(|i| {
                (
                    10 + i,
                    form(
                        &format!("/Resources << /XObject << /F {} 0 R >> >>", 11 + i),
                        "/F Do 0 0 1 1 re f",
                    ),
                )
            })
            .collect();
        let deep = doc("<< /XObject << /F 10 0 R >> >>", "/F Do", &more);
        assert_eq!(shapes(&deep).len(), MAX_DEPTH);
        // A form without /Resources reads the page's.
        let form_at_10 = (10, form("", "/Gold cs 1 scn 0 0 1 1 re f"));
        let bare = doc(
            "<< /XObject << /F 10 0 R >> /ColorSpace << /Gold [/Separation /Gold /DeviceCMYK 9 0 R] >> >>",
            "/F Do",
            &[form_at_10, (9, "<< >>".to_owned())],
        );
        assert_eq!(
            serde_json::to_value(shapes(&bare)).unwrap()[0]["fill"],
            json!("spot Gold 1")
        );
    }

    #[test]
    fn pieces_are_written_alone() {
        let resources = "<< /ExtGState << /G 9 0 R >> /Font << /H << /Subtype /Type1 /BaseFont /Helvetica >> /I 8 0 R >> \
            /XObject << /Im 7 0 R >> >>";
        let more_text = "BT 0.5 g /H 9.5 Tf (g) Tj 0 0 0 1 k (k) Tj /Gone 7 Tf (u) Tj ET";
        let data = "/G gs 1 0 0 rg 10 10 20 20 re f BT /H 12 Tf 1 0 0 1 12 12 Tm (Hi) Tj ET BT /I 5 Tf [(A) 5 (B)] TJ \
            ET q 5 0 0 5 0 0 cm /Im Do Q /Gold cs BT /H 9 Tf (x) ' ET /Missing gs";
        let more = [
            (9, "<< /CA 0.5 >>".to_owned()),
            (8, "<< /Subtype /Type1 /BaseFont /Times-Roman >>".to_owned()),
            (
                7,
                "<< /Subtype /Image /Width 1 /Height 1 /Length 1 >>\nstream\nx\nendstream".to_owned(),
            ),
        ];
        let doc = doc(resources, &format!("{data} {more_text}"), &more);
        let request = [
            Piece {
                page: 1,
                bbox: [10.0, 10.0, 30.0, 30.0],
                shapes: vec![0, 9],
                labels: vec![0, 1, 2, 3, 4, 5, 6, 9],
                image: None,
            },
            Piece {
                page: 1,
                bbox: [0.0, 0.0, 1.0, 1.0],
                shapes: Vec::new(),
                labels: Vec::new(),
                image: None,
            },
            Piece {
                page: 2,
                bbox: [0.0; 4],
                shapes: Vec::new(),
                labels: Vec::new(),
                image: None,
            },
            // An image alone, for a thumbnail: the page is not read.
            Piece {
                page: 0,
                bbox: [0.0, 0.0, 40.0, 20.0],
                shapes: Vec::new(),
                labels: Vec::new(),
                image: Some(7),
            },
        ];
        let mut blobs = Vec::new();
        let written = pieces(&doc, &request, &mut blobs);
        assert_eq!(written[2], Json::Null);
        assert_eq!(written[3]["keys"], json!([["/XObject", "7"]]));
        assert_eq!(blobs[4], b"q 40 0 0 20 0 0 cm /R0 Do Q");
        let keys = written[0]["keys"].as_array().unwrap();
        assert_eq!(
            (keys.len(), &keys[0], &keys[2]),
            (3, &json!(["/ExtGState", "9"]), &json!(["/Font", "8"]))
        );
        let content = String::from_utf8(blobs[0].clone()).unwrap();
        assert!(
            content.starts_with("q\n1 0 0 1 -10 -10 cm\n/R0 gs\n1 0 0 rg\n10 10 20 20 re\nf\nQ"),
            "{content}"
        );
        assert!(content.contains("1 0 0 rg\nBT\n/R1 12 Tf"), "{content}");
        assert!(content.contains("[<41> 5 <42>] TJ"), "{content}");
        assert!(content.contains("0.5 g\nBT\n/R1 9.5 Tf"), "{content}");
        assert!(content.contains("0 0 0 1 k\nBT"), "{content}");
        assert!(
            !content.contains("<75> Tj"),
            "a run in a font that is not there is left out: {content}"
        );
        // The piece reads back as a one-page PDF of the box's size, its resources under their new names; a box
        // smaller than 3 points gets a 3-point page.
        let piece = Document::open(&blobs[1], &Limits::default()).unwrap();
        let page = &piece.pages()[0];
        assert_eq!(
            crate::common::rect::rect(&piece, page.get(b"MediaBox").unwrap()).map(|r| r.x1),
            Some(20.0)
        );
        let small = Document::open(&blobs[3], &Limits::default()).unwrap();
        // The binding's way in: JSON requests, the file read once.
        let file = classic(
            &[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
                (2, b"<< /Type /Pages /Kids [] /Count 0 >>"),
            ],
            "<< /Size SIZE /Root 1 0 R >>",
        );
        assert_eq!(
            super::super::vector_pieces(&file, "[]"),
            Ok(("[]".to_owned(), Vec::new()))
        );
        assert!(matches!(
            super::super::vector_pieces(&file, "{"),
            Err(super::super::PiecesError::Request(_))
        ));
        assert!(matches!(
            super::super::vector_pieces(b"x", "[]"),
            Err(super::super::PiecesError::Unreadable(_))
        ));
        assert_eq!(
            crate::common::rect::rect(&small, small.pages()[0].get(b"MediaBox").unwrap()).map(|r| r.x1),
            Some(3.0)
        );
    }
}
