//! Values built from tier 2's events: the reader that tiers 4 and 5 share.
//!
//! Every value knows its span in the buffer. Integers and reals stay distinct. A dictionary keeps its keys in the order
//! they first appeared, compared after `#xx` decoding (7.3.5); a key given twice keeps the last value, and the values it
//! beat move into `overridden` layers, so nothing in the file is lost (see the architecture note, "Duplicate keys").

use std::collections::HashMap;

use super::lexer::{Defect, hex_bytes, literal_bytes, name_bytes};
use super::{Damage, Event, Form, Located, Parser, Reason};

/// A value and its span, `start..end`, in the buffer it was read from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Object {
    pub(crate) value: Value,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// What a value is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    /// A real; its text is the object's span.
    Real(f64),
    /// A name's decoded bytes.
    Name(Vec<u8>),
    /// A string's decoded bytes and the form it was written in.
    String(Vec<u8>, Form),
    Array(Vec<Object>),
    Dict(Dict),
    Reference(u32, u16),
}

/// Entries from which a dictionary keeps an index of its keys: below it a scan is as quick, and the index costs memory.
const INDEXED: usize = 16;

/// A dictionary in key order, with the layers of values that later duplicates overrode.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Dict {
    entries: Vec<(Vec<u8>, Object)>,
    /// Layer 0 holds what the winning values overrode, layer 1 what those overrode, and so on.
    pub(crate) overridden: Vec<Vec<(Vec<u8>, Object)>>,
    /// Each key's first position in `entries`, once there are [`INDEXED`] of them; empty below that. Derived from
    /// `entries` alone, which only the methods here change, so it stays in step and equal entries mean equal indexes.
    index: HashMap<Vec<u8>, usize>,
}

impl Dict {
    /// A dictionary of `entries`, in their order.
    pub(crate) fn from_entries(entries: Vec<(Vec<u8>, Object)>) -> Self {
        let mut dict = Self {
            entries,
            ..Self::default()
        };
        dict.reindex();
        dict
    }

    /// The entries in key order.
    pub(crate) fn entries(&self) -> &[(Vec<u8>, Object)] {
        &self.entries
    }

    /// The value under `key`, by its decoded bytes.
    pub(crate) fn get(&self, key: &[u8]) -> Option<&Object> {
        self.position(key).and_then(|i| self.entries.get(i)).map(|(_, v)| v)
    }

    /// Append `key` and `value` as they are, even when `key` is already there (the first one stays the one found).
    pub(crate) fn push(&mut self, key: Vec<u8>, value: Object) {
        let at = self.entries.len();
        if at + 1 == INDEXED {
            self.entries.push((key, value));
            self.reindex();
            return;
        }
        if at >= INDEXED {
            self.index.entry(key.clone()).or_insert(at);
        }
        self.entries.push((key, value));
    }

    /// Keep only the entries `keep` accepts.
    pub(crate) fn retain(&mut self, keep: impl FnMut(&(Vec<u8>, Object)) -> bool) {
        self.entries.retain(keep);
        self.reindex();
    }

    fn position(&self, key: &[u8]) -> Option<usize> {
        if self.entries.len() >= INDEXED {
            self.index.get(key).copied()
        } else {
            self.entries.iter().position(|(k, _)| k == key)
        }
    }

    fn reindex(&mut self) {
        self.index.clear();
        if self.entries.len() >= INDEXED {
            for (i, (key, _)) in self.entries.iter().enumerate() {
                self.index.entry(key.clone()).or_insert(i);
            }
        }
    }

    fn insert(&mut self, key: Vec<u8>, value: Object) -> bool {
        let Some(slot) = self.position(&key).and_then(|i| self.entries.get_mut(i)) else {
            self.push(key, value);
            return false;
        };
        let mut beaten = std::mem::replace(&mut slot.1, value);
        for layer in &mut self.overridden {
            if let Some(older) = layer.iter_mut().find(|(k, _)| *k == key) {
                beaten = std::mem::replace(&mut older.1, beaten);
            } else {
                layer.push((key, beaten));
                return true;
            }
        }
        self.overridden.push(vec![(key, beaten)]);
        true
    }
}

