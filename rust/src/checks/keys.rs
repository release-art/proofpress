//! PDF/X-4 (ISO 15930-7) rows that forbid keys or values anywhere in the file (X4-Q3): external content (6.7), the
//! graphics state and halftones (6.13), PostScript (6.14), image alternates (6.16), actions and JavaScript (6.18),
//! viewer preferences (6.21), alternate presentations (6.22) and XFA (6.26).

use super::Settings;
use super::info::per_level;
use super::walk::{Node, dicts};
use crate::document::{Dict, Document, Object, Value};
use crate::findings::{
    Check, Finding, Location, X1A_FILESPEC_EMBEDDED_FILE, X4_ACROFORM_NO_XFA, X4_ACTIONS_ABSENT, X4_EXTGSTATE_NO_HTP,
    X4_EXTGSTATE_NO_TR, X4_EXTGSTATE_TR2_DEFAULT, X4_FILESPEC_EMBEDDED, X4_HALFTONE_NO_NAME,
    X4_HALFTONE_TRANSFER_FUNCTION, X4_HALFTONE_TYPE_1_OR_5, X4_IMAGE_NO_DEFAULT_FOR_PRINTING, X4_JAVASCRIPT_ABSENT,
    X4_NAMES_NO_ALTERNATE_PRESENTATIONS, X4_PAGE_NO_PRES_STEPS, X4_STREAM_NO_EXTERNAL_F, X4_VIEWER_PREFS_AREAS,
    X4_XOBJECT_NO_OPI, X4_XOBJECT_NO_POSTSCRIPT, X4_XOBJECT_NO_REF, X4_XOBJECT_NO_SUBTYPE2_PS,
    X5_G_REF_FILESPEC_EXTERNAL, X13_EXTGSTATE_NO_TR2, X13_FILESPEC_ABSENT, X13_VIEWER_PREFS_AREAS,
};

/// The action types of ISO 32000-1, 12.6.4, Table 198, and the obsolete `SetState` and `NOP` of PDF 1.2 (PDF 1.6
/// Reference, Appendix H); an action dictionary's `/S` names one.
const ACTIONS: [&[u8]; 20] = [
    b"GoTo",
    b"GoToR",
    b"GoToE",
    b"Launch",
    b"Thread",
    b"URI",
    b"Sound",
    b"Movie",
    b"Hide",
    b"Named",
    b"SubmitForm",
    b"ResetForm",
    b"ImportData",
    b"JavaScript",
    b"SetOCGState",
    b"Rendition",
    b"Trans",
    b"GoTo3DView",
    b"SetState",
    b"NOP",
];

/// The colorants a type 5 halftone may give a transfer function for are the others (X4-Q9).
const PRIMARY: [&[u8]; 9] = [
    b"Cyan", b"Magenta", b"Yellow", b"Black", b"Red", b"Green", b"Blue", b"Gray", b"Default",
];

fn finding(check: &'static Check, num: u32, message: &str, clause: &str) -> Finding {
    Finding::new(check, format!("object {num}: {message}"))
        .citing(clause)
        .at(vec![Location::Object(num, 0)])
}

/// A finding from `check` for every dictionary `test` picks out.
fn each(
    doc: &Document,
    check: &'static Check,
    clause: &str,
    test: &dyn Fn(&Node<'_>) -> Option<String>,
) -> Vec<Finding> {
    dicts(doc)
        .iter()
        .filter_map(|n| test(n).map(|m| finding(check, n.num, &m, clause)))
        .collect()
}

fn xobject(node: &Node<'_>, subtype: &[u8]) -> bool {
    node.stream && node.name(b"Subtype") == Some(subtype)
}

fn ext_gstate(node: &Node<'_>) -> bool {
    node.inside(b"ExtGState") || node.name(b"Type") == Some(b"ExtGState")
}

/// `message` when `holds`.
fn when(holds: bool, message: &str) -> Option<String> {
    holds.then(|| message.to_owned())
}

pub(super) fn no_opi(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_XOBJECT_NO_OPI, "6.7", &|n| {
        when(
            (xobject(n, b"Image") || xobject(n, b"Form")) && n.dict.get(b"OPI").is_some(),
            "an XObject with /OPI",
        )
    })
}

