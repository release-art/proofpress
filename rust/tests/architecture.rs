//! Tiers depend only downwards. Each top-level module may reach only the modules listed for it here; a `crate::x`
//! path to anything else fails. A new module must be added to the map, which forces the decision. See "Code
//! conventions" in `docs/notes/rust-architecture.md`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Each module and the modules it may use. `pipeline` is the base every tier publishes through, and `findings` (the
/// check catalogue, findings and reports) sits on it, since every tier raises findings.
const ALLOWED: &[(&str, &[&str])] = &[
    ("pipeline", &[]),
    ("findings", &["pipeline"]),
    ("syntax", &["pipeline", "findings"]),
    ("filters", &["pipeline", "findings"]),
    ("file", &["pipeline", "findings", "syntax", "filters"]),
    // Document structure and the helpers use each other: a page needs rectangles, and rectangles are read from objects.
    (
        "document",
        &["pipeline", "findings", "syntax", "filters", "file", "common"],
    ),
    ("common", &["pipeline", "findings", "document"]),
    (
        "content",
        &["pipeline", "findings", "syntax", "filters", "document", "common"],
    ),
    (
        "graphics",
        &["pipeline", "findings", "filters", "document", "common", "content"],
    ),
    // A tier's own validators live in its module; `checks` holds the print checks on tiers 5 to 7 only.
    (
        "checks",
        &["pipeline", "findings", "document", "common", "content", "graphics"],
    ),
    // The Python module sits beside the tiers: it hands whole files to the checks and reports back.
    ("python", &["findings", "checks", "cli", "writer", "preview", "extract"]),
    // The command line, like the Python module, hands whole files to the checks and reports back.
    ("cli", &["findings", "checks"]),
    // What `extract` recovers, read from tier 5 and the helpers.
    (
        "extract",
        &["pipeline", "findings", "document", "common", "graphics", "checks"],
    ),
    // Previews read files with hayro's own parser, never the tiers: a picture judges nothing.
    ("preview", &[]),
    // The binaries are separate crates: they reach the library as `proof_pdf::`, never by `crate::` paths.
    ("bin", &[]),
    (
        "writer",
        &[
            "pipeline", "findings", "syntax", "filters", "document", "common", "content",
        ],
    ),
];

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The module's own file and, if it has become a directory, everything under it.
fn files_of(module: &str) -> Vec<PathBuf> {
    let mut files = vec![src().join(format!("{module}.rs"))];
    let mut dirs = vec![src().join(module)];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    files.retain(|f| f.exists());
    files
}

/// Every `crate::name` the text refers to.
fn crate_paths(text: &str) -> BTreeSet<String> {
    text.match_indices("crate::")
        .map(|(at, m)| {
            text[at + m.len()..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect()
        })
        .collect()
}

#[test]
fn every_module_is_in_the_map() {
    let declared: BTreeSet<String> = fs::read_dir(src())
        .unwrap()
        .map(|e| e.unwrap().path())
        // A module is a `.rs` file or a directory of them; insta's `snapshots/` is neither.
        .filter(|p| {
            p.extension().is_some_and(|e| e == "rs")
                || (p.is_dir() && !files_of(&p.file_name().unwrap().to_string_lossy()).is_empty())
        })
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .filter(|name| name != "lib")
        .collect();
    let mapped: BTreeSet<String> = ALLOWED.iter().map(|(m, _)| (*m).to_owned()).collect();
    assert_eq!(
        declared, mapped,
        "add the module to ALLOWED in tests/architecture.rs, with what it may use"
    );
}

#[test]
fn tiers_depend_only_downwards() {
    let mut broken = Vec::new();
    for (module, allowed) in ALLOWED {
        for file in files_of(module) {
            let text = fs::read_to_string(&file).unwrap();
            for used in crate_paths(&text) {
                if used != *module && !allowed.contains(&used.as_str()) {
                    broken.push(format!("{} uses crate::{used}", file.display()));
                }
            }
        }
    }
    assert!(broken.is_empty(), "forbidden dependencies:\n{}", broken.join("\n"));
}

#[test]
fn the_scanner_finds_crate_paths() {
    let found = crate_paths("use crate::syntax::Lexer;\nfn f() { crate::filters::decode(); }");
    assert_eq!(found, BTreeSet::from(["syntax".to_owned(), "filters".to_owned()]));
}
