//! Findings and reports: what every tier reports, and the verdict on a file.
//!
//! The checks themselves are data: `rust/checks.toml`, which `build.rs` validates and turns into one constant per check
//! and the list [`ALL`]. A check is named by its constant, so a typo does not compile. Every tier raises findings
//! against these constants, so they live here, below the tiers, and not in tier 8.
//!
//! A report is our own versioned JSON (`schema` 1), shaped so SARIF can be derived from it: a check's status maps to
//! SARIF's `kind`, a finding's severity to its `level`, the chain to logical locations and `caused_by` to related ones.

use serde::{Serialize, Serializer};

use crate::pipeline::EventId;

/// One standard and a clause number in it, such as ISO 15930-7, 6.4.3.3. Numbers only: no titles or text.
#[derive(Debug, PartialEq, Eq)]
pub struct Clause {
    /// The standard, as its clause list in `rust/clauses/` is named: `iso-15930-7`.
    pub standard: &'static str,
    /// The clause number: `6.4.3.3`, or an annex letter.
    pub number: &'static str,
}

/// How bad a failure of the check is, by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// The file fails.
    Error,
    /// Reported; the file still passes.
    Warning,
}

/// A PDF/X conformance level a check belongs to, or `Print` for proof-pdf's own print checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// PDF/X-4 (ISO 15930-7).
    X4,
    /// PDF/X-4p (ISO 15930-7, Annex A).
    X4p,
    /// PDF/X-5g (ISO 15930-8).
    X5g,
    /// PDF/X-5pg (ISO 15930-8).
    X5pg,
    /// PDF/X-5n (ISO 15930-8).
    X5n,
    /// PDF/X-1a:2001 (ISO 15930-1).
    X1a2001,
    /// PDF/X-3:2002 (ISO 15930-3).
    X32002,
    /// PDF/X-1a:2003 (ISO 15930-4).
    X1a2003,
    /// PDF/X-3:2003 (ISO 15930-6).
    X32003,
    /// A house check, not from a PDF/X standard.
    Print,
}

impl Level {
    /// Every level, in the order `checks.toml` and the README list them.
    pub const ALL: [Self; 10] = [
        Self::X4,
        Self::X4p,
        Self::X5g,
        Self::X5pg,
        Self::X5n,
        Self::X1a2001,
        Self::X32002,
        Self::X1a2003,
        Self::X32003,
        Self::Print,
    ];

    /// The level a command line or a profile names: `x4`, `x4p`, `x5g`, `x5pg`, `x5n`, `x1a-2001`, `x3-2002`,
    /// `x1a-2003`, `x3-2003` or `print`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.name() == name)
    }

    /// The name [`Level::from_name`] reads.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::X4 => "x4",
            Self::X4p => "x4p",
            Self::X5g => "x5g",
            Self::X5pg => "x5pg",
            Self::X5n => "x5n",
            Self::X1a2001 => "x1a-2001",
            Self::X32002 => "x3-2002",
            Self::X1a2003 => "x1a-2003",
            Self::X32003 => "x3-2003",
            Self::Print => "print",
        }
    }

    /// The standard that defines the level, as `rust/clauses/` names it.
    #[must_use]
    pub fn standard(self) -> &'static str {
        match self {
            Self::X4 | Self::X4p => "iso-15930-7",
            Self::X5g | Self::X5pg | Self::X5n => "iso-15930-8",
            Self::X1a2001 => "iso-15930-1",
            Self::X32002 => "iso-15930-3",
            Self::X1a2003 => "iso-15930-4",
            Self::X32003 => "iso-15930-6",
            Self::Print => "iso-32000-2",
        }
    }

    /// The level as its standard names it, `PDF/X-1a:2001`; `None` for the print checks.
    #[must_use]
    pub fn title(self) -> Option<&'static str> {
        Some(match self {
            Self::X4 => "PDF/X-4",
            Self::X4p => "PDF/X-4p",
            Self::X5g => "PDF/X-5g",
            Self::X5pg => "PDF/X-5pg",
            Self::X5n => "PDF/X-5n",
            Self::X1a2001 => "PDF/X-1a:2001",
            Self::X32002 => "PDF/X-3:2002",
            Self::X1a2003 => "PDF/X-1a:2003",
            Self::X32003 => "PDF/X-3:2003",
            Self::Print => return None,
        })
    }
}