pub(super) fn no_ref(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_XOBJECT_NO_REF, "6.7", &|n| {
        when(
            xobject(n, b"Form") && n.dict.get(b"Ref").is_some(),
            "a form XObject with /Ref",
        )
    })
}

pub(super) fn no_external_f(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_STREAM_NO_EXTERNAL_F, "6.7", &|n| {
        when(n.stream && n.dict.get(b"F").is_some(), "a stream with /F")
    })
}

/// 6.7: every file specification embeds its file. File specifications are found by `/Type /Filespec` and where they
/// are written (`/FS`, `/AF`); a string there is a file specification without `/EF`. The URL specifications of an
/// external profile's `/URLs` are exempt, since A.2 forbids them `/EF` (X4-Q12); at PDF/X-4 the reference itself fails
/// `x4.output-intent.no-dest-profile-ref`.
pub(super) fn filespec_embedded(doc: &Document, _: &Settings) -> Vec<Finding> {
    filespec_rule(doc, X4_FILESPEC_EMBEDDED, "6.7", &|_| false)
}

/// ISO 15930-8 8.2 relaxes 6.7 for PDF/X-5g and X-5pg: the file a reference `XObject`'s `/Ref` names need not be
/// embedded, though it may be (X5-Q3). Every other file specification still embeds its file.
pub(super) fn filespec_embedded_x5(doc: &Document, _: &Settings) -> Vec<Finding> {
    // Only a form proxy's `/Ref` is a reference: one on an image or any other stream is an unknown key, and its file
    // specification owes `/EF` like any other.
    let files: Vec<&Dict> = super::x5g::proxies(doc)
        .iter()
        .filter_map(|p| {
            p.reference
                .get(b"F")
                .and_then(|f| doc.resolve(f))
                .and_then(Object::dict)
        })
        .collect();
    filespec_rule(doc, X5_G_REF_FILESPEC_EXTERNAL, "8.2", &|n| {
        files.iter().any(|f| std::ptr::eq(*f, n.dict))
    })
}

/// 6.7's rule, as `check`, for every file specification but `/URLs` ones and those `exempt` picks out.
fn filespec_rule(
    doc: &Document,
    check: &'static Check,
    clause: &str,
    exempt: &dyn Fn(&Node<'_>) -> bool,
) -> Vec<Finding> {
    let exempt = |n: &Node<'_>| n.under(b"URLs") || exempt(n);
    let mut out = each(doc, check, clause, &|n| {
        let filespec = n.name(b"Type") == Some(b"Filespec") || n.under(b"FS") || n.under(b"AF");
        when(
            filespec && !exempt(n) && n.dict.get(b"EF").is_none(),
            "a file specification without /EF",
        )
    });
    for node in dicts(doc) {
        for key in [b"FS".as_slice(), b"AF"] {
            let value = node.dict.get(key).and_then(|v| doc.resolve(v));
            let strings = match value.map(|v| &v.value) {
                Some(Value::String(..)) => 1,
                Some(Value::Array(items)) => items
                    .iter()
                    .filter(|i| matches!(doc.resolve(i).map(|o| &o.value), Some(Value::String(..))))
                    .count(),
                _ => 0,
            };
            out.extend((0..strings).map(|_| {
                finding(
                    check,
                    node.num,
                    "a file specification that is a string, with no /EF",
                    clause,
                )
            }));
        }
    }
    out
}

pub(super) fn no_tr(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_EXTGSTATE_NO_TR, "6.13", &|n| {
        when(ext_gstate(n) && n.dict.get(b"TR").is_some(), "an ExtGState with /TR")
    })
}

pub(super) fn no_htp(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_EXTGSTATE_NO_HTP, "6.13", &|n| {
        when(ext_gstate(n) && n.dict.get(b"HTP").is_some(), "an ExtGState with /HTP")
    })
}

