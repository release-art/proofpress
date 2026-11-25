//! The Python module `proof_pdf._internal._core`, built by maturin with the `python` feature. Python hands over whole files and
//! gets whole reports back; it never sees single operators.

use std::fmt::Display;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::checks;
use crate::findings::Level;

/// Verify `data`, the bytes of the file named `file`, at `level` (`x4`, `x4p`, `x5g`, `x5pg` or `x5n`), and return the
/// report as JSON (schema 1). The GIL is released while the file is read.
#[pyfunction]
fn verify(py: Python<'_>, file: &str, data: &[u8], level: &str) -> PyResult<String> {
    let level = Level::from_name(level).ok_or_else(|| value_error(format_args!("unknown level {level:?}")))?;
    let report = py.detach(|| checks::verify(file, data, level));
    serde_json::to_string(&report).map_err(value_error)
}

/// A `ValueError` saying `message`.
fn value_error(message: impl Display) -> PyErr {
    PyValueError::new_err(message.to_string())
}

/// The module itself, `proof_pdf._internal._core` to Python.
#[pymodule]
mod _core {
    #[pymodule_export]
    use super::verify;
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
        });
    }
}