/// Whether a check exists in code yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Checked, with a passing and a failing fixture.
    Implemented,
    /// Listed so the gap shows; nothing checks it yet.
    Todo,
    /// Deliberately not checked, for the reason given: a rule on readers or processes, or not a property of the file.
    Skip(&'static str),
}

/// A check, as `rust/checks.toml` declares it.
#[derive(Debug, PartialEq, Eq)]
pub struct Check {
    /// Stable, never reused.
    pub id: &'static str,
    /// Ids the check had before, still accepted.
    pub aliases: &'static [&'static str],
    /// What it checks, in a few words of our own.
    pub title: &'static str,
    /// The clauses its findings may cite.
    pub clauses: &'static [Clause],
    /// Its default severity.
    pub severity: Severity,
    /// The levels whose profiles include it.
    pub applies: &'static [Level],
    /// For an X-4 check reused at an older level: the level, an ISO 15930-7 clause number the check cites, and the
    /// clause of that level's standard a finding cites instead.
    pub cites: &'static [(Level, &'static str, Clause)],
    /// Whether it is checked yet.
    pub status: Status,
    /// The external program whose verdict it reports (`veraPDF`, `Ghostscript`), run by the command line; `None` for
    /// the core's own checks.
    pub tool: Option<&'static str>,
}

/// The Markdown listing of every check, as `rust/CHECKS.md` must hold it.
pub const CHECKS_MD: &str = include_str!(concat!(env!("OUT_DIR"), "/CHECKS.md"));

/// The README's section on checks, between its `<!-- checks -->` and `<!-- /checks -->` markers.
pub const README_CHECKS: &str = include_str!(concat!(env!("OUT_DIR"), "/README-checks.md"));

include!(concat!(env!("OUT_DIR"), "/checks.rs"));

impl Serialize for Clause {
    /// As `iso-15930-7:6.4.3.3`, the spelling `rust/checks.toml` uses.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{}:{}", self.standard, self.number))
    }
}

impl Serialize for Check {
    /// As its id: a report names checks, it does not repeat the catalogue.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.id)
    }
}

impl Check {
    /// The clause `number` among those this check may cite, or `None` when it may not cite it, which is a bug in the
    /// check that raised it: a debug build stops there, so a test catches it.
    #[must_use]
    pub fn clause(&self, number: &str) -> Option<&'static Clause> {
        let found = self.clauses.iter().find(|c| c.number == number);
        debug_assert!(found.is_some(), "{} may not cite clause {number}", self.id);
        found
    }
}

/// Where a finding is, one step of the path from the file down to the event it is about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Location {
    /// A byte offset in the buffer the rest of the chain names: the file, or an object stream's decoded data.
    Offset(usize),
    /// An indirect object, by number and generation.
    Object(u32, u16),
    /// A page, counted from 1.
    Page(u32),
    /// Another file of the file set, as it was found: the steps after it are in that file.
    File(String),
}

/// Something a check found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// The check that raised it.
    pub check: &'static Check,
    /// Error or warning; the check's default unless a profile changed it.
    pub severity: Severity,
    /// What is wrong and where, for a person.
    pub message: String,
    /// The clause it breaks, one of its check's.
    pub clause: Option<&'static Clause>,
    /// The event it is about, when it is about one.
    pub event: Option<EventId>,
    /// From the file down to the event.
    pub chain: Vec<Location>,
    /// The event that caused it, so a report can group one defect's consequences.
    pub caused_by: Option<EventId>,
    /// The page, from the chain, when it is on one.
    pub page: Option<u32>,
}

impl Finding {
    /// A finding of `check` at its default severity, about nothing in particular yet.
    #[must_use]
    pub fn new(check: &'static Check, message: impl Into<String>) -> Self {
        Self {
            check,
            severity: check.severity,
            message: message.into(),
            clause: None,
            event: None,
            chain: Vec::new(),
            caused_by: None,
            page: None,
        }
    }

    /// The same finding, citing clause `number` of its check.
    #[must_use]
    pub fn citing(mut self, number: &str) -> Self {
        self.clause = self.check.clause(number);
        self
    }

