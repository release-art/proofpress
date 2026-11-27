//! The Python module `proof_pdf._internal._core`, built by maturin with the `python` feature. Python hands over whole files and
//! gets whole reports back; it never sees single operators.

use std::fmt::Display;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::checks;
use crate::findings::Level;

/// Verify `data`, the bytes of the file named `file`, at `level` (`x4`, `x4p`, `x5g`, `x5pg`, `x5n`, `x1a-2001`, `x3-2002`,
/// `x1a-2003`, `x3-2003` or `print`), with
/// the print checks' parameters given as a JSON object (`params`, by name; any left out keep their defaults), and return
/// the report as JSON (schema 1). The GIL is released while the file is read.
#[pyfunction]
#[pyo3(signature = (file, data, level, params = "{}"))]
fn verify(py: Python<'_>, file: &str, data: &[u8], level: &str, params: &str) -> PyResult<String> {
    let level = Level::from_name(level).ok_or_else(|| value_error(format_args!("unknown level {level:?}")))?;
    let params = serde_json::from_str(params).map_err(|e| value_error(format_args!("params is not valid: {e}")))?;
    let settings = checks::Settings {
        level,
        params,
        files: checks::Files::Alone,
    };
    let report = py.detach(|| checks::verify_settings(file, data, &settings));
    serde_json::to_string(&report).map_err(value_error)
}

/// Run the `proof-pdf` command line with `argv` (the program name first) and return its exit code, standard output and
/// standard error; `colour` when the output goes to a terminal. The wheel's `proof-pdf verify` and the Python API both
/// come here.
#[pyfunction]
#[pyo3(signature = (argv, colour = false))]
fn run(py: Python<'_>, argv: Vec<String>, colour: bool) -> (i32, String, String) {
    py.detach(|| {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = crate::cli::main(argv.into_iter().map(Into::into), &mut out, &mut err, colour);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    })
}

/// What is in the PDF `data`, as JSON: its pages, fonts, text colours, spot inks, images and, unless `with_contrast`
/// is false, the contrast of each text and background pair, measured through `proof_profile` (the bytes of an ICC
/// profile) or else the output intents' profiles. Raises `UnreadablePdfError` for a file that cannot be read.
#[pyfunction]
#[pyo3(signature = (data, proof_profile = None, with_contrast = true))]
fn inspect(py: Python<'_>, data: &[u8], proof_profile: Option<&[u8]>, with_contrast: bool) -> PyResult<String> {
    let inventory = py
        .detach(|| checks::inventory::inspect(data, proof_profile, with_contrast))
        .map_err(UnreadablePdfError::new_err)?;
    serde_json::to_string(&inventory).map_err(value_error)
}

/// A `ValueError` saying `message`.
fn value_error(message: impl Display) -> PyErr {
    PyValueError::new_err(message.to_string())
}

pyo3::create_exception!(
    _core,
    UnreadablePdfError,
    pyo3::exceptions::PyException,
    "A PDF that cannot be read: its structure is too damaged, or it is encrypted."
);

/// Cut `bounds` (`x0, y0, x1, y1` in points, either corner order, widened by `pad` on every side) of page `page`,
/// counting from 1, of the PDF `data` out as a one-page PDF. Raises `ValueError` for a page that does not exist or a box
/// that misses it, and `UnreadablePdfError` for a file that cannot be read.
#[pyfunction]
#[pyo3(signature = (data, page, bounds, pad = 0.0))]
fn crop(py: Python<'_>, data: &[u8], page: usize, bounds: [f64; 4], pad: f64) -> PyResult<Vec<u8>> {
    use crate::writer::crop::{CropError, crop};
    py.detach(|| crop(data, page, bounds, pad)).map_err(|e| match e {
        CropError::Unreadable(why) => UnreadablePdfError::new_err(why),
        CropError::Damaged(objects) => {
            let objects: Vec<String> = objects.iter().map(u32::to_string).collect();
            UnreadablePdfError::new_err(format!(
                "page {page} needs objects that cannot be read: {}",
                objects.join(", ")
            ))
        }
        CropError::NoPage(pages) => value_error(format_args!("the file has {pages} page(s); there is no page {page}")),
        CropError::Outside([x0, y0, x1, y1]) => value_error(format_args!(
            "the box {bounds:?} lies outside page {page}, which spans ({x0}, {y0}) to ({x1}, {y1}) points"
        )),
    })
}

