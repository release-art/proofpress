//! The completeness gate (pdfx-plan task 1.3): every check in `rust/checks.toml` is accounted for, and one marked
//! `implemented` has a fixture that fails it and one that passes it in `tests/fixtures/pdfx/expected.toml`.
//! Run with `--nocapture` to see the register coverage.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use proof_pdf::checks::{Files, Params, Settings, verify_settings};
use proof_pdf::findings::{ALL, Check, Level, Outcome, Status, Structure};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    levels: Vec<String>,
    expect: BTreeMap<String, String>,
    /// The print checks' parameters it is verified with, when not the defaults.
    #[serde(default)]
    params: Params,
    /// Verified alone, not with the files beside it (ISO 15930-7 A.1).
    #[serde(default)]
    alone: bool,
    /// The checks it leaves not evaluated, each with why; none otherwise.
    #[serde(default)]
    unjudged: BTreeMap<String, String>,
}

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn fixtures() -> BTreeMap<String, Fixture> {
    let path = repo().join("tests/fixtures/pdfx/expected.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn level(name: &str) -> Level {
    Level::from_name(name).unwrap_or_else(|| panic!("unknown level {name:?} in expected.toml"))
}

/// Whether `fixture` is verified at a level `check` applies to.
fn judges(fixture: &Fixture, check: &Check) -> bool {
    fixture.levels.iter().any(|l| check.applies.contains(&level(l)))
}

#[test]
fn every_fixture_exists_and_names_only_known_checks() {
    let known: BTreeSet<&str> = ALL.iter().map(|c| c.id).collect();
    for (name, fixture) in fixtures() {
        let file = format!("{name}.pdf");
        assert!(
            repo().join("tests/fixtures/pdfs").join(&file).is_file()
                || repo().join("tests/fixtures/pdfx").join(&file).is_file(),
            "{file} is in expected.toml but not in tests/fixtures/pdfs or pdfx"
        );
        assert!(!fixture.levels.is_empty(), "{name}: no levels");
        for (id, reason) in &fixture.expect {
            assert!(known.contains(id.as_str()), "{name}: {id} is not in checks.toml");
            assert!(!reason.is_empty(), "{name}: {id} needs a reason");
        }
    }
}

#[test]
fn an_implemented_check_has_a_failing_and_a_passing_fixture() {
    let fixtures = fixtures();
    let mut missing = Vec::new();
    // A check that reports an external tool's verdict is tested with stand-in tools, by the command line's tests.
    for check in ALL
        .iter()
        .filter(|c| c.status == Status::Implemented && c.tool.is_none())
    {
        let fails = fixtures.values().any(|f| f.expect.contains_key(check.id));
        let passes = fixtures
            .values()
            .any(|f| judges(f, check) && !f.expect.contains_key(check.id) && !f.unjudged.contains_key(check.id));
        if !fails || !passes {
            missing.push(format!("{} (fail fixture: {fails}, pass fixture: {passes})", check.id));
        }
    }
    assert!(
        missing.is_empty(),
        "implemented without fixtures:\n{}",
        missing.join("\n")
    );
}

#[test]
fn register_coverage() {
    let mut counts: BTreeMap<&str, [usize; 3]> = BTreeMap::new();
    for check in ALL {
        let group = if check.applies.contains(&Level::Print) {
            "print"
        } else if check.id.starts_with("x5.") {
            "PDF/X-5"
        } else if ["x13.", "x1a.", "x3."].iter().any(|p| check.id.starts_with(p)) {
            "PDF/X-1a, X-3"
        } else {
            "PDF/X-4"
        };
        let slot = match check.status {
            Status::Implemented => 0,
            Status::Todo => 1,
            Status::Skip(_) => 2,
        };
        counts.entry(group).or_default()[slot] += 1;
    }
    println!("register coverage (implemented, todo, skip):");
    for (group, [implemented, todo, skip]) in &counts {
        println!("  {group:8} {implemented:4} {todo:4} {skip:4}");
    }
    assert_eq!(
        counts.values().map(|c| c.iter().sum::<usize>()).sum::<usize>(),
        ALL.len()
    );
}

/// Every fixture raises exactly the checks expected.toml lists for it, at every level it names.
#[test]
fn every_fixture_raises_exactly_its_expected_checks() {
    let mut wrong = Vec::new();
    for (name, fixture) in fixtures() {
        let file = format!("{name}.pdf");
        let path = [
            repo().join("tests/fixtures/pdfs").join(&file),
            repo().join("tests/fixtures/pdfx").join(&file),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        for name_of_level in &fixture.levels {
            let folder = path.parent().unwrap().to_owned();
            let settings = Settings {
                level: level(name_of_level),
                params: fixture.params.clone(),
                files: if fixture.alone {
                    Files::Alone
                } else {
                    Files::Set(folder)
                },
            };
            let report = verify_settings(&file, &bytes, &settings);
            let found: BTreeSet<&str> = report.findings.iter().map(|f| f.check.id).collect();
            let expected: BTreeSet<&str> = fixture.expect.keys().map(String::as_str).collect();
            if found != expected {
                wrong.push(format!(
                    "{name} at {name_of_level}: expected {expected:?}, found {found:?}"
                ));
            }
            let unjudged: BTreeSet<&str> = report
                .checks
                .iter()
                .filter(|c| matches!(c.outcome, Outcome::NotEvaluated(_)) && c.id.status == Status::Implemented)
                .map(|c| c.id.id)
                .collect();
            let expected: BTreeSet<&str> = fixture.unjudged.keys().map(String::as_str).collect();
            // On a refused or damaged file every check is left unjudged, and the structure says why.
            if report.structure == Structure::Sound && unjudged != expected {
                wrong.push(format!(
                    "{name} at {name_of_level}: expected not evaluated {expected:?}, found {unjudged:?}"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
