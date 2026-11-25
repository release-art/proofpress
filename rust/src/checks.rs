//! Tier 8: the checks, each a first-order listener chosen per profile. What each check is, its id, title and clauses,
//! is data in `rust/checks.toml`, published as constants in [`crate::findings`].

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::findings::{Check, Finding, INTERNAL, Level, Report, Structure};

/// Verify `bytes`, the file named `file`, at `level`.
///
/// ponytail: no tier reads the file yet, so nothing is judged and the structure is taken as sound; phase 2 of
/// `docs/briefs/pdfx-plan.md` brings tiers 1 to 4, and each check joins the report as it is implemented.
#[must_use]
pub fn verify(file: &str, bytes: &[u8], level: Level) -> Report {
    let _ = (bytes, level);
    Report::new(file, Structure::Sound, Vec::new(), Vec::new())
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
    fn nothing_is_judged_yet() {
        let report = verify("x.pdf", b"%PDF-1.7", Level::X4);
        assert_eq!(
            (report.verdict, report.checks.len(), report.findings.len()),
            (Verdict::Pass, 0, 0)
        );
    }
}
