//! Reads `checks.toml`, refuses it if any entry is malformed, and writes the check table (`$OUT_DIR/checks.rs`) and
//! its Markdown listing (`$OUT_DIR/CHECKS.md`). The generated code is data only, with no branches: every check is a
//! constant, so naming a check that does not exist fails to compile.

// A build script reports a malformed checks.toml, or an unreadable file, by failing the build, and panicking is how it
// fails. The no-panic rule protects PDF input, which never reaches this file.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a build script fails the build by panicking"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    check: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    #[serde(default)]
    aliases: Vec<String>,
    title: String,
    clauses: Vec<String>,
    severity: String,
    applies: Vec<String>,
    status: String,
    reason: Option<String>,
}

const SEVERITIES: &[&str] = &["error", "warning"];
const LEVELS: &[&str] = &["x4", "x4p", "x5g", "x5pg", "x5n", "print"];

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let source = root.join("checks.toml");
    println!("cargo::rerun-if-changed={}", source.display());
    println!("cargo::rerun-if-changed={}", root.join("clauses").display());
    let text = fs::read_to_string(&source).expect("rust/checks.toml is readable");
    let file: File = toml::from_str(&text).unwrap_or_else(|e| panic!("rust/checks.toml: {e}"));
    let known = known_clauses(&root.join("clauses"));
    let problems = validate(&file.check, &known);
    assert!(problems.is_empty(), "rust/checks.toml:\n{}", problems.join("\n"));
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    fs::write(out.join("checks.rs"), code(&file.check)).expect("OUT_DIR is writable");
    fs::write(out.join("CHECKS.md"), markdown(&file.check)).expect("OUT_DIR is writable");
}

/// The clause numbers held per standard: `clauses/<standard>.txt`, one number per line.
fn known_clauses(dir: &Path) -> BTreeSet<String> {
    let mut known = BTreeSet::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(standard) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().is_some_and(|e| e == "txt") {
            let text = fs::read_to_string(&path).expect("clause lists are readable");
            known.extend(
                text.lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| format!("{standard}:{l}")),
            );
        }
    }
    known
}

fn validate(checks: &[Entry], known: &BTreeSet<String>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut names = BTreeSet::new();
    for c in checks {
        let id = &c.id;
        for name in std::iter::once(id).chain(&c.aliases) {
            if !names.insert(name.clone()) {
                problems.push(format!("{name}: used twice, as an id or an alias"));
            }
            if name.is_empty()
                || !name
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || "-.".contains(ch))
            {
                problems.push(format!("{name}: ids are lower case letters, digits, '-' and '.'"));
            }
        }
        if c.title.is_empty() {
            problems.push(format!("{id}: no title"));
        }
        if !SEVERITIES.contains(&c.severity.as_str()) {
            problems.push(format!("{id}: severity {:?} is not one of {SEVERITIES:?}", c.severity));
        }
        if c.applies.is_empty() || c.applies.iter().any(|a| !LEVELS.contains(&a.as_str())) {
            problems.push(format!("{id}: applies {:?} must name some of {LEVELS:?}", c.applies));
        }
        for clause in &c.clauses {
            if !known.contains(clause) {
                problems.push(format!("{id}: clause {clause} is not in rust/clauses/"));
            }
        }
        match (c.status.as_str(), &c.reason) {
            ("implemented" | "todo", None) => {}
            ("skip", Some(reason)) if !reason.is_empty() => {}
            ("skip", _) => problems.push(format!("{id}: a skip needs a reason")),
            ("implemented" | "todo", Some(_)) => problems.push(format!("{id}: only a skip has a reason")),
            (other, _) => problems.push(format!("{id}: status {other:?} is not implemented, todo or skip")),
        }
    }
    problems
}

/// `x4.output-intent.present` → `X4_OUTPUT_INTENT_PRESENT`.
fn constant(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn variant(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_ascii_uppercase().to_string() + chars.as_str())
}

fn code(checks: &[Entry]) -> String {
    let mut out = String::from("// Generated by build.rs from rust/checks.toml. Do not edit.\n\n");
    for c in checks {
        let clauses: Vec<String> = c
            .clauses
            .iter()
            .map(|cl| {
                let (standard, number) = cl.split_once(':').expect("validated: every clause is known");
                format!("Clause {{ standard: {standard:?}, number: {number:?} }}")
            })
            .collect();
        let applies: Vec<String> = c.applies.iter().map(|a| format!("Level::{}", variant(a))).collect();
        let status = match (c.status.as_str(), &c.reason) {
            ("skip", Some(reason)) => format!("Status::Skip({reason:?})"),
            (status, _) => format!("Status::{}", variant(status)),
        };
        let _ = writeln!(
            out,
            "/// The `{id}` check.\npub const {name}: &Check = &Check {{\n    id: {id:?},\n    aliases: &{aliases:?},\n    title: {title:?},\n    \
             clauses: &[{clauses}],\n    severity: Severity::{severity},\n    applies: &[{applies}],\n    status: {status},\n}};\n",
            title = c.title,
            name = constant(&c.id),
            id = c.id,
            aliases = c.aliases,
            clauses = clauses.join(", "),
            severity = variant(&c.severity),
            applies = applies.join(", "),
        );
    }
    let all: Vec<String> = checks.iter().map(|c| constant(&c.id)).collect();
    let _ = writeln!(
        out,
        "/// Every check, in the order of `rust/checks.toml`.\npub const ALL: &[&Check] = &[{}];",
        all.join(", ")
    );
    out
}

fn markdown(checks: &[Entry]) -> String {
    let mut out = String::from(
        "# Checks\n\nGenerated from `rust/checks.toml` by `rust/build.rs`; `cargo test` fails while this copy is stale \
         (refresh it with `bin/rust/check.sh --fix`).\n\n| Id | Title | Clauses | Severity | Applies | Status |\n|---|---|---|---|---|---|\n",
    );
    for c in checks {
        let status = match &c.reason {
            Some(reason) => format!("skip: {reason}"),
            None => c.status.clone(),
        };
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} |",
            c.id,
            c.title,
            c.clauses.join(", "),
            c.severity,
            c.applies.join(", "),
            status
        );
    }
    out
}