/// Each page of the PDF `data` as PNG bytes at `dpi`, with the trim and bleed guides (and a safe-area guide `safe_mm`
/// inside the trim, when given) drawn when `guides` is true. Raises `UnreadablePdfError` for a file that cannot be read
/// and `ValueError` for a page too large to render at `dpi`.
#[pyfunction]
#[pyo3(signature = (data, dpi, guides = false, safe_mm = None))]
fn render(py: Python<'_>, data: &[u8], dpi: f64, guides: bool, safe_mm: Option<f64>) -> PyResult<Vec<Vec<u8>>> {
    use crate::preview::{Guides, RenderError, render};
    py.detach(|| render(data, dpi, guides.then_some(Guides { safe_mm })))
        .map_err(|e| match e {
            RenderError::Unreadable(why) => UnreadablePdfError::new_err(why),
            RenderError::Size(why) => value_error(why),
        })
}

/// What `extract` recovers from the PDF `data` that needs no content walk: page boxes, document information and XMP,
/// font programs and ICC profiles with the output intents, and with `images` every image written out, as JSON, with the recovered files' bytes the JSON indexes.
/// Raises `UnreadablePdfError` for a file that cannot be read.
#[pyfunction]
#[pyo3(signature = (data, images = false, vector = false))]
fn extract(py: Python<'_>, data: &[u8], images: bool, vector: bool) -> PyResult<(String, Vec<Vec<u8>>)> {
    let recovered = py
        .detach(|| crate::extract::recover(data, images, vector))
        .map_err(UnreadablePdfError::new_err)?;
    serde_json::to_string(&recovered.facts)
        .map(|facts| (facts, recovered.blobs))
        .map_err(value_error)
}

/// The pieces of vector artwork the JSON `requests` asks for (page, box, shape indices and text-run indices, as
/// `extract` reported them; or an `image` object number alone, filling the box, for a thumbnail), each written
/// out as a one-page PDF: JSON naming each piece's content stream, resources and PDF among the bytes returned with
/// it. Raises `UnreadablePdfError` for a file that cannot be read and `ValueError` for requests that are not of the
/// right shape.
#[pyfunction]
fn vector_pieces(py: Python<'_>, data: &[u8], requests: &str) -> PyResult<(String, Vec<Vec<u8>>)> {
    use crate::extract::PiecesError;
    py.detach(|| crate::extract::vector_pieces(data, requests))
        .map_err(|e| match e {
            PiecesError::Unreadable(why) => UnreadablePdfError::new_err(why),
            PiecesError::Request(why) => value_error(why),
        })
}

