//! `_internal/pdfx/x4.py` and the structure.py helpers it needs, line for line, on hayro-syntax.
use hayro_syntax::Pdf;
use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier, Stream};
use hayro_syntax::page::Page;

const TOLERANCE: f64 = 0.5;
type Rect = [f64; 4];

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub check: &'static str,
    pub severity: &'static str,
    pub message: String,
    pub page: Option<usize>,
}

fn err(check: &'static str, message: &str, page: Option<usize>) -> Finding {
    Finding { check, severity: "error", message: message.into(), page }
}

fn rect(o: Option<Array<'_>>) -> Option<Rect> {
    let v: Option<Vec<f64>> = o?.iter::<Object<'_>>().map(|x| x.into_number().map(|n| n.as_f64())).collect();
    let [x0, y0, x1, y1]: [f64; 4] = v?.try_into().ok()?;
    Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)])
}

fn contains(o: &Rect, i: &Rect) -> bool {
    i[0] >= o[0] - TOLERANCE && i[1] >= o[1] - TOLERANCE && i[2] <= o[2] + TOLERANCE && i[3] <= o[3] + TOLERANCE
}

fn own_box(page: &Dict<'_>, name: &str) -> Option<Rect> {
    rect(page.get::<Array<'_>>(name))
}

fn media_box(page: &Page<'_>) -> Option<Rect> {
    let mut node = page.raw().clone();
    let mut seen: Vec<ObjectIdentifier> = vec![];
    loop {
        if let Some(id) = node.obj_id() {
            if seen.contains(&id) {
                return None;
            }
            seen.push(id);
        }
        if let Some(b) = own_box(&node, "MediaBox") {
            return Some(b);
        }
        node = node.get::<Dict<'_>>("Parent")?;
    }
}

fn page_boxes(pdf: &Pdf, out: &mut Vec<Finding>) {
    for (n, page) in pdf.pages().iter().enumerate() {
        let i = Some(n + 1);
        let obj = page.raw();
        let (trim, art, bleed) = (own_box(obj, "TrimBox"), own_box(obj, "ArtBox"), own_box(obj, "BleedBox"));
        let Some(media) = media_box(page) else {
            out.push(err("boxes", "no MediaBox", i));
            continue;
        };
        let finished = trim.or(art);
        if trim.is_none() && art.is_none() {
            out.push(err("trimbox", "no TrimBox or ArtBox -- PDF/X-4 needs one to mark the finished page", i));
        } else if trim.is_some() && art.is_some() {
            out.push(err("trimbox", "both TrimBox and ArtBox present -- PDF/X-4 allows only one", i));
        }
        if finished.is_some_and(|f| !contains(&media, &f)) {
            out.push(err("trimbox", "the trim/art box extends beyond the MediaBox", i));
        }
        if let Some(b) = bleed {
            if !contains(&media, &b) {
                out.push(err("bleedbox", "the BleedBox extends beyond the MediaBox", i));
            }
            if finished.is_some_and(|f| !contains(&b, &f)) {
                out.push(err("bleedbox", "the BleedBox does not contain the trim/art box", i));
            }
        } else if finished.is_some() {
            out.push(Finding {
                check: "bleedbox",
                severity: "warn",
                message: "no BleedBox -- add bleed if the artwork runs to the trim edge".into(),
                page: i,
            });
        }
    }
}

fn trapped(pdf: &Pdf, out: &mut Vec<Finding>) {
    let value = pdf.xref().info().and_then(|d| d.get::<Object<'_>>("Trapped"));
    let text = match value {
        None | Some(Object::Null(_)) => {
            out.push(err("trapped", "/Trapped missing from Info -- PDF/X requires True or False", None));
            return;
        }
        Some(Object::Name(n)) => format!("/{}", n.as_str()),
        Some(Object::String(s)) => String::from_utf8_lossy(s.as_bytes()).into_owned(),
        Some(Object::Boolean(b)) => if b { "True".into() } else { "False".into() },
        Some(o) => format!("{o:?}"),
    };
    if text != "/True" && text != "/False" {
        let shown = text.trim_start_matches('/');
        out.push(err("trapped", &format!("/Trapped is {shown} -- PDF/X requires True or False"), None));
    }
}

fn identity(pdf: &Pdf, out: &mut Vec<Finding>) {
    let xref = pdf.xref();
    let root = xref.get::<Dict<'_>>(xref.root_id()).unwrap_or_default();
    let xmp = root.get::<Stream<'_>>("Metadata").and_then(|s| s.decoded_data().ok());
    if !xmp.is_some_and(|d| d.windows(15).any(|w| w == b"GTS_PDFXVersion")) {
        out.push(err("pdfx-xmp", "XMP lacks pdfxid:GTS_PDFXVersion -- the PDF/X version identifier", None));
    }
    let has = root.get::<Array<'_>>("OutputIntents").is_some_and(|a| {
        a.iter::<Object<'_>>()
            .filter_map(|o| o.into_dict())
            .any(|d| d.get::<Name<'_>>("S").is_some_and(|s| s.as_str() == "GTS_PDFX"))
    });
    if !has {
        out.push(err("pdfx-intent", "no OutputIntent with subtype GTS_PDFX -- PDF/X-4 needs a print output intent", None));
    }
}

pub fn check_pdfx(path: &str) -> Result<Vec<Finding>, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let pdf = Pdf::new(data).map_err(|e| format!("{e:?}"))?;
    let mut out = vec![];
    page_boxes(&pdf, &mut out);
    trapped(&pdf, &mut out);
    identity(&pdf, &mut out);
    Ok(out)
}

#[cfg(feature = "python")]
#[pyo3::pymodule]
mod proofcore {
    use pyo3::exceptions::PyValueError;
    use pyo3::prelude::*;

    /// The same tuples as (check, severity, message, page); the GIL is released while the file is read.
    #[pyfunction]
    fn check_pdfx(py: Python<'_>, path: String) -> PyResult<Vec<(&'static str, &'static str, String, Option<usize>)>> {
        let found = py.detach(|| super::check_pdfx(&path)).map_err(PyValueError::new_err)?;
        Ok(found.into_iter().map(|f| (f.check, f.severity, f.message, f.page)).collect())
    }
}
