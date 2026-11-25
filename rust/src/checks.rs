//! Tier 8: the checks, each a first-order listener chosen per profile. What each check is, its id, title and clauses,
//! is data in `rust/checks.toml`, published as constants in [`crate::findings`].

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::document::Document;
use crate::findings::{Check, Finding, INTERNAL, Level, Report, Structure};
use crate::pipeline::Limits;

/// Verify `bytes`, the file named `file`, at `level`: open it, reading everything reachable, and report what tiers 1 to
/// 5 found. The checks of tiers 6 to 8 join as they are written.
#[must_use]
pub fn verify(file: &str, bytes: &[u8], level: Level) -> Report {
    let _ = level;
    match Document::open(bytes, &Limits::default()) {
        Err(refusal) => Report::new(file, Structure::Refused, Vec::new(), vec![*refusal]),
        Ok(doc) => {
            let structure = if doc.damaged() {
                Structure::Damaged
            } else {
                Structure::Sound
            };
            Report::new(file, structure, Vec::new(), doc.findings())
        }
    }
}

/// Run one check, turning a panic into an `internal` finding so the other checks still run and nothing panics into the
/// caller (`PyO3` would raise `PanicException`, which `except Exception` does not catch). Unwinding is safe here: a check
/// reads the frozen object table and owns everything it writes.
pub fn guarded(check: &'static Check, run: impl FnOnce() -> Vec<Finding>) -> Vec<Finding> {
    catch_unwind(AssertUnwindSafe(run)).unwrap_or_else(|panic| {
        let reason = panic
            .downcast_ref::<&str>()
            .map(ToString::to_string)
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        vec![Finding::new(
            INTERNAL,
            format!("check {} failed inside proof-pdf: {reason}", check.id),
        )]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::findings::{Verdict, X4_TRAPPED_PRESENT};

    #[test]
    fn a_check_that_returns_is_left_alone() {
        let found = guarded(X4_TRAPPED_PRESENT, || vec![Finding::new(X4_TRAPPED_PRESENT, "missing")]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].check.id, "x4.trapped.present");
    }

    fn boom(payload: impl std::any::Any + Send) -> Vec<Finding> {
        std::panic::panic_any(payload)
    }

    #[test]
    fn a_panic_becomes_an_internal_finding() {
        let said = |found: Vec<Finding>| {
            assert_eq!((found.len(), found[0].check.id), (1, "internal"));
            found[0].message.clone()
        };
        let prefix = "check x4.trapped.present failed inside proof-pdf: ";
        assert_eq!(
            said(guarded(X4_TRAPPED_PRESENT, || boom("static"))),
            format!("{prefix}static")
        );
        assert_eq!(
            said(guarded(X4_TRAPPED_PRESENT, || boom(String::from("owned")))),
            format!("{prefix}owned")
        );
        assert_eq!(said(guarded(X4_TRAPPED_PRESENT, || boom(7_u8))), prefix);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_refused() {
        let report = verify("x.pdf", b"%PDF-1.7", Level::X4);
        assert_eq!(
            (report.verdict, report.structure),
            (Verdict::Refused, Structure::Refused)
        );
        assert_eq!(report.findings[0].check.id, "file.startxref");
    }

    #[test]
    fn a_damaged_object_makes_the_file_damaged() {
        let body = b"%PDF-1.7\n1 0 obj\n<< /A 2 0 R >>\nendobj\n2 0 obj\n[1\nendobj\n";
        let mut file = body.to_vec();
        file.extend(format!("xref\n0 3\n0000000000 65535 f\r\n0000000009 00000 n\r\n0000000039 00000 n\r\ntrailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", body.len()).as_bytes());
        let report = verify("x.pdf", &file, Level::X4);
        assert_eq!((report.verdict, report.structure), (Verdict::Fail, Structure::Damaged));
        assert_eq!(report.findings[0].check.id, "object.damaged");
    }

    #[test]
    fn the_genuine_fixture_is_sound() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/pdfs/x4-good.pdf"
        ))
        .unwrap();
        let report = verify("x4-good.pdf", &bytes, Level::X4);
        assert_eq!(
            (report.verdict, report.structure),
            (Verdict::Pass, Structure::Sound),
            "{:#?}",
            report.findings
        );
    }
}
