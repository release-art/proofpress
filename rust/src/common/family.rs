//! Colour spaces reduced to the family a print check cares about: gray, RGB, CMYK or other.
//!
//! A space named in the resources is looked up there, `Indexed` is looked through to its base, and an `ICCBased` space
//! goes by its profile's data colour space, or by `/N` when the profile cannot be read. `Separation` and `DeviceN` are
//! other: their alternate space is a rendering hint for a spot ink, not the artwork's own space. Lookups stop five
//! levels down.

use crate::document::{Dict, Document, Object, Value};

/// A colour family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Family {
    Gray,
    Rgb,
    Cmyk,
    Other,
}

/// How deep named spaces and `Indexed` bases are followed.
const DEPTH: usize = 4;

/// The family of a colour space name that needs no lookup.
fn family_of(name: &[u8]) -> Option<Family> {
    match name {
        b"DeviceRGB" | b"CalRGB" => Some(Family::Rgb),
        b"DeviceCMYK" => Some(Family::Cmyk),
        b"DeviceGray" | b"CalGray" => Some(Family::Gray),
        b"Lab" | b"Pattern" => Some(Family::Other),
        _ => None,
    }
}

/// The family of a profile's colour space, by its components.
fn by_components(n: i64) -> Family {
    match n {
        1 => Family::Gray,
        3 => Family::Rgb,
        4 => Family::Cmyk,
        _ => Family::Other,
    }
}

/// The colour space a name gives in `resources`.
fn named<'a>(doc: &'a Document, resources: Option<&'a Dict>, name: &[u8]) -> Option<&'a Object> {
    doc.resolve(doc.resolve(resources?.get(b"ColorSpace")?)?.dict()?.get(name)?)
}

/// The family of `space`, a colour space operand or object, names looked up in `resources`.
pub(crate) fn classify(doc: &Document, space: Option<&Object>, resources: Option<&Dict>) -> Family {
    classify_at(doc, space, resources, 0)
}

fn classify_at(doc: &Document, space: Option<&Object>, resources: Option<&Dict>, depth: usize) -> Family {
    let Some(space) = space.filter(|_| depth <= DEPTH).and_then(|s| doc.resolve(s)) else {
        return Family::Other;
    };
    match &space.value {
        Value::Array(items) => classify_array(doc, items, resources, depth),
        Value::Name(name) => family_of(name).unwrap_or_else(|| {
            named(doc, resources, name).map_or(Family::Other, |n| classify_at(doc, Some(n), resources, depth + 1))
        }),
        _ => Family::Other,
    }
}

fn classify_array(doc: &Document, items: &[Object], resources: Option<&Dict>, depth: usize) -> Family {
    let head = items.first().and_then(|h| doc.resolve(h)).and_then(Object::name);
    match head {
        Some(b"ICCBased") => icc(doc, items.get(1)),
        Some(b"Indexed") => classify_at(doc, items.get(1), resources, depth + 1),
        Some(name) => family_of(name).unwrap_or(Family::Other),
        None => Family::Other,
    }
}

/// An ICC profile stream's family: its header's data colour space when the profile can be read, else its `/N`.
fn icc(doc: &Document, stream: Option<&Object>) -> Family {
    let Some(stream) = stream else { return Family::Other };
    let Some(n) = doc
        .resolve(stream)
        .and_then(Object::dict)
        .and_then(|d| d.get(b"N"))
        .and_then(|n| doc.resolve(n))
        .and_then(truncated)
    else {
        return Family::Other;
    };
    let data = match stream.value {
        Value::Reference(num, _) => doc.stream_data(num),
        _ => None,
    };
    match data.as_deref().and_then(|d| d.get(16..20)) {
        Some(b"GRAY") => Family::Gray,
        Some(b"RGB ") => Family::Rgb,
        Some(b"CMYK") => Family::Cmyk,
        Some(_) => Family::Other,
        None => by_components(n),
    }
}

