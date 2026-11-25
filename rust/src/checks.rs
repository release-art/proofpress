//! Tier 8: the checks, each a first-order listener chosen per profile. What each check is, its id, title and clauses,
//! is data in `rust/checks.toml`, published as constants in [`crate::findings`].

use crate::findings::{Level, Report, Structure};

/// Verify `bytes`, the file named `file`, at `level`.
///
/// ponytail: no tier reads the file yet, so nothing is judged and the structure is taken as sound; phase 2 of
/// `docs/briefs/pdfx-plan.md` brings tiers 1 to 4, and each check joins the report as it is implemented.
#[must_use]
pub fn verify(file: &str, bytes: &[u8], level: Level) -> Report {
    let _ = (bytes, level);
    Report::new(file, Structure::Sound, Vec::new(), Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::findings::Verdict;

    #[test]
    fn nothing_is_judged_yet() {
        let report = verify("x.pdf", b"%PDF-1.7", Level::X4);
        assert_eq!(
            (report.verdict, report.checks.len(), report.findings.len()),
            (Verdict::Pass, 0, 0)
        );
    }
}
