//! Mutation fuzzing on stable Rust: every fixture, damaged at random, must be answered
//! with a report, never a panic and never a hang. A panic inside a check is caught as the `internal` finding, so that
//! finding fails the test too. Each case shrinks to a small set of edits when it fails.
//!
//! `PROPTEST_CASES=100000 cargo test --release --test mutation` runs it for hours.

use std::path::Path;
use std::time::{Duration, Instant};

use proof_pdf::checks::verify_with;
use proof_pdf::findings::{INTERNAL, Level};
use proof_pdf::pipeline::Limits;
use proptest::prelude::*;

/// Every built fixture, read once.
fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/pdfs");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "pdf"))
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(p).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

/// Tokens that steer a parser into its rarer paths when spliced in.
const TOKENS: &[&[u8]] = &[
    b"<<",
    b">>",
    b"[",
    b"]",
    b"(",
    b")",
    b"<",
    b">",
    b"/",
    b"%",
    b"\\",
    b" R ",
    b" 0 ",
    b"4294967296",
    b"-1",
    b"99999999999999999999",
    b"obj",
    b"endobj",
    b"stream\n",
    b"endstream",
    b"xref\n",
    b"trailer",
    b"startxref\n0\n",
    b"%%EOF",
    b"/Length 0",
    b"/Length 1 0 R",
    b"/Filter /FlateDecode",
    b"/Prev 0",
    b"/XRefStm 0",
    b"/Type /ObjStm",
    b"/N 99999",
    b"#",
    b"\r",
    b"\x00",
    b"[[[[[[[[[[[[[[[[",
];

#[derive(Debug, Clone)]
enum Edit {
    /// Replace the byte at this fraction of the file.
    Flip(f64, u8),
    /// Remove this many bytes there.
    Cut(f64, usize),
    /// Splice in a token there.
    Insert(f64, usize),
    /// Repeat this many bytes there.
    Repeat(f64, usize),
    /// Keep only this fraction of the file.
    Truncate(f64),
}

fn edit() -> impl Strategy<Value = Edit> {
    let at = 0.0..1.0f64;
    prop_oneof![
        (at.clone(), any::<u8>()).prop_map(|(a, b)| Edit::Flip(a, b)),
        (at.clone(), 1..64usize).prop_map(|(a, n)| Edit::Cut(a, n)),
        (at.clone(), 0..TOKENS.len()).prop_map(|(a, t)| Edit::Insert(a, t)),
        (at, 1..256usize).prop_map(|(a, n)| Edit::Repeat(a, n)),
        (0.0..1.0f64).prop_map(Edit::Truncate),
    ]
}

fn apply(bytes: &mut Vec<u8>, edit: &Edit) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a fraction of a length"
    )]
    let at = |f: f64, len: usize| ((len as f64) * f) as usize;
    let len = bytes.len();
    match *edit {
        Edit::Flip(f, b) => {
            if let Some(x) = bytes.get_mut(at(f, len)) {
                *x = b;
            }
        }
        Edit::Cut(f, n) => {
            let start = at(f, len);
            bytes.drain(start..(start + n).min(len));
        }
        Edit::Insert(f, t) => {
            let start = at(f, len);
            bytes.splice(start..start, TOKENS[t].iter().copied());
        }
        Edit::Repeat(f, n) => {
            let start = at(f, len);
            let piece = bytes[start..(start + n).min(len)].to_vec();
            bytes.splice(start..start, piece);
        }
        Edit::Truncate(f) => bytes.truncate(at(f, len)),
    }
}

const LEVELS: [Level; 6] = [Level::X4, Level::X4p, Level::X5g, Level::X5pg, Level::X5n, Level::Print];

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn a_damaged_file_gets_a_report(
        pick in any::<prop::sample::Index>(),
        level in 0..LEVELS.len(),
        edits in prop::collection::vec(edit(), 1..8),
    ) {
        thread_local!(static FIXTURES: Vec<(String, Vec<u8>)> = fixtures());
        let (name, mut bytes) = FIXTURES.with(|f| pick.get(f).clone());
        for e in &edits {
            apply(&mut bytes, e);
        }
        let start = Instant::now();
        // A million objects, not ten: the fixture that lists 8,388,608 takes seconds to open in a debug build, which is
        // what the gates run, and the limit's own refusal is worth fuzzing too.
        let limits = Limits { objects: 1_000_000, ..Limits::default() };
        let report = verify_with(&name, &bytes, LEVELS[level], &limits);
        prop_assert!(start.elapsed() < Duration::from_secs(10), "{name}: took {:?}", start.elapsed());
        let internal = report.findings.iter().find(|f| f.check.id == INTERNAL.id);
        prop_assert!(internal.is_none(), "{name}: {internal:?}");
    }
}