/// A number as an integer, a real truncated.
fn truncated(object: &Object) -> Option<i64> {
    match object.value {
        Value::Integer(n) => Some(n),
        #[expect(clippy::cast_possible_truncation, reason = "truncation is the reading")]
        Value::Real(r) => Some(r as i64),
        _ => None,
    }
}

/// Whether `space` is `DeviceGray`, `DeviceRGB` or `DeviceCMYK`, by name or through a resource name.
pub(crate) fn is_device(doc: &Document, space: Option<&Object>, resources: Option<&Dict>) -> bool {
    let device = |name: &[u8]| matches!(name, b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK");
    let Some(name) = space.and_then(Object::name) else {
        return false;
    };
    device(name) || named(doc, resources, name).and_then(Object::name).is_some_and(device)
}

/// The ink names a `Separation` or `DeviceN` space paints with, through names and `Indexed`; none for any other space.
pub(crate) fn spot_names(doc: &Document, space: Option<&Object>, resources: Option<&Dict>) -> Vec<Vec<u8>> {
    spots_at(doc, space, resources, 0)
}

fn spots_at(doc: &Document, space: Option<&Object>, resources: Option<&Dict>, depth: usize) -> Vec<Vec<u8>> {
    let Some(space) = space.filter(|_| depth <= DEPTH).and_then(|s| doc.resolve(s)) else {
        return Vec::new();
    };
    let items = match &space.value {
        Value::Array(items) => items,
        Value::Name(name) => {
            return spots_at(doc, named(doc, resources, name), resources, depth + 1);
        }
        _ => return Vec::new(),
    };
    let second = items.get(1).and_then(|s| doc.resolve(s));
    match items.first().and_then(|h| doc.resolve(h)).and_then(Object::name) {
        Some(b"Separation") => second.and_then(Object::name).map(<[u8]>::to_vec).into_iter().collect(),
        Some(b"DeviceN") => second
            .and_then(Object::array)
            .unwrap_or_default()
            .iter()
            .filter_map(|n| doc.resolve(n)?.name().map(<[u8]>::to_vec))
            .collect(),
        Some(b"Indexed") => spots_at(doc, items.get(1), resources, depth + 1),
        _ => Vec::new(),
    }
}

/// The underlying space of a `[/Pattern base]` space (8.7.3.3), by name or array; `None` for any other space.
pub(crate) fn pattern_base<'a>(
    doc: &'a Document,
    space: Option<&'a Object>,
    resources: Option<&'a Dict>,
) -> Option<&'a Object> {
    let space = space?;
    let space = match space.name() {
        Some(name) => named(doc, resources, name)?,
        None => doc.resolve(space)?,
    };
    match space.array()? {
        [head, base, ..] if doc.resolve(head).and_then(Object::name) == Some(b"Pattern") => Some(base),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::pipeline::Limits;

    fn profile(space: &[u8]) -> String {
        let mut header = vec![b'x'; 16];
        header.extend(space);
        header.extend([b'x'; 4]);
        let text = String::from_utf8(header).unwrap();
        format!("<< /N 3 /Length {} >>\nstream\n{text}\nendstream", text.len())
    }

    /// The document whose catalog's `/R` is a resource dictionary and `/S` a list of spaces to judge.
    fn doc(spaces: &str) -> Document {
        let resources = "<< /ColorSpace << /C /DeviceCMYK /I [/ICCBased 5 0 R] /Me /Me /Sp [/Separation /Gold /DeviceCMYK 7 0 R] >> >>";
        let catalog = format!(
            "<< /R {resources} /S [{spaces}] /Dangling << /ColorSpace 99 0 R >> /NotADict << /ColorSpace 5 >> >>"
        );
        let objects: [(u32, &[u8]); 9] = [
            (1, catalog.as_bytes()),
            (5, profile(b"CMYK").leak().as_bytes()),
            (6, profile(b"Lab ").leak().as_bytes()),
            (8, b"<< /N 4 /Length 3 >>\nstream\nabc\nendstream"),
            (9, b"<< /N 3 /Filter /FlateDecode /Length 3 >>\nstream\nabc\nendstream"),
            (10, profile(b"GRAY").leak().as_bytes()),
            (11, profile(b"RGB ").leak().as_bytes()),
            (12, b"<< /N 1 /Length 3 >>\nstream\nabc\nendstream"),
            (13, b"<< /N 2 /Length 3 >>\nstream\nabc\nendstream"),
        ];
        Document::open(&classic(&objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap()
    }

    fn each<T>(spaces: &str, judge: &dyn Fn(&Document, &Object, Option<&Dict>) -> T) -> Vec<T> {
        let doc = doc(spaces);
        let catalog = doc.catalog().unwrap();
        let resources = catalog.get(b"R").and_then(Object::dict);
        catalog
            .get(b"S")
            .unwrap()
            .array()
            .unwrap()
            .iter()
            .map(|s| judge(&doc, s, resources))
            .collect()
    }

    #[test]
    fn families() {
        use Family::{Cmyk, Gray, Other, Rgb};
        let got = each(
            "/DeviceGray /CalRGB /DeviceCMYK /Lab /Pattern /C /I /Nope /Me 5 [] [5] \
             [/ICCBased 6 0 R] [/ICCBased 8 0 R] [/ICCBased 9 0 R] [/ICCBased] [/ICCBased << >>] [/ICCBased << /N 4.0 >>] \
             [/ICCBased << /N /X >>] [/Indexed /C 1 <00>] [/Separation /S /DeviceCMYK 7 0 R] [/CalGray << >>] [/Foo] \
             [/ICCBased 10 0 R] [/ICCBased 11 0 R] [/ICCBased 12 0 R] [/ICCBased 13 0 R]",
            &|d, s, r| classify(d, Some(s), r),
        );
        assert_eq!(
            got,
            [
                Gray, Rgb, Cmyk, Other, Other, Cmyk, Cmyk, Other, Other, Other, Other, Other, Other, Cmyk, Rgb, Other,
                Other, Cmyk, Other, Cmyk, Other, Gray, Other, Gray, Rgb, Gray, Other
            ]
        );
        assert_eq!(classify(&doc(""), None, None), Other);
    }

    #[test]
    fn device_spaces_spots_and_pattern_bases() {
        let device = each("/DeviceRGB /C /I /Nope 5", &|d, s, r| is_device(d, Some(s), r));
        assert_eq!(device, [true, true, false, false, false]);
        let spots = each(
            "/Sp [/DeviceN [/A 5 99 0 R /B] /DeviceCMYK 7 0 R] [/Indexed /Sp 1 <00>] [/Separation 5] [/DeviceN 5] /C /Me 5 []",
            &|d, s, r| spot_names(d, Some(s), r),
        );
        let names = |n: &[&str]| n.iter().map(|s| s.as_bytes().to_vec()).collect::<Vec<_>>();
        assert_eq!(
            spots,
            [
                names(&["Gold"]),
                names(&["A", "B"]),
                names(&["Gold"]),
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec![]
            ]
        );
        let bases = each(
            "[/Pattern /DeviceRGB] /Nope /C [/Pattern] [/Indexed /C] 5 99 0 R",
            &|d, s, r| pattern_base(d, Some(s), r).and_then(Object::name).map(<[u8]>::to_vec),
        );
        assert_eq!(bases, [Some(b"DeviceRGB".to_vec()), None, None, None, None, None, None]);
        assert!(pattern_base(&doc(""), None, None).is_none());
        let odd = doc("");
        let catalog = odd.catalog().unwrap();
        for resources in [b"Dangling".as_slice(), b"NotADict"] {
            let r = catalog.get(resources).and_then(Object::dict);
            assert!(named(&odd, r, b"C").is_none(), "a /ColorSpace that is not a dictionary");
        }
        assert!(named(&odd, None, b"C").is_none(), "no resources");
    }
}
