//! Tier 8: the checks, each a first-order listener chosen per profile. What each check is, its id, title and clauses,
//! is data in `rust/checks.toml`, published as constants in [`crate::findings`].

use std::panic::{AssertUnwindSafe, catch_unwind};

mod metadata;
mod x4;

use crate::document::Document;
use crate::findings::{
    self as f, Check, CheckResult, FILE_ENCRYPTED, Finding, INTERNAL, Level, Outcome, Report, Severity, Status,
    Structure, X4_ENCRYPT_ABSENT, X4_FILTER_NO_CRYPT, X4_FILTER_NO_LZW, X4_FILTER_STANDARD_ONLY, X4_LIMITS_OBJECTS,
    X4_NAMES_HEX_ESCAPED, X4_NAMES_UTF8, X4_PDF_REFERENCE_CONFORMS, X4_PERMS_UR_ONLY,
};
use crate::pipeline::Limits;

/// A check's code: what it finds in an open document verified at a level.
type Run = fn(&Document, Level) -> Vec<Finding>;

/// Every implemented check that reads an open document, with its code. A test holds this list and the
/// `implemented` entries of `rust/checks.toml` equal, apart from the checks judged from tiers 1 to 5's own findings.
const RUNS: &[(&Check, Run)] = &[
    (X4_FILTER_NO_LZW, |d, _| x4::no_lzw(d)),
    (X4_FILTER_NO_CRYPT, |d, _| x4::no_crypt(d)),
    (X4_FILTER_STANDARD_ONLY, |d, _| x4::standard_only(d)),
    (X4_PERMS_UR_ONLY, |d, _| x4::ur_only(d)),
    (X4_LIMITS_OBJECTS, |d, _| x4::object_count(d)),
    (X4_NAMES_UTF8, |d, _| x4::names_utf8(d)),
    (X4_NAMES_HEX_ESCAPED, |d, _| x4::names_escaped(d)),
    (f::X4_TRAPPED_PRESENT, metadata::trapped_present),
    (f::X4_TRAPPED_TRUE_OR_FALSE, metadata::trapped_true_or_false),
    (f::X4_XMP_PRESENT, metadata::present),
    (f::X4_XMP_NAMESPACE_PREFIXES, metadata::prefixes),
    (f::X4_XMP_UNFILTERED, metadata::unfiltered),
    (f::X4_INFO_XMP_ANALOGUE_PRESENT, metadata::analogue_present),
    (f::X4_INFO_XMP_ANALOGUE_EQUAL, metadata::analogue_equal),
    (f::X4_INFO_PDFX_VERSION_PDFDOC, metadata::info_version_pdfdoc),
    (f::X4_INFO_AUTHOR_SINGLE_CREATOR, metadata::author_single_creator),
    (f::X4_XMP_NO_BYTES_ENCODING_ATTRS, metadata::header_attributes),
    (f::X4_XMP_DOCUMENT_ID, metadata::document_id),
    (f::X4_XMP_VERSION_ID, metadata::version_id),
    (f::X4_XMP_RENDITION_CLASS, metadata::rendition_class),
    (f::X4_XMP_DOCUMENT_ID_UUID, metadata::document_id_uuid),
    (f::X4_XMP_CREATE_DATE, metadata::create_date),
    (f::X4_XMP_MODIFY_DATE, metadata::modify_date),
    (f::X4_XMP_METADATA_DATE, metadata::metadata_date),
    (f::X4_XMP_TITLE, metadata::title),
    (f::X4_XMP_AGENT_NAMES, metadata::agent_names),
    (f::X4_TRAILER_ID, metadata::trailer_id),
    (f::X4_XMP_WELL_FORMED, metadata::well_formed),
    (f::X4_IDENTITY_VERSION_PRESENT, metadata::version_present),
    (f::X4_IDENTITY_VERSION_VALUE, metadata::version_value),
    (f::X4_IDENTITY_PDFXID_ONLY_VERSION, metadata::pdfxid_only_version),
    (f::X4_IDENTITY_NO_INFO_CONFORMANCE, metadata::no_info_conformance),
];