/// The older levels ban `/TR2` outright (15930-1 6.15, -3 6.8, -4 and -6 6.9), where X-4 allows `/Default`.
pub(super) fn no_tr2(doc: &Document, settings: &Settings) -> Vec<Finding> {
    each(doc, X13_EXTGSTATE_NO_TR2, "6.15", &|n| {
        when(ext_gstate(n) && n.dict.get(b"TR2").is_some(), "an ExtGState with /TR2")
    })
    .into_iter()
    .map(|f| per_level(f, settings, ["6.15", "6.8", "6.9", "6.9"]))
    .collect()
}

/// Whether a dictionary is a file specification: `/Type /Filespec`, or the value of `/FS` or `/AF`.
fn is_filespec(n: &Node<'_>) -> bool {
    n.name(b"Type") == Some(b"Filespec") || n.under(b"FS") || n.under(b"AF")
}

/// 15930-3 6.15, -4 and -6 6.4 (Q22): no file specifications at all, as dictionaries or as strings under `/FS`.
/// Those of OPI, reference `XObject`s, external streams and actions are their own rows' too.
pub(super) fn filespec_absent(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let mut out = each(doc, X13_FILESPEC_ABSENT, "6.15", &|n| {
        when(is_filespec(n), "a file specification")
    });
    for node in dicts(doc) {
        let value = node.dict.get(b"FS").and_then(|v| doc.resolve(v));
        if matches!(value.map(|v| &v.value), Some(Value::String(..))) {
            out.push(finding(
                X13_FILESPEC_ABSENT,
                node.num,
                "a file specification that is a string",
                "6.15",
            ));
        }
    }
    out.into_iter()
        .map(|f| per_level(f, settings, ["", "6.15", "6.4", "6.4"]))
        .collect()
}

/// 15930-1 (Q22, a warning by the oracle's judgement): an X-1a:2001 file embeds no files.
pub(super) fn x1a_embedded_file(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X1A_FILESPEC_EMBEDDED_FILE, "6.5", &|n| {
        when(is_filespec(n) && n.dict.get(b"EF").is_some(), "an embedded file")
    })
}

/// 15930-4 and -6 6.17 (Q21): when any page has a `/BleedBox`, the viewer preferences' areas are `MediaBox` or
/// `BleedBox`; without one, any value.
pub(super) fn x13_viewer_areas(doc: &Document, settings: &Settings) -> Vec<Finding> {
    let Some(prefs) = catalog_dict(doc, b"ViewerPreferences") else {
        return Vec::new();
    };
    if !doc.pages().iter().any(|p| p.dict.get(b"BleedBox").is_some()) {
        return Vec::new();
    }
    [b"ViewArea".as_slice(), b"ViewClip", b"PrintArea", b"PrintClip"]
        .into_iter()
        .filter_map(|key| {
            let value = prefs.get(key).and_then(|v| doc.resolve(v))?;
            (!matches!(value.name(), Some(b"MediaBox" | b"BleedBox"))).then(|| {
                let key = String::from_utf8_lossy(key);
                let finding = Finding::new(
                    X13_VIEWER_PREFS_AREAS,
                    format!("ViewerPreferences /{key} is not MediaBox or BleedBox, and a page has a BleedBox"),
                );
                per_level(finding, settings, ["", "", "6.17", "6.17"])
            })
        })
        .collect()
}

pub(super) fn tr2_default(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_EXTGSTATE_TR2_DEFAULT, "6.13", &|n| {
        let tr2 = n.dict.get(b"TR2").filter(|_| ext_gstate(n));
        when(
            tr2.is_some_and(|v| v.name() != Some(b"Default")),
            "an ExtGState with /TR2 other than /Default",
        )
    })
}