impl Object {
    pub(crate) fn integer(&self) -> Option<i64> {
        match self.value {
            Value::Integer(i) => Some(i),
            _ => None,
        }
    }

    pub(crate) fn name(&self) -> Option<&[u8]> {
        match &self.value {
            Value::Name(n) => Some(n),
            _ => None,
        }
    }

    pub(crate) fn dict(&self) -> Option<&Dict> {
        match &self.value {
            Value::Dict(d) => Some(d),
            _ => None,
        }
    }

    pub(crate) fn array(&self) -> Option<&[Object]> {
        match &self.value {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
}

/// Something irregular about a value that did not stop reading it, and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// A byte-level defect at this offset.
    Defect(usize, Defect),
    /// A dictionary key given again; the offset is the repeated key's.
    DuplicateKey(usize),
    /// Stream data not followed by an end of line before `endstream`, which 7.3.8.1 recommends; the offset is the data's end.
    NoEolBeforeEndstream(usize),
}

/// The outcome of reading one value.
#[derive(Debug, PartialEq)]
pub(crate) enum Read {
    /// A complete value.
    Value(Object),
    /// A top-level keyword or operator, at `start..end`, where a value would start.
    Keyword { word: Vec<u8>, start: usize, end: usize },
    /// The end of the input.
    End,
}

/// An open container: where it started, what it holds so far, and, for a dictionary, the key waiting for its value.
enum Frame {
    Array(usize, Vec<Object>),
    Dict(usize, Dict, Option<(Vec<u8>, usize)>),
}

/// Read one complete value from `parser`, noting defects into `notes`.
pub(crate) fn read(parser: &mut Parser<'_>, notes: &mut Vec<Note>) -> Result<Read, Damage> {
    read_from(&mut || parser.next(), notes)
}

/// [`read`] from any source of events. The parser already refuses malformed sequences; another source gets the same
/// refusals here, so this reader holds no state it cannot explain.
fn read_from<'a>(
    next: &mut dyn FnMut() -> Result<Option<Located<'a>>, Damage>,
    notes: &mut Vec<Note>,
) -> Result<Read, Damage> {
    let mut stack: Vec<Frame> = Vec::new();
    loop {
        let Some(located) = next()? else {
            return match stack.last() {
                None => Ok(Read::End),
                Some(_) => Err(Damage {
                    at: usize::MAX,
                    reason: Reason::Unbalanced,
                }),
            };
        };
        if let Some(defect) = located.defect {
            notes.push(Note::Defect(located.start, defect));
        }
        let (start, end) = (located.start, located.end);
        let damage = |reason| Err(Damage { at: start, reason });
        let here = |value| Object { value, start, end };
        let value = match located.event {
            Event::Keyword(word) | Event::Operator(word) if stack.is_empty() => {
                return Ok(Read::Keyword {
                    word: word.to_vec(),
                    start,
                    end,
                });
            }
            Event::Keyword(_) | Event::Operator(_) => return damage(Reason::MisplacedKeyword),
            Event::ArrayStart => {
                stack.push(Frame::Array(start, Vec::new()));
                continue;
            }
            Event::DictStart => {
                stack.push(Frame::Dict(start, Dict::default(), None));
                continue;
            }
            Event::ArrayEnd => match stack.pop() {
                Some(Frame::Array(at, items)) => Object {
                    value: Value::Array(items),
                    start: at,
                    end,
                },
                _ => return damage(Reason::Unbalanced),
            },
            Event::DictEnd => match stack.pop() {
                Some(Frame::Dict(at, dict, None)) => Object {
                    value: Value::Dict(dict),
                    start: at,
                    end,
                },
                Some(Frame::Dict(_, _, Some(_))) => return damage(Reason::BadKey),
                _ => return damage(Reason::Unbalanced),
            },
            Event::Null => here(Value::Null),
            Event::Bool(b) => here(Value::Bool(b)),
            Event::Integer(i) => here(Value::Integer(i)),
            Event::Real(r) => here(Value::Real(r)),
            Event::Name(raw) => here(Value::Name(name_bytes(raw))),
            Event::String(form, raw) => {
                let bytes = if form == Form::Literal {
                    literal_bytes(raw)
                } else {
                    hex_bytes(raw)
                };
                here(Value::String(bytes, form))
            }
            Event::Reference(num, generation) => here(Value::Reference(num, generation)),
        };
        match stack.last_mut() {
            None => return Ok(Read::Value(value)),
            Some(Frame::Array(_, items)) => items.push(value),
            Some(Frame::Dict(_, dict, pending)) => match (pending.take(), value.name()) {
                (None, Some(key)) => *pending = Some((key.to_vec(), start)),
                (None, None) => return damage(Reason::BadKey),
                (Some((key, at)), _) => {
                    if dict.insert(key, value) {
                        notes.push(Note::DuplicateKey(at));
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Mode;

    fn one(buf: &[u8]) -> (Result<Read, Damage>, Vec<Note>) {
        let mut parser = Parser::new(buf, 0, Mode::Objects, 8);
        let mut notes = Vec::new();
        (read(&mut parser, &mut notes), notes)
    }

    fn as_value(read: Read) -> Option<Object> {
        match read {
            Read::Value(v) => Some(v),
            _ => None,
        }
    }

    fn value(buf: &[u8]) -> Object {
        as_value(one(buf).0.unwrap()).unwrap()
    }

    #[test]
    fn values_and_spans() {
        let v = value(b"  << /A [1 2.5 (x) <41> /N#20m true null 3 0 R] /B << >> >> rest");
        assert_eq!((v.start, v.end), (2, 59));
        let dict = v.dict().unwrap();
        let a = dict.get(b"A").unwrap().array().unwrap();
        assert_eq!(a[0].integer(), Some(1));
        assert_eq!(a[1].value, Value::Real(2.5));
        assert_eq!(a[2].value, Value::String(b"x".to_vec(), Form::Literal));
        assert_eq!(a[3].value, Value::String(b"A".to_vec(), Form::Hex));
        assert_eq!(a[4].name(), Some(&b"N m"[..]));
        assert_eq!(
            (a[5].value.clone(), a[6].value.clone(), a[7].value.clone()),
            (Value::Bool(true), Value::Null, Value::Reference(3, 0))
        );
        assert_eq!((a[0].start, a[0].end), (9, 10));
        assert_eq!(dict.get(b"B").unwrap().dict(), Some(&Dict::default()));
        assert_eq!(dict.get(b"C"), None);
        assert_eq!(
            (v.integer(), v.name(), v.array(), a[0].dict()),
            (None, None, None, None)
        );
    }

    #[test]
    fn duplicate_keys_keep_every_value() {
        let (read, notes) = one(b"<< /K1 1 /K2 2 /K3 3 /K#31 4 /K2 5 /K1 6 >>");
        let v = as_value(read.unwrap()).unwrap();
        let d = v.dict().unwrap();
        let ints = |layer: &[(Vec<u8>, Object)]| {
            layer
                .iter()
                .map(|(k, v)| (k.clone(), v.integer().unwrap()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ints(&d.entries),
            [(b"K1".to_vec(), 6), (b"K2".to_vec(), 5), (b"K3".to_vec(), 3)]
        );
        assert_eq!(ints(&d.overridden[0]), [(b"K1".to_vec(), 4), (b"K2".to_vec(), 2)]);
        assert_eq!(ints(&d.overridden[1]), [(b"K1".to_vec(), 1)]);
        assert_eq!(
            notes,
            [Note::DuplicateKey(21), Note::DuplicateKey(29), Note::DuplicateKey(35)]
        );
    }

    #[test]
    fn a_large_dictionary_is_found_through_its_index() {
        let mut text = b"<<".to_vec();
        for i in 0..40 {
            text.extend(format!(" /K{i} {i}").bytes());
        }
        text.extend(b" /K3 99 >>");
        let (read, notes) = one(&text);
        let d = as_value(read.unwrap()).unwrap().dict().unwrap().clone();
        assert_eq!(notes.len(), 1, "the duplicate is still seen");
        assert_eq!(d.get(b"K3").and_then(Object::integer), Some(99));
        assert_eq!(d.get(b"K39").and_then(Object::integer), Some(39));
        assert_eq!(d.get(b"K40"), None);
        assert_eq!(d.index.len(), 40);

        let int = |i| Object {
            value: Value::Integer(i),
            start: 0,
            end: 0,
        };
        let mut pushed = Dict::default();
        for i in 0..20 {
            pushed.push(format!("K{}", i % 18).into_bytes(), int(i));
        }
        assert_eq!(
            pushed.get(b"K1").and_then(Object::integer),
            Some(1),
            "the first of a pushed duplicate"
        );
        assert_eq!(pushed.get(b"K17").and_then(Object::integer), Some(17));
        assert_eq!(
            pushed,
            Dict::from_entries(pushed.entries().to_vec()),
            "built either way, the same"
        );

        pushed.retain(|(k, _)| k.len() == 2);
        assert_eq!(
            (pushed.entries().len(), pushed.index.len()),
            (12, 0),
            "small again: no index"
        );
        assert_eq!(pushed.get(b"K9").and_then(Object::integer), Some(9));
        assert_eq!(pushed.get(b"K10"), None);
    }

    #[test]
    fn defects_and_keywords_and_the_end() {
        let (_, notes) = one(b"[<414> /A#00]");
        assert_eq!(
            notes,
            [
                Note::Defect(1, Defect::OddHexDigits),
                Note::Defect(7, Defect::BadNameEscape)
            ]
        );
        assert_eq!(
            one(b" obj").0,
            Ok(Read::Keyword {
                word: b"obj".to_vec(),
                start: 1,
                end: 4
            })
        );
        assert_eq!(one(b"  ").0, Ok(Read::End));
        assert_eq!(as_value(Read::End), None);
        assert_eq!(one(b"[1").0.map_err(|d| d.reason), Err(Reason::Unbalanced));
    }

    /// Reads from a scripted list of events, as a source other than the parser might produce them.
    fn scripted(events: &[Event<'static>]) -> Result<Read, Damage> {
        let mut events = events.iter().enumerate();
        let mut next = || {
            Ok(events.next().map(|(i, &event)| Located {
                event,
                start: i,
                end: i + 1,
                defect: None,
            }))
        };
        read_from(&mut next, &mut Vec::new())
    }

    #[test]
    fn a_malformed_source_is_damage_not_a_panic() {
        let reason = |events: &[Event<'static>]| scripted(events).map_err(|d| d.reason);
        assert_eq!(reason(&[Event::ArrayStart]), Err(Reason::Unbalanced));
        assert_eq!(reason(&[Event::ArrayEnd]), Err(Reason::Unbalanced));
        assert_eq!(reason(&[Event::ArrayStart, Event::DictEnd]), Err(Reason::Unbalanced));
        assert_eq!(
            reason(&[Event::DictStart, Event::Name(b"A"), Event::DictEnd]),
            Err(Reason::BadKey)
        );
        assert_eq!(reason(&[Event::DictStart, Event::Integer(1)]), Err(Reason::BadKey));
        assert_eq!(
            reason(&[Event::ArrayStart, Event::Operator(b"q")]),
            Err(Reason::MisplacedKeyword)
        );
        assert_eq!(
            reason(&[Event::Operator(b"q")]),
            Ok(Read::Keyword {
                word: b"q".to_vec(),
                start: 0,
                end: 1
            })
        );
    }
}