/// Verify `bytes`, the file named `file`, at `level`: open it, reading everything reachable, report what tiers 1 to 5
/// found, and run every implemented check of the level. A check whose scope holds a gap is not evaluated, and on a
/// refused file nothing is.
#[must_use]
pub fn verify(file: &str, bytes: &[u8], level: Level) -> Report {
    let applies = |check: &Check| check.applies.contains(&level) && check.status == Status::Implemented;
    let enabled = RUNS.iter().filter(|(check, _)| applies(check));
    match Document::open(bytes, &Limits::default()) {
        Err(refusal) => {
            let unread = enabled.map(|(id, _)| CheckResult {
                id,
                outcome: Outcome::NotEvaluated("the file was refused".into()),
            });
            judged_from_structure(file, Structure::Refused, unread.collect(), vec![*refusal], &applies)
        }
        Ok(doc) => {
            let structure = if doc.damaged() {
                Structure::Damaged
            } else {
                Structure::Sound
            };
            let mut findings = doc.findings();
            let mut checks = Vec::new();
            for (id, run) in enabled {
                let outcome = if structure == Structure::Damaged {
                    Outcome::NotEvaluated("objects it would read could not be read".into())
                } else {
                    let found = guarded(id, &|| run(&doc, level));
                    let outcome = if found.is_empty() { Outcome::Pass } else { Outcome::Fail };
                    findings.extend(found);
                    outcome
                };
                checks.push(CheckResult { id, outcome });
            }
            judged_from_structure(file, structure, checks, findings, &applies)
        }
    }
}

/// The report, with the checks judged from tiers 1 to 5's findings added: `x4.encrypt.absent` fails on an encrypted
/// file, and `x4.pdf-reference.conforms` fails on every error those tiers found (6.1.1), naming it.
fn judged_from_structure(
    file: &str,
    structure: Structure,
    mut checks: Vec<CheckResult>,
    mut findings: Vec<Finding>,
    applies: &dyn Fn(&Check) -> bool,
) -> Report {
    let encrypted = findings.iter().any(|f| f.check == FILE_ENCRYPTED);
    if applies(X4_ENCRYPT_ABSENT) {
        let outcome = if encrypted { Outcome::Fail } else { Outcome::Pass };
        if encrypted {
            findings.push(Finding::new(X4_ENCRYPT_ABSENT, "the file is encrypted").citing("6.15"));
        }
        checks.push(CheckResult {
            id: X4_ENCRYPT_ABSENT,
            outcome,
        });
    }
    if applies(X4_PDF_REFERENCE_CONFORMS) {
        let invalid: Vec<Finding> = findings
            .iter()
            .filter(|f| {
                f.severity == Severity::Error && f.check.applies.contains(&Level::Print) && f.check != FILE_ENCRYPTED
            })
            .map(|f| {
                let mut conforms = Finding::new(
                    X4_PDF_REFERENCE_CONFORMS,
                    format!("not valid PDF ({}): {}", f.check.id, f.message),
                )
                .citing("6.1.1")
                .at(f.chain.clone());
                conforms.caused_by = f.event;
                conforms
            })
            .collect();
        let outcome = match (encrypted, invalid.is_empty()) {
            (true, true) => Outcome::NotEvaluated("the file is encrypted, so its objects cannot be read".into()),
            (_, true) => Outcome::Pass,
            (_, false) => Outcome::Fail,
        };
        findings.extend(invalid);
        checks.push(CheckResult {
            id: X4_PDF_REFERENCE_CONFORMS,
            outcome,
        });
    }
    Report::new(file, structure, checks, findings)
}