    /// The same finding, citing clause `number` of `standard` among its check's clauses: for a check whose clause
    /// numbers repeat across the standards of the levels it applies to.
    #[must_use]
    pub fn citing_in(mut self, standard: &str, number: &str) -> Self {
        let found = self
            .check
            .clauses
            .iter()
            .find(|c| c.standard == standard && c.number == number);
        debug_assert!(found.is_some(), "{} may not cite {standard}:{number}", self.check.id);
        self.clause = found;
        self
    }

    /// The same finding, located by `chain`; its page is the last page in the chain.
    #[must_use]
    pub fn at(mut self, chain: Vec<Location>) -> Self {
        self.page = chain.iter().rev().find_map(|l| match l {
            Location::Page(n) => Some(*n),
            _ => None,
        });
        self.chain = chain;
        self
    }
}

/// How one enabled check came out on a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "reason")]
pub enum Outcome {
    /// It ran and found nothing.
    Pass,
    /// It raised at least one finding.
    Fail,
    /// It could not be judged, for the reason given: a gap, a missing tool, a limit. Fails the file.
    NotEvaluated(String),
}

/// One enabled check and how it came out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CheckResult {
    /// The check.
    pub id: &'static Check,
    /// Its outcome.
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// Whether the file could be read: tiers 1 to 4 found nothing, read it with gaps, or refused it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Structure {
    /// Read whole.
    Sound,
    /// Read with gaps.
    Damaged,
    /// Not read at all.
    Refused,
}

/// The one verdict on a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// No error and nothing left unjudged.
    Pass,
    /// An error, or a check that could not be judged.
    Fail,
    /// The file could not be read.
    Refused,
}

/// Everything verifying one file produced, as written to JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    /// The JSON's version. A change that would break a reader raises it.
    pub schema: u32,
    /// The file, as it was named.
    pub file: String,
    /// Pass, fail or refused.
    pub verdict: Verdict,
    /// The PDF/X level the file was verified at, as its standard names it (`PDF/X-1a:2001`); absent for the print
    /// checks alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<&'static str>,
    /// Whether the file itself was sound, separate from what the checks thought of its content.
    pub structure: Structure,
    /// Every enabled check and its outcome, so a check that never ran still leaves a trace.
    pub checks: Vec<CheckResult>,
    /// Every finding, in the order they were raised.
    pub findings: Vec<Finding>,
}