/// Every halftone dictionary: one with `/HalftoneType` that is the value of `/HT` or has `/Type /Halftone`, with each type 5 halftone's components and the
/// colorant each is for.
fn halftones<'a>(doc: &'a Document, nodes: &[Node<'a>]) -> Vec<(u32, &'a Dict, Option<&'a [u8]>)> {
    let mut out = Vec::new();
    let halftone = |n: &&Node<'a>| {
        n.dict.get(b"HalftoneType").is_some() && (n.under(b"HT") || n.name(b"Type") == Some(b"Halftone"))
    };
    for node in nodes.iter().filter(halftone) {
        out.push((node.num, node.dict, None));
        if node.dict.get(b"HalftoneType").and_then(Object::integer) == Some(5) {
            for (key, value) in node.dict.entries() {
                if let Some(component) = doc.resolve(value).and_then(Object::dict) {
                    out.push((node.num, component, Some(key.as_slice())));
                }
            }
        }
    }
    out
}

pub(super) fn halftone_type(doc: &Document, _: &Settings) -> Vec<Finding> {
    let nodes = dicts(doc);
    halftones(doc, &nodes)
        .into_iter()
        .filter(|(_, ht, _)| !matches!(ht.get(b"HalftoneType").and_then(Object::integer), Some(1 | 5)))
        .map(|(num, ..)| {
            finding(
                X4_HALFTONE_TYPE_1_OR_5,
                num,
                "a halftone whose /HalftoneType is not 1 or 5",
                "6.13",
            )
        })
        .collect()
}

pub(super) fn halftone_name(doc: &Document, _: &Settings) -> Vec<Finding> {
    let nodes = dicts(doc);
    halftones(doc, &nodes)
        .into_iter()
        .filter(|(_, ht, _)| ht.get(b"HalftoneName").is_some())
        .map(|(num, ..)| finding(X4_HALFTONE_NO_NAME, num, "a halftone with /HalftoneName", "6.13"))
        .collect()
}

/// 6.13 (X4-Q9): a halftone carries `/TransferFunction` only as a component of a type 5 halftone for a colorant that
/// is not primary.
pub(super) fn halftone_transfer(doc: &Document, _: &Settings) -> Vec<Finding> {
    let nodes = dicts(doc);
    halftones(doc, &nodes)
        .into_iter()
        .filter(|(_, ht, colorant)| {
            ht.get(b"TransferFunction").is_some() && colorant.is_none_or(|c| PRIMARY.contains(&c))
        })
        .map(|(num, ..)| {
            finding(
                X4_HALFTONE_TRANSFER_FUNCTION,
                num,
                "a /TransferFunction outside a non-primary type 5 component",
                "6.13",
            )
        })
        .collect()
}

pub(super) fn no_postscript(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_XOBJECT_NO_POSTSCRIPT, "6.14", &|n| {
        when(xobject(n, b"PS"), "a PostScript XObject")
    })
}

pub(super) fn no_subtype2_ps(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_XOBJECT_NO_SUBTYPE2_PS, "6.14", &|n| {
        when(
            xobject(n, b"Form") && n.name(b"Subtype2") == Some(b"PS"),
            "a form XObject with /Subtype2 /PS",
        )
    })
}

/// 6.16: no image alternate has `/DefaultForPrinting true`.
pub(super) fn no_default_for_printing(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_IMAGE_NO_DEFAULT_FOR_PRINTING, "6.16", &|n| {
        let default = n
            .dict
            .get(b"DefaultForPrinting")
            .and_then(|v| doc.resolve(v))
            .map(|v| &v.value);
        when(
            n.under(b"Alternates") && default == Some(&Value::Bool(true)),
            "an image alternate that is the default for printing",
        )
    })
}

/// 6.18 (X4-Q11): no action of any kind. An action is a dictionary whose `/S` names an action type, found by
/// `/Type /Action` or where actions are written: `/A`, `/OpenAction` (a destination array there is not an action),
/// `/Next`, and inside `/AA`. `/S` keeps a Type 3 glyph named `A` from counting.
pub(super) fn no_actions(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_ACTIONS_ABSENT, "6.18", &|n| {
        let typed = n.name(b"S").is_some_and(|s| ACTIONS.contains(&s));
        let placed = n.name(b"Type") == Some(b"Action")
            || n.under(b"A")
            || n.under(b"OpenAction")
            || n.under(b"Next")
            || n.inside(b"AA");
        let action = typed && placed;
        let kind = String::from_utf8_lossy(n.name(b"S").unwrap_or_default()).into_owned();
        when(action, &format!("a {kind} action"))
    })
}

