//! The pipeline around each tier: event identity, and the order in which listeners hear an event.

use std::hash::{Hash, Hasher};

use serde::{Serialize, Serializer};

/// The identity of one event.
///
/// Identity is the document and the sequence number. `tier` and `synthetic` only describe the event, so two ids that
/// name the same event are equal whatever they say.
#[derive(Clone, Copy, Debug)]
pub struct EventId {
    /// Index into the pipeline's table of documents, which holds the name.
    pub doc: u32,
    /// Global for the document, in parse order.
    pub seq: u64,
    /// The tier that emitted the event, for reading logs.
    pub tier: u8,
    /// Inserted by a proxy, never found in the file.
    pub synthetic: bool,
}

impl PartialEq for EventId {
    fn eq(&self, other: &Self) -> bool {
        (self.doc, self.seq) == (other.doc, other.seq)
    }
}

impl Eq for EventId {}

impl Hash for EventId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.doc, self.seq).hash(state);
    }
}

impl Serialize for EventId {
    /// As its identity, `{"doc": .., "seq": ..}`: a saved report names the event, not how it was logged.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Identity {
            doc: u32,
            seq: u64,
        }
        Identity {
            doc: self.doc,
            seq: self.seq,
        }
        .serialize(serializer)
    }
}

/// Something that hears a tier's events. Listeners are called synchronously and may keep state.
pub trait Listener<E> {
    /// Hear one event.
    fn on_event(&mut self, event: &E);
}

/// Publish one event: every first-order observer hears it before any second-order reader, so an observer's report
/// of an event always comes before anything built from it.
pub fn publish<E>(event: &E, observers: &mut [&mut dyn Listener<E>], readers: &mut [&mut dyn Listener<E>]) {
    for listener in observers.iter_mut() {
        listener.on_event(event);
    }
    for listener in readers.iter_mut() {
        listener.on_event(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne};
    use proptest::prelude::*;
    use std::collections::HashSet;

    fn id(doc: u32, seq: u64, tier: u8, synthetic: bool) -> EventId {
        EventId {
            doc,
            seq,
            tier,
            synthetic,
        }
    }

    #[test]
    fn identity_ignores_tier_and_synthetic() {
        assert_eq!(id(1, 7, 2, false), id(1, 7, 5, true));
        assert_ne!(id(1, 7, 2, false), id(1, 8, 2, false));
        assert_ne!(id(1, 7, 2, false), id(2, 7, 2, false));
    }

    #[test]
    fn equal_ids_hash_alike() {
        let set: HashSet<EventId> = [id(1, 7, 2, false), id(1, 7, 6, true)].into_iter().collect();
        assert_eq!(set.len(), 1);
    }

    proptest! {
        #[test]
        fn equal_exactly_when_doc_and_seq_are(a in any::<(u32, u64, u8, bool)>(), b in any::<(u32, u64, u8, bool)>()) {
            let (x, y) = (id(a.0, a.1, a.2, a.3), id(b.0, b.1, b.2, b.3));
            prop_assert_eq!(x == y, (a.0, a.1) == (b.0, b.1));
        }
    }
}