impl Report {
    /// The report on `file`, with its verdict: refused when the structure is, otherwise a fail on any error finding or
    /// any check not evaluated. Warnings alone pass.
    #[must_use]
    pub fn new(
        file: impl Into<String>,
        structure: Structure,
        checks: Vec<CheckResult>,
        findings: Vec<Finding>,
    ) -> Self {
        let failed = findings.iter().any(|f| f.severity == Severity::Error)
            || checks.iter().any(|c| matches!(c.outcome, Outcome::NotEvaluated(_)));
        let verdict = match (structure, failed) {
            (Structure::Refused, _) => Verdict::Refused,
            (_, true) => Verdict::Fail,
            (_, false) => Verdict::Pass,
        };
        Self {
            schema: 1,
            file: file.into(),
            verdict,
            level: None,
            structure,
            checks,
            findings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_check_is_reachable_by_its_constant() {
        assert_eq!(X4_TRAPPED_PRESENT.id, "x4.trapped.present");
        assert_eq!(X4_TRAPPED_PRESENT.aliases, ["trapped"]);
        assert_eq!(
            X4_ICC_CMYK_NOT_INTENT_PROFILE.clauses,
            [Clause {
                standard: "iso-15930-7",
                number: "6.4.3.3"
            }]
        );
        assert!(ALL.iter().any(|c| c.applies.contains(&Level::Print)));
    }

    #[test]
    fn levels_by_name() {
        let names = [
            "x4", "x4p", "x5g", "x5pg", "x5n", "x1a-2001", "x3-2002", "x1a-2003", "x3-2003", "print",
        ];
        assert_eq!(names.map(Level::from_name), Level::ALL.map(Some));
        assert_eq!(Level::from_name("x3"), None);
        assert_eq!(Level::X1a2001.title(), Some("PDF/X-1a:2001"));
        assert_eq!(Level::Print.title(), None);
        let standards = Level::ALL.map(Level::standard);
        assert_eq!(
            standards,
            [
                "iso-15930-7",
                "iso-15930-7",
                "iso-15930-8",
                "iso-15930-8",
                "iso-15930-8",
                "iso-15930-1",
                "iso-15930-3",
                "iso-15930-4",
                "iso-15930-6",
                "iso-32000-2"
            ]
        );
    }

    #[test]
    fn the_readme_check_section_is_current() {
        let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../README.md")).unwrap_or_default();
        let section = readme
            .split_once("<!-- checks -->\n")
            .and_then(|(_, rest)| rest.split_once("<!-- /checks -->"))
            .map(|(section, _)| section);
        assert_eq!(
            section,
            Some(README_CHECKS),
            "README.md's check section is stale: run bin/rust/check.sh --fix"
        );
    }

    #[test]
    fn rust_checks_md_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/CHECKS.md");
        assert_eq!(
            std::fs::read_to_string(path).unwrap_or_default(),
            CHECKS_MD,
            "rust/CHECKS.md is stale: run bin/rust/check.sh --fix"
        );
    }

    #[test]
    #[should_panic(expected = "may not cite clause 6.9")]
    fn citing_a_clause_the_check_does_not_declare_is_a_bug() {
        let _ = Finding::new(X4_ICC_CMYK_NOT_INTENT_PROFILE, "x").citing("6.9");
    }

    #[test]
    fn the_page_is_the_last_page_in_the_chain() {
        let finding = Finding::new(X4_BOXES_TRIM_OR_ART, "x").at(vec![
            Location::Object(3, 0),
            Location::Page(2),
            Location::Offset(9),
        ]);
        assert_eq!(finding.page, Some(2));
        assert_eq!(
            Finding::new(X4_BOXES_TRIM_OR_ART, "x")
                .at(vec![Location::Offset(9)])
                .page,
            None
        );
    }

    fn result(check: &'static Check, outcome: Outcome) -> CheckResult {
        CheckResult { id: check, outcome }
    }

    #[test]
    fn the_verdict() {
        let error = || vec![Finding::new(X4_TRAPPED_PRESENT, "missing")];
        let warning = || vec![Finding::new(X4_BOXES_PREFER_TRIM, "ArtBox")];
        let gap = || vec![result(X4_XMP_TITLE, Outcome::NotEvaluated("gap".into()))];
        let verdict = |structure, checks, findings| Report::new("f.pdf", structure, checks, findings).verdict;
        assert_eq!(verdict(Structure::Sound, vec![], vec![]), Verdict::Pass);
        assert_eq!(verdict(Structure::Sound, vec![], warning()), Verdict::Pass);
        assert_eq!(verdict(Structure::Sound, vec![], error()), Verdict::Fail);
        assert_eq!(verdict(Structure::Damaged, gap(), vec![]), Verdict::Fail);
        assert_eq!(verdict(Structure::Refused, vec![], vec![]), Verdict::Refused);
        assert_eq!(verdict(Structure::Refused, gap(), error()), Verdict::Refused);
    }

    #[test]
    fn the_json_schema_1() {
        let event = EventId {
            doc: 0,
            seq: 41,
            tier: 5,
            synthetic: false,
        };
        let mut finding = Finding::new(
            X4_ICC_CMYK_NOT_INTENT_PROFILE,
            "the image uses the output intent's profile",
        )
        .citing("6.4.3.3")
        .at(vec![Location::Page(1), Location::Object(109, 0)]);
        finding.event = Some(event);
        finding.caused_by = Some(event);
        let report = Report::new(
            "altona.pdf",
            Structure::Damaged,
            vec![
                result(X4_ICC_CMYK_NOT_INTENT_PROFILE, Outcome::Fail),
                result(X4_TRAPPED_PRESENT, Outcome::Pass),
                result(
                    X4_XMP_TITLE,
                    Outcome::NotEvaluated("the metadata stream is a gap".into()),
                ),
            ],
            vec![finding, Finding::new(X4_BOXES_PREFER_TRIM, "page 2 uses an ArtBox")],
        );
        insta::assert_json_snapshot!(report);
    }
}