/// 6.18: no JavaScript: no `/JavaScript` name tree, and no dictionary with `/JS` or `/S /JavaScript`.
pub(super) fn no_javascript(doc: &Document, _: &Settings) -> Vec<Finding> {
    each(doc, X4_JAVASCRIPT_ABSENT, "6.18", &|n| {
        let script = n.dict.get(b"JS").is_some()
            || n.name(b"S") == Some(b"JavaScript")
            || (n.under(b"Names") && n.dict.get(b"JavaScript").is_some());
        when(script, "JavaScript")
    })
}

/// The catalog's entry `key` as a dictionary.
fn catalog_dict<'a>(doc: &'a Document, key: &[u8]) -> Option<&'a Dict> {
    doc.resolve(doc.catalog()?.get(key)?)?.dict()
}

fn catalog_finding(check: &'static Check, message: &str, clause: &str) -> Vec<Finding> {
    vec![Finding::new(check, message).citing(clause)]
}

/// 6.21: viewer areas and clips are `MediaBox`, or `BleedBox` when every page has one.
pub(super) fn viewer_areas(doc: &Document, _: &Settings) -> Vec<Finding> {
    let Some(prefs) = catalog_dict(doc, b"ViewerPreferences") else {
        return Vec::new();
    };
    let all_bleed = doc.pages().iter().all(|p| p.get(b"BleedBox").is_some());
    [b"ViewArea".as_slice(), b"ViewClip", b"PrintArea", b"PrintClip"]
        .into_iter()
        .filter_map(|key| {
            let value = prefs.get(key).and_then(|v| doc.resolve(v))?;
            let allowed = value.name() == Some(b"MediaBox") || (all_bleed && value.name() == Some(b"BleedBox"));
            (!allowed).then(|| {
                let key = String::from_utf8_lossy(key);
                Finding::new(
                    X4_VIEWER_PREFS_AREAS,
                    format!("ViewerPreferences /{key} is not MediaBox or an allowed BleedBox"),
                )
                .citing("6.21")
            })
        })
        .collect()
}

/// 6.22: no `/AlternatePresentations` in the name dictionary.
pub(super) fn no_alternate_presentations(doc: &Document, _: &Settings) -> Vec<Finding> {
    match catalog_dict(doc, b"Names").and_then(|n| n.get(b"AlternatePresentations")) {
        Some(_) => catalog_finding(
            X4_NAMES_NO_ALTERNATE_PRESENTATIONS,
            "the name dictionary has /AlternatePresentations",
            "6.22",
        ),
        None => Vec::new(),
    }
}

/// 6.22: no page has `/PresSteps`.
pub(super) fn no_pres_steps(doc: &Document, _: &Settings) -> Vec<Finding> {
    let pages = doc.pages();
    pages
        .iter()
        .enumerate()
        .filter(|(_, p)| p.dict.get(b"PresSteps").is_some())
        .map(|(i, _)| {
            let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
            Finding::new(X4_PAGE_NO_PRES_STEPS, format!("page {number} has /PresSteps"))
                .citing("6.22")
                .at(vec![Location::Page(number)])
        })
        .collect()
}

