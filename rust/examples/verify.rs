//! Verify one file and print its report as JSON: `cargo run --example verify -- x4 file.pdf`. Exits 0 when the file
//! passes, 1 when it fails, 2 when it is refused, so `bin/suites/pdfx.py` can rank outcomes; a panic exits 101. The real
//! command line arrives in phase 6 of the PDF/X plan.

// A command-line program prints, exits with a code, and gives up on an unreadable argument; the library's no-print and
// no-panic rules are for code that handles PDF input.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::expect_used,
    reason = "a command-line example"
)]

use proof_pdf::checks::verify;
use proof_pdf::findings::{Level, Verdict};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [level, path] = args.as_slice() else {
        eprintln!("usage: verify <x4|x4p|x5g|x5pg|x5n> <file.pdf>");
        std::process::exit(64);
    };
    let level = match level.as_str() {
        "x4" => Level::X4,
        "x4p" => Level::X4p,
        "x5g" => Level::X5g,
        "x5pg" => Level::X5pg,
        "x5n" => Level::X5n,
        other => {
            eprintln!("unknown level {other}");
            std::process::exit(64);
        }
    };
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(66);
    });
    let report = verify(path, &bytes, level);
    println!(
        "{}",
        serde_json::to_string(&report).expect("a report always serialises")
    );
    std::process::exit(match report.verdict {
        Verdict::Pass => 0,
        Verdict::Fail => 1,
        Verdict::Refused => 2,
    });
}
