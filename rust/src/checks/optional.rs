//! PDF/X-4 (ISO 15930-7) 6.24, optional content: every configuration (`/D` and each of `/Configs`) is named, the
//! names are unique, an `/Order` is empty or lists every optional content group (X4-Q13: every group in
//! `/OCProperties /OCGs`), and no configuration has `/AS`.

use super::Settings;
use std::collections::BTreeSet;

use crate::common::text;
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{
    Check, Finding, X4_OC_CONFIG_NAME_PRESENT, X4_OC_CONFIG_NAME_UNIQUE, X4_OC_NO_AS, X4_OC_ORDER_COMPLETE,
};

/// The catalog's `/OCProperties`.
fn properties(doc: &Document) -> Option<&Dict> {
    doc.resolve(doc.catalog()?.get(b"OCProperties")?)?.dict()
}

/// Every configuration: `/D` first, then `/Configs` in order.
fn configs(doc: &Document) -> Vec<&Dict> {
    let Some(props) = properties(doc) else {
        return Vec::new();
    };
    let default = props.get(b"D").and_then(|d| doc.resolve(d)).and_then(Object::dict);
    let others = props
        .get(b"Configs")
        .and_then(|c| doc.resolve(c))
        .and_then(Object::array)
        .unwrap_or_default();
    default
        .into_iter()
        .chain(others.iter().filter_map(|c| doc.resolve(c)?.dict()))
        .collect()
}

fn finding(check: &'static Check, message: String) -> Finding {
    Finding::new(check, message).citing("6.24")
}

/// The configuration's name as text, or `None` when it has none that decodes.
fn name(doc: &Document, config: &Dict) -> Option<String> {
    match &doc.resolve(config.get(b"Name")?)?.value {
        Value::String(bytes, _) => text::decode(bytes),
        _ => None,
    }
}

pub(super) fn name_present(doc: &Document, _: &Settings) -> Vec<Finding> {
    configs(doc)
        .iter()
        .enumerate()
        .filter(|(_, c)| name(doc, c).is_none())
        .map(|(i, _)| {
            finding(
                X4_OC_CONFIG_NAME_PRESENT,
                format!("optional content configuration {} has no /Name", i + 1),
            )
        })
        .collect()
}

pub(super) fn name_unique(doc: &Document, _: &Settings) -> Vec<Finding> {
    let mut seen = BTreeSet::new();
    configs(doc)
        .iter()
        .filter_map(|c| name(doc, c))
        .filter(|n| !seen.insert(n.clone()))
        .map(|n| {
            finding(
                X4_OC_CONFIG_NAME_UNIQUE,
                format!("two optional content configurations are named {n:?}"),
            )
        })
        .collect()
}

/// The object numbers of the groups an `/Order` array refers to, at any depth.
fn ordered(doc: &Document, order: &[Object], out: &mut BTreeSet<u32>) {
    for item in order {
        match &item.value {
            Value::Reference(num, _) => match doc.resolve(item).map(|o| &o.value) {
                Some(Value::Array(nested)) => ordered(doc, nested, out),
                _ => {
                    out.insert(*num);
                }
            },
            Value::Array(nested) => ordered(doc, nested, out),
            _ => {}
        }
    }
}

/// 6.24 (X4-Q13): an `/Order` that is not empty names every group in `/OCGs`.
pub(super) fn order_complete(doc: &Document, _: &Settings) -> Vec<Finding> {
    let groups: BTreeSet<u32> = properties(doc)
        .and_then(|p| p.get(b"OCGs"))
        .and_then(|g| doc.resolve(g))
        .and_then(Object::array)
        .unwrap_or_default()
        .iter()
        .filter_map(|g| match g.value {
            Value::Reference(num, _) => Some(num),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for (i, config) in configs(doc).iter().enumerate() {
        let Some(order) = config
            .get(b"Order")
            .and_then(|o| doc.resolve(o))
            .and_then(Object::array)
        else {
            continue;
        };
        if order.is_empty() {
            continue;
        }
        let mut listed = BTreeSet::new();
        ordered(doc, order, &mut listed);
        let missing = groups.difference(&listed).count();
        if missing > 0 {
            out.push(finding(
                X4_OC_ORDER_COMPLETE,
                format!(
                    "the /Order of optional content configuration {} leaves out {missing} of {} groups",
                    i + 1,
                    groups.len()
                ),
            ));
        }
    }
    out
}

pub(super) fn no_as(doc: &Document, _: &Settings) -> Vec<Finding> {
    configs(doc)
        .iter()
        .enumerate()
        .filter(|(_, c)| c.get(b"AS").is_some())
        .map(|(i, _)| finding(X4_OC_NO_AS, format!("optional content configuration {} has /AS", i + 1)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    const RUNS: [fn(&Document, &Settings) -> Vec<Finding>; 4] = [name_present, name_unique, order_complete, no_as];

    fn ids(objects: &[(u32, &[u8])]) -> Vec<&'static str> {
        let doc = Document::open(&classic(objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        RUNS.iter()
            .flat_map(|run| run(&doc, &Level::X4.into()))
            .map(|f| f.check.id)
            .collect()
    }

    #[test]
    fn configurations() {
        let good: [(u32, &[u8]); 5] = [
            (1, b"<< /OCProperties 2 0 R >>"),
            (2, b"<< /OCGs [3 0 R 4 0 R 9] /D << /Name (A) /Order [(Label) 3 0 R [4 0 R] 5 0 R] >> /Configs [<< /Name (B) /Order [] >> 7] >>"),
            (3, b"<< /Type /OCG /Name (one) >>"),
            (4, b"<< /Type /OCG /Name (two) >>"),
            (5, b"[4 0 R]"),
        ];
        assert_eq!(ids(&good), Vec::<&str>::new());
        let bad: [(u32, &[u8]); 3] = [
            (1, b"<< /OCProperties << /OCGs [3 0 R 4 0 R] /D << /Name (A) /Order [3 0 R] /AS [] >> /Configs [<< /Name (A) >> << /Name 5 >>] >> >>"),
            (3, b"<< /Type /OCG >>"),
            (4, b"<< /Type /OCG >>"),
        ];
        assert_eq!(
            ids(&bad),
            [
                "x4.oc.config-name-present",
                "x4.oc.config-name-unique",
                "x4.oc.order-complete",
                "x4.oc.no-as"
            ]
        );
        assert_eq!(ids(&[(1, b"<< >>")]), Vec::<&str>::new());
        let odd: [(u32, &[u8]); 1] = [(
            1,
            b"<< /OCProperties << /D << /Name 9 0 R >> /Configs [8 0 R << /Name 3 >>] >> >>",
        )];
        assert_eq!(
            ids(&odd),
            ["x4.oc.config-name-present", "x4.oc.config-name-present"],
            "names that are not text"
        );
        assert_eq!(
            ids(&[(1, b"5")]),
            Vec::<&str>::new(),
            "a catalog that is not a dictionary"
        );
        assert_eq!(
            ids(&[(1, b"<< /OCProperties 9 0 R >>")]),
            Vec::<&str>::new(),
            "properties that refer to nothing"
        );
        let unnamed: [(u32, &[u8]); 1] = [(1, b"<< /OCProperties << /D << >> >> >>")];
        assert_eq!(ids(&unnamed), ["x4.oc.config-name-present"]);
    }
}
