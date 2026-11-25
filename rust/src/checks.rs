//! Tier 8: the checks, each a first-order listener chosen per profile.
//!
//! What each check is comes from `rust/checks.toml`, which `build.rs` validates and turns into one constant per check
//! and the list [`ALL`]. The table is data, not code: a check is named by its constant, so a typo does not compile.

/// One standard and a clause number in it, such as ISO 15930-7, 6.4.3.3. Numbers only: no titles or text.
#[derive(Debug, PartialEq, Eq)]
pub struct Clause {
    /// The standard, as its clause list in `rust/clauses/` is named: `iso-15930-7`.
    pub standard: &'static str,
    /// The clause number: `6.4.3.3`, or an annex letter.
    pub number: &'static str,
}

/// How bad a failure of the check is, by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// A house check, not from a PDF/X standard.
    Print,
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
    /// Whether it is checked yet.
    pub status: Status,
}

/// The Markdown listing of every check, as `rust/CHECKS.md` must hold it.
pub const CHECKS_MD: &str = include_str!(concat!(env!("OUT_DIR"), "/CHECKS.md"));

include!(concat!(env!("OUT_DIR"), "/checks.rs"));

#[cfg(test)]
mod tests {
    use super::*;

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
    fn rust_checks_md_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/CHECKS.md");
        assert_eq!(
            std::fs::read_to_string(path).unwrap_or_default(),
            CHECKS_MD,
            "rust/CHECKS.md is stale: run bin/rust/check.sh --fix"
        );
    }
}
