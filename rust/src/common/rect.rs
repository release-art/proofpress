//! Rectangles (ISO 32000-2, 7.9.5): four numbers giving two opposite corners, in either order.

use crate::document::{Document, Object, Value};

/// A rectangle with its corners ordered: `x0 <= x1`, `y0 <= y1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x0: f64,
    pub(crate) y0: f64,
    pub(crate) x1: f64,
    pub(crate) y1: f64,
}

impl Rect {
    /// Whether `self` does not extend beyond `outer`; touching edges are inside.
    pub(crate) fn within(&self, outer: &Self) -> bool {
        self.x0 >= outer.x0 && self.y0 >= outer.y0 && self.x1 <= outer.x1 && self.y1 <= outer.y1
    }
}

/// `object` as a rectangle, when it is (or refers to) an array of four numbers.
pub(crate) fn rect(doc: &Document, object: &Object) -> Option<Rect> {
    let items = doc.resolve(object)?.array()?;
    let numbers: Vec<f64> = items
        .iter()
        .map(|i| match doc.resolve(i)?.value {
            // An integer in a rectangle is a coordinate, which f64 holds well beyond any page size.
            #[expect(clippy::cast_precision_loss, reason = "coordinates far below 2^53")]
            Value::Integer(n) => Some(n as f64),
            Value::Real(r) => Some(r),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let [a, b, c, d] = numbers.as_slice() else { return None };
    Some(Rect {
        x0: a.min(*c),
        y0: b.min(*d),
        x1: a.max(*c),
        y1: b.max(*d),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    #[test]
    fn rectangles_are_normalised_and_compared() {
        let pdf = classic(
            &[
                (
                    1,
                    b"<< /A [10 20.5 0 0] /B 2 0 R /C [0 0 1] /D [0 0 1 (x)] /E 5 /F 9 0 R /G [0 0 1 9 0 R] >>",
                ),
                (2, b"[0 0 3 0 R 30]"),
                (3, b"40"),
            ],
            "<< /Size SIZE /Root 1 0 R >>",
        );
        let doc = Document::open(&pdf, &Limits::default()).unwrap();
        let dict = doc.catalog().unwrap();
        let get = |k: &[u8]| rect(&doc, dict.get(k).unwrap());
        let a = get(b"A").unwrap();
        assert_eq!(
            a,
            Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 10.0,
                y1: 20.5
            }
        );
        let b = get(b"B").unwrap();
        assert!(a.within(&b) && !b.within(&a) && a.within(&a));
        assert_eq!((get(b"C"), get(b"D"), get(b"E")), (None, None, None));
        assert_eq!((get(b"F"), get(b"G")), (None, None), "references to nothing");
    }
}