/// The module itself, `proof_pdf._internal._core` to Python.
#[pymodule]
mod _core {
    #[pymodule_export]
    use super::{UnreadablePdfError, crop, extract, inspect, render, run, vector_pieces, verify};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_returns_the_report_as_json() {
        Python::initialize();
        Python::attach(|py| {
            let module = pyo3::wrap_pymodule!(_core)(py);
            let verify = module.bind(py).getattr("verify").unwrap();
            let json: String = verify
                .call1(("x.pdf", b"%PDF-1.7".as_slice(), "x4"))
                .unwrap()
                .extract()
                .unwrap();
            assert!(
                json.starts_with(r#"{"schema":1,"file":"x.pdf","verdict":"refused""#),
                "{json}"
            );
            let error = verify.call1(("x.pdf", b"".as_slice(), "x3")).unwrap_err();
            assert!(error.to_string().contains("unknown level"), "{error}");
            let json: String = verify
                .call1(("x.pdf", b"%PDF-1.7".as_slice(), "print", r#"{"small_text_pt": 9}"#))
                .unwrap()
                .extract()
                .unwrap();
            assert!(json.contains(r#""verdict":"refused""#), "{json}");
            let error = verify
                .call1(("x.pdf", b"".as_slice(), "print", r#"{"nope": 1}"#))
                .unwrap_err();
            assert!(error.to_string().contains("params is not valid"), "{error}");
        });
    }

    #[test]
    fn run_returns_the_exit_code_and_output() {
        Python::initialize();
        Python::attach(|py| {
            let module = pyo3::wrap_pymodule!(_core)(py);
            let run = module.bind(py).getattr("run").unwrap();
            let (code, out, err): (i32, String, String) =
                run.call1((vec!["proof-pdf", "--version"],)).unwrap().extract().unwrap();
            assert_eq!((code, out.starts_with("proof-pdf "), err.as_str()), (0, true, ""));
            let (code, _, err): (i32, String, String) = run
                .call1((vec!["proof-pdf", "verify", "--bogus"], true))
                .unwrap()
                .extract()
                .unwrap();
            assert_eq!(code, 64);
            assert!(err.contains("--bogus"), "{err}");
        });
    }

    #[test]
    fn inspect_returns_the_inventory_as_json() {
        Python::initialize();
        Python::attach(|py| {
            let module = pyo3::wrap_pymodule!(_core)(py);
            let inspect = module.bind(py).getattr("inspect").unwrap();
            let file = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pdfs/good.pdf")).unwrap();
            let json: String = inspect.call1((file.as_slice(),)).unwrap().extract().unwrap();
            let facts: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(facts["pages"][0]["number"], 1);
            let error = inspect.call1((b"x".as_slice(),)).unwrap_err().to_string();
            assert!(error.starts_with("UnreadablePdfError"), "{error}");
        });
    }

    #[test]
    fn render_returns_a_png_a_page_or_says_why() {
        Python::initialize();
        Python::attach(|py| {
            let module = pyo3::wrap_pymodule!(_core)(py);
            let render = module.bind(py).getattr("render").unwrap();
            let file = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pdfs/good.pdf")).unwrap();
            let pngs: Vec<Vec<u8>> = render
                .call1((file.as_slice(), 72.0, true, 3.0))
                .unwrap()
                .extract()
                .unwrap();
            assert!(pngs.len() == 1 && pngs[0].starts_with(b"\x89PNG"));
            let message = |args: (&[u8], f64)| render.call1(args).unwrap_err().to_string();
            assert!(message((b"x".as_slice(), 72.0)).starts_with("UnreadablePdfError"));
            assert!(message((file.as_slice(), 0.0)).starts_with("ValueError: page 1 at 0 dpi"));
        });
    }

    #[test]
    fn extract_returns_facts_and_blobs_or_says_why() {
        Python::initialize();
        Python::attach(|py| {
            let module = pyo3::wrap_pymodule!(_core)(py);
            let extract = module.bind(py).getattr("extract").unwrap();
            let file = std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../tests/fixtures/pdfs/x4-good.pdf"
            ))
            .unwrap();
            let (json, blobs): (String, Vec<Vec<u8>>) = extract.call1((file.as_slice(),)).unwrap().extract().unwrap();
            let facts: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(facts["pages"][0]["number"], 1);
            assert_eq!((blobs.len(), &facts["xmp"]), (3, &serde_json::json!({"data": 0})));
            let error = extract.call1((b"x".as_slice(),)).unwrap_err().to_string();
            assert!(error.starts_with("UnreadablePdfError"), "{error}");
        });
    }

    #[test]
    fn vector_pieces_or_says_why() {
        Python::initialize();
        Python::attach(|py| {
            let module = pyo3::wrap_pymodule!(_core)(py);
            let pieces = module.bind(py).getattr("vector_pieces").unwrap();
            let file = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pdfs/good.pdf")).unwrap();
            let request = r#"[{"page": 1, "bbox": [0, 0, 10, 10], "shapes": [0], "labels": []}]"#;
            let (json, blobs): (String, Vec<Vec<u8>>) =
                pieces.call1((file.as_slice(), request)).unwrap().extract().unwrap();
            assert!(json.contains("\"pdf\":1") && blobs[1].starts_with(b"%PDF-"), "{json}");
            let message = |args: (&[u8], &str)| pieces.call1(args).unwrap_err().to_string();
            assert!(message((file.as_slice(), "{")).starts_with("ValueError"));
            assert!(message((b"x".as_slice(), "[]")).starts_with("UnreadablePdfError"));
        });
    }

    #[test]
    fn crop_cuts_a_page_or_says_why() {
        Python::initialize();
        Python::attach(|py| {
            let module = pyo3::wrap_pymodule!(_core)(py);
            let crop = module.bind(py).getattr("crop").unwrap();
            let file = std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../tests/fixtures/pdfs/x4-good.pdf"
            ))
            .unwrap();
            let box_ = (0.0, 0.0, 100.0, 100.0);
            let piece: Vec<u8> = crop.call1((file.as_slice(), 1, box_)).unwrap().extract().unwrap();
            assert!(piece.starts_with(b"%PDF-"));
            let message = |args: (&[u8], usize, (f64, f64, f64, f64))| crop.call1(args).unwrap_err().to_string();
            assert!(message((file.as_slice(), 2, box_)).contains("there is no page 2"));
            assert!(message((file.as_slice(), 1, (9000.0, 9000.0, 9001.0, 9001.0))).contains("lies outside page 1"));
            assert!(message((b"x".as_slice(), 1, box_)).starts_with("UnreadablePdfError"));
            // Every stream of the file ends without endstream: damaged objects a piece would lack.
            let mut damaged = file.clone();
            let ends: Vec<usize> = damaged
                .windows(9)
                .enumerate()
                .filter(|(_, w)| *w == b"endstream")
                .map(|(i, _)| i + 8)
                .collect();
            for at in ends {
                damaged[at] = b'X';
            }
            let why = message((damaged.as_slice(), 1, box_));
            assert!(why.contains("page 1 needs objects that cannot be read"), "{why}");
        });
    }
}
