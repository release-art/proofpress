//! Every dictionary in the file with where it sits: the key it is the value of (an array's items count as the array's
//! value) and the keys one level up. A dictionary written once and referred to from several places has every place,
//! and the entries of an indirect object take as their parents the keys that object is referred to under, so
//! `/AA 13 0 R` puts the actions in object 13 inside `AA`.
//!
//! The PDF/X prohibitions on keys cover the whole file (X4-Q3), not only what the pages paint, so they judge this list.

use std::collections::BTreeMap;

use crate::document::{Dict, Document, Object, Value};

/// Where a value sits: under `key`, inside the values of `parents`. Both empty at an unreferenced object's top level.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Place<'a> {
    pub(super) key: Option<&'a [u8]>,
    pub(super) parents: Vec<&'a [u8]>,
}

/// One dictionary: the indirect object it is in, whether it is that object's stream dictionary, and its places.
#[derive(Debug)]
pub(super) struct Node<'a> {
    pub(super) num: u32,
    pub(super) dict: &'a Dict,
    pub(super) stream: bool,
    pub(super) places: Vec<Place<'a>>,
}

impl Node<'_> {
    /// Whether the dictionary sits anywhere under `key`.
    pub(super) fn under(&self, key: &[u8]) -> bool {
        self.places.iter().any(|p| p.key == Some(key))
    }

    /// Whether the dictionary sits anywhere inside the value of `key`, one level down.
    pub(super) fn inside(&self, key: &[u8]) -> bool {
        self.places.iter().any(|p| p.parents.contains(&key))
    }

    /// The name under `key`, as written in the dictionary (not resolved).
    pub(super) fn name(&self, key: &[u8]) -> Option<&[u8]> {
        self.dict.get(key).and_then(Object::name)
    }
}

/// What a value sits inside: a key of the enclosing dictionary, or the top level of an indirect object.
#[derive(Clone, Copy)]
enum Parent<'a> {
    Key(Option<&'a [u8]>),
    Top(u32),
}

/// A dictionary or a reference found by the walk, with its key and what it sits inside.
enum Found<'a> {
    Dict {
        num: u32,
        dict: &'a Dict,
        key: Option<&'a [u8]>,
        parent: Parent<'a>,
    },
    Reference {
        target: u32,
        key: Option<&'a [u8]>,
        parent: Parent<'a>,
    },
}

/// Every dictionary in every object the walk at open read.
pub(super) fn dicts(doc: &Document) -> Vec<Node<'_>> {
    let mut found = Vec::new();
    let mut streams = Vec::new();
    for (num, object, stream, _) in doc.objects() {
        if let (Some(dict), Some(_)) = (object.dict(), stream) {
            streams.push(std::ptr::from_ref(dict));
        }
        visit(num, object, None, Parent::Key(None), &mut found);
    }
    // The keys each object is referred to under, then each reference's place.
    let mut keys: BTreeMap<u32, Vec<&[u8]>> = BTreeMap::new();
    for item in &found {
        if let Found::Reference {
            target, key: Some(key), ..
        } = item
        {
            keys.entry(*target).or_default().push(key);
        }
    }
    let mut referred: BTreeMap<u32, Vec<Place<'_>>> = BTreeMap::new();
    for item in &found {
        if let Found::Reference { target, key, parent } = item {
            referred.entry(*target).or_default().push(Place {
                key: *key,
                parents: parents(*parent, &keys),
            });
        }
    }
    found
        .iter()
        .filter_map(|item| match item {
            Found::Dict {
                num,
                dict,
                key: None,
                parent: Parent::Key(None),
            } => Some(Node {
                num: *num,
                dict,
                stream: streams.contains(&std::ptr::from_ref(*dict)),
                places: referred.get(num).cloned().unwrap_or_default(),
            }),
            Found::Dict { num, dict, key, parent } => Some(Node {
                num: *num,
                dict,
                stream: false,
                places: vec![Place {
                    key: *key,
                    parents: parents(*parent, &keys),
                }],
            }),
            Found::Reference { .. } => None,
        })
        .collect()
}

/// The keys a value sits inside: its enclosing key, or the keys its object is referred to under.
fn parents<'a>(parent: Parent<'a>, keys: &BTreeMap<u32, Vec<&'a [u8]>>) -> Vec<&'a [u8]> {
    match parent {
        Parent::Key(key) => key.into_iter().collect(),
        Parent::Top(num) => keys.get(&num).cloned().unwrap_or_default(),
    }
}

fn visit<'a>(num: u32, object: &'a Object, key: Option<&'a [u8]>, parent: Parent<'a>, found: &mut Vec<Found<'a>>) {
    match &object.value {
        Value::Dict(dict) => {
            found.push(Found::Dict { num, dict, key, parent });
            // At an object's top level the entries sit inside whatever the object is referred to under.
            let inner = if key.is_none() && matches!(parent, Parent::Key(None)) {
                Parent::Top(num)
            } else {
                Parent::Key(key)
            };
            for (k, value) in dict.entries() {
                visit(num, value, Some(k.as_slice()), inner, found);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| visit(num, i, key, parent, found)),
        Value::Reference(target, _) => found.push(Found::Reference {
            target: *target,
            key,
            parent,
        }),
        _ => {}
    }
}