/// Run one check, turning a panic into an `internal` finding so the other checks still run and nothing panics into the
/// caller (`PyO3` would raise `PanicException`, which `except Exception` does not catch). Unwinding is safe here: a check
/// reads the frozen object table and owns everything it writes.
pub fn guarded(check: &'static Check, run: &dyn Fn() -> Vec<Finding>) -> Vec<Finding> {
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

    /// Every implemented PDF/X check has code: a run in `RUNS`, or a verdict judged from tiers 1 to 5's findings. House
    /// checks (`print`) are raised by the tiers themselves.
    #[test]
    fn every_implemented_check_has_code() {
        let judged = [X4_ENCRYPT_ABSENT, X4_PDF_REFERENCE_CONFORMS];
        for check in crate::findings::ALL.iter().filter(|c| c.status == Status::Implemented) {
            let run = RUNS.iter().any(|(c, _)| c == check);
            let (judged, tiers) = (judged.contains(check), check.applies.contains(&Level::Print));
            assert!(
                run | judged | tiers,
                "{} is implemented in checks.toml but has no code",
                check.id
            );
        }
        for (check, _) in RUNS {
            assert_eq!(
                check.status,
                Status::Implemented,
                "{} has code but checks.toml says otherwise",
                check.id
            );
        }
    }

    #[test]
    fn valid_pdf_is_judged_from_the_tiers_errors() {
        let all = |_: &Check| true;
        let outcome = |findings: Vec<Finding>| {
            let report = judged_from_structure("x.pdf", Structure::Sound, Vec::new(), findings, &all);
            let conforms = report
                .checks
                .into_iter()
                .find(|c| c.id == X4_PDF_REFERENCE_CONFORMS)
                .unwrap();
            let raised = report
                .findings
                .iter()
                .filter(|f| f.check == X4_PDF_REFERENCE_CONFORMS)
                .count();
            (conforms.outcome, raised)
        };
        let damaged = Finding::new(crate::findings::OBJECT_DAMAGED, "object 2 cannot be read");
        assert_eq!(outcome(vec![damaged]), (Outcome::Fail, 1));
        let warning = Finding::new(crate::findings::SYNTAX_DUPLICATE_KEY, "a key twice");
        assert_eq!(outcome(vec![warning]), (Outcome::Pass, 0));
        let encrypted = Finding::new(FILE_ENCRYPTED, "encrypted");
        let why = "the file is encrypted, so its objects cannot be read".to_owned();
        assert_eq!(outcome(vec![encrypted]), (Outcome::NotEvaluated(why), 0));
    }

    #[test]
    fn a_failing_check_fails_the_file() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/pdfs/x4-lzw.pdf"
        ))
        .unwrap();
        let report = verify("x4-lzw.pdf", &bytes, Level::X4);
        let lzw = report.checks.iter().find(|c| c.id == X4_FILTER_NO_LZW).unwrap();
        assert_eq!((report.verdict, &lzw.outcome), (Verdict::Fail, &Outcome::Fail));
    }

    #[test]
    fn a_level_runs_only_its_own_checks() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/pdfs/x4-good.pdf"
        ))
        .unwrap();
        assert_eq!(verify("x4-good.pdf", &bytes, Level::X5g).checks, []);
        assert_eq!(verify("x4-good.pdf", &bytes, Level::X4).checks.len(), RUNS.len() + 1);
    }

    #[test]
    fn a_check_that_returns_is_left_alone() {
        let found = guarded(X4_TRAPPED_PRESENT, &|| {
            vec![Finding::new(X4_TRAPPED_PRESENT, "missing")]
        });
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
            said(guarded(X4_TRAPPED_PRESENT, &|| boom("static"))),
            format!("{prefix}static")
        );
        assert_eq!(
            said(guarded(X4_TRAPPED_PRESENT, &|| boom(String::from("owned")))),
            format!("{prefix}owned")
        );
        assert_eq!(said(guarded(X4_TRAPPED_PRESENT, &|| boom(7_u8))), prefix);
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