/// 6.26: the interactive form has no `/XFA`.
pub(super) fn no_xfa(doc: &Document, _: &Settings) -> Vec<Finding> {
    match catalog_dict(doc, b"AcroForm").and_then(|a| a.get(b"XFA")) {
        Some(_) => catalog_finding(X4_ACROFORM_NO_XFA, "the interactive form has /XFA", "6.26"),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::test_pdf::classic;
    use crate::findings::Level;
    use crate::pipeline::Limits;

    const RUNS: [fn(&Document, &Settings) -> Vec<Finding>; 19] = [
        no_opi,
        no_ref,
        no_external_f,
        filespec_embedded,
        no_tr,
        no_htp,
        tr2_default,
        halftone_type,
        halftone_name,
        halftone_transfer,
        no_postscript,
        no_subtype2_ps,
        no_default_for_printing,
        no_actions,
        no_javascript,
        viewer_areas,
        no_alternate_presentations,
        no_pres_steps,
        no_xfa,
    ];

    fn ids(objects: &[(u32, &[u8])]) -> Vec<&'static str> {
        let doc = Document::open(&classic(objects, "<< /Size SIZE /Root 1 0 R >>"), &Limits::default()).unwrap();
        RUNS.iter()
            .flat_map(|run| run(&doc, &Level::X4.into()))
            .map(|f| f.check.id)
            .collect()
    }

    #[test]
    fn what_is_allowed() {
        let catalog = b"<< /Pages 2 0 R /OpenAction [3 0 R /Fit] /ViewerPreferences << /ViewArea /BleedBox /PrintClip /MediaBox >> \
            /AF [<< /Type /Filespec /EF << /F 6 0 R >> >>] /Names << /Dests 7 0 R >> /AcroForm << /Fields [] >> >>";
        let objects: [(u32, &[u8]); 6] = [
            (1, catalog),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Resources << /ExtGState << /G 4 0 R /H << /HT << /HalftoneType 1 >> >> >> >> >>"),
            (3, b"<< /Type /Page /BleedBox [0 0 1 1] /CharProcs << /A 5 0 R >> >>"),
            (4, b"<< /TR2 /Default /HT << /HalftoneType 5 /Cyan << /HalftoneType 1 >> /Spot1 << /HalftoneType 1 /TransferFunction /Identity >> /Default /Unused >> >>"),
            (5, b"<< /Length 0 >>\nstream\n\nendstream"),
            (6, b"<< /Length 0 >>\nstream\n\nendstream"),
        ];
        assert_eq!(ids(&objects), Vec::<&str>::new());
    }

    #[test]
    fn what_is_not() {
        let catalog =
            b"<< /Pages 2 0 R /ViewerPreferences << /ViewArea /BleedBox /ViewClip 8 0 R >> /AF [(a.txt) 9 0 R] >>";
        let objects: [(u32, &[u8]); 10] = [
            (1, catalog),
            (2, b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Extra 11 0 R >>"),
            (3, b"<< /Type /Page /BleedBox [0 0 1 1] /AA << /O << /S /Named /N /NextPage >> >> /Annots [<< /FS (b.txt) /HT << /HalftoneType 5 /Cyan << /HalftoneType 1 /TransferFunction /Identity >> >> >>] >>"),
            (4, b"<< /Type /Page /A << /S /JavaScript /JS (1) >> /AA 10 0 R >>"),
            (10, b"<< /O << /S /SetState >> >>"),
            (11, b"<< /Type /Pages /Kids [] /Resources << /XObject << /I 12 0 R /F 13 0 R >> >> /HT 14 0 R >>"),
            (12, b"<< /Subtype /Image /OPI << >> /Length 0 >>\nstream\n\nendstream"),
            (13, b"<< /Subtype /Form /Ref << >> /Subtype2 /PS /Length 0 >>\nstream\n\nendstream"),
            (14, b"<< /Type /Halftone /HalftoneType 5 /Spot << /HalftoneType 1 /TransferFunction /Identity >> >>"),
            (9, b"<< /Type /Filespec >>"),
        ];
        assert_eq!(
            ids(&objects),
            [
                "x4.xobject.no-opi",
                "x4.xobject.no-ref",
                "x4.filespec.embedded",
                "x4.filespec.embedded",
                "x4.filespec.embedded",
                "x4.halftone.transfer-function",
                "x4.xobject.no-subtype2-ps",
                "x4.actions.absent",
                "x4.actions.absent",
                "x4.actions.absent",
                "x4.javascript.absent",
                "x4.viewer-prefs.areas",
            ],
            "a ViewClip that refers to nothing reads as null, so as absent"
        );
    }

    #[test]
    fn what_the_catalog_holds() {
        let catalog =
            b"<< /Pages 2 0 R /Names << /AlternatePresentations 3 0 R /JavaScript 3 0 R >> /AcroForm << /XFA 3 0 R >> \
            /ViewerPreferences 9 0 R >>";
        let objects: [(u32, &[u8]); 4] = [
            (1, catalog),
            (2, b"<< /Type /Pages /Kids [] /Resources << /XObject << /I 4 0 R >> >> >>"),
            (3, b"<< >>"),
            (4, b"<< /Subtype /Image /Alternates [<< /Image 4 0 R /DefaultForPrinting true >>] /Length 0 >>\nstream\n\nendstream"),
        ];
        assert_eq!(
            ids(&objects),
            [
                "x4.image.no-default-for-printing",
                "x4.javascript.absent",
                "x4.names.no-alternate-presentations",
                "x4.acroform.no-xfa",
            ]
        );
        let odd = classic(&[(1, b"5")], "<< /Size SIZE /Root 1 0 R >>");
        let doc = Document::open(&odd, &Limits::default()).unwrap();
        assert_eq!(
            RUNS.iter().flat_map(|run| run(&doc, &Level::X4.into())).count(),
            0,
            "a catalog that is not a dictionary"
        );
    }

    /// What `run` cites at `level` on one page with `page`'s entries, the catalog's `catalog` and objects `more`.
    fn older_cites(
        run: fn(&Document, &Settings) -> Vec<Finding>,
        level: Level,
        page: &str,
        catalog: &str,
        more: &[(u32, Vec<u8>)],
    ) -> Vec<String> {
        let doc = crate::checks::test_doc::paged(&[(page, "")], catalog, more);
        run(&doc, &level.into())
            .into_iter()
            .map(|f| f.clause.map_or_default(|c| format!("{}:{}", c.standard, c.number)))
            .collect()
    }

    #[test]
    fn rows_of_the_older_levels() {
        let tr2 = "/Resources << /ExtGState << /G << /Type /ExtGState /TR2 /Default >> >> >>";
        assert_eq!(older_cites(no_tr2, Level::X32002, tr2, "", &[]), ["iso-15930-3:6.8"]);
        let annot = "/Annots [<< /Type /Annot /Subtype /FileAttachment /Rect [0 0 1 1] /FS (notes.txt) >>]";
        assert_eq!(
            older_cites(filespec_absent, Level::X1a2003, annot, "", &[]),
            ["iso-15930-4:6.4"]
        );
        let embedded = "/Names << /EmbeddedFiles << /Names [(a) << /Type /Filespec /F (a) /EF << /F 9 0 R >> >>] >> >>";
        let data = (9, b"<< /Length 1 >>\nstream\nx\nendstream".to_vec());
        assert_eq!(
            older_cites(
                filespec_absent,
                Level::X32003,
                "",
                embedded,
                std::slice::from_ref(&data)
            ),
            ["iso-15930-6:6.4"]
        );
        assert_eq!(
            older_cites(x1a_embedded_file, Level::X1a2001, "", embedded, &[data]),
            ["iso-15930-1:6.5"]
        );
        let bleed = "/BleedBox [0 0 1 1]";
        let prefs = |value: &str| format!("/ViewerPreferences << /PrintArea /{value} /ViewArea 7 0 R >>");
        assert_eq!(
            older_cites(x13_viewer_areas, Level::X32003, bleed, &prefs("CropBox"), &[]),
            ["iso-15930-6:6.17"]
        );
        assert_eq!(
            older_cites(x13_viewer_areas, Level::X32003, bleed, &prefs("BleedBox"), &[]),
            [""; 0]
        );
        assert_eq!(
            older_cites(x13_viewer_areas, Level::X1a2003, "", &prefs("CropBox"), &[]),
            [""; 0]
        );
        assert_eq!(older_cites(x13_viewer_areas, Level::X1a2003, bleed, "", &[]), [""; 0]);
    }
}
