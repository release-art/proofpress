//! The Python module `proof_pdf._internal._core`, built by maturin with the `python` feature. Python hands over whole files and
//! gets whole reports back; it never sees single operators.

use std::fmt::Display;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::checks;
use crate::findings::Level;

/// Verify `data`, the bytes of the file named `file`, at `level` (`x4`, `x4p`, `x5g`, `x5pg`, `x5n` or `print`), with
/// the print checks' parameters given as a JSON object (`params`, by name; any left out keep their defaults), and return
/// the report as JSON (schema 1). The GIL is released while the file is read.
#[pyfunction]
#[pyo3(signature = (file, data, level, params = "{}"))]
fn verify(py: Python<'_>, file: &str, data: &[u8], level: &str, params: &str) -> PyResult<String> {
    let level = Level::from_name(level).ok_or_else(|| value_error(format_args!("unknown level {level:?}")))?;
    let params = serde_json::from_str(params).map_err(|e| value_error(format_args!("bad params: {e}")))?;
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

/// A `ValueError` saying `message`.
fn value_error(message: impl Display) -> PyErr {
    PyValueError::new_err(message.to_string())
}

/// The module itself, `proof_pdf._internal._core` to Python.
#[pymodule]
mod _core {
    #[pymodule_export]
    use super::{run, verify};
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
            assert!(error.to_string().contains("bad params"), "{error}");
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
}
