//! The `proof-pdf` command line: `verify` in Rust, with the Python command's options and reports.
//!
//! Exit codes: 0 pass, 1 findings, 2 a file refused or unreadable, 3 a tool timed out, 64 a usage error; `--help`
//! and `--version` exit 0.
//! A panic outside any check exits 70 (`EX_SOFTWARE`); each check's own panics are findings.

mod args;
mod config;
mod output;
mod run;
mod tools;

use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;
use clap::error::ErrorKind;

use args::{Cli, Command, Verify};
use output::{Style, USAGE};
use run::{FileReport, Usage};

/// Files verified at once when `--jobs` is not given.
const DEFAULT_JOBS: u64 = 4;
/// A panic outside any check.
const INTERNAL: u8 = 70;

/// The binary's whole work: run `args` against the process's standard streams and return the exit code.
#[must_use]
pub fn run(args: impl IntoIterator<Item = OsString>) -> u8 {
    let colour = std::io::stdout().is_terminal();
    guarded(|| main(args, &mut std::io::stdout(), &mut std::io::stderr(), colour))
}

/// `f`'s exit code, or 70 if it panics.
fn guarded(f: impl FnOnce() -> i32) -> u8 {
    catch_unwind(AssertUnwindSafe(f)).map_or(INTERNAL, |code| u8::try_from(code).unwrap_or(INTERNAL))
}

/// Run the command line `args` (the program name first), writing to `out` and `err`; `colour` when `out` is a
/// terminal. Returns the exit code.
pub fn main(args: impl IntoIterator<Item = OsString>, out: &mut dyn Write, err: &mut dyn Write, colour: bool) -> i32 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            let _ = write!(out, "{e}");
            return output::PASS;
        }
        Err(e) => {
            let _ = write!(err, "{e}");
            return USAGE;
        }
    };
    let Command::Verify(mut options) = cli.command;
    let configured = if cli.no_config {
        Ok(options.output.show_profile.then(String::new))
    } else {
        configure(&mut options, cli.config.as_deref(), cli.profile.as_deref())
    };
    match configured.and_then(|shown| match shown {
        // The settings are checked as `verify` would check them before they are shown.
        Some(table) => run::plan(&options).map(|_| {
            let _ = write!(out, "{table}");
            output::PASS
        }),
        None => verify(&options, out, colour),
    }) {
        Ok(code) => code,
        Err(Usage(message)) => {
            let _ = writeln!(err, "error: {message}");
            USAGE
        }
    }
}

/// Fill `o` from the config file, if there is one; with `--show-profile`, the merged settings to print instead.
fn configure(o: &mut Verify, path: Option<&std::path::Path>, profile: Option<&str>) -> Result<Option<String>, Usage> {
    let here = std::env::current_dir().unwrap_or_default();
    let Some(path) = path.map(std::path::Path::to_owned).or_else(|| config::find(&here)) else {
        return match profile {
            Some(_) => Err(Usage(
                "--profile needs a config file and none was found; pass --config or add proof-pdf.toml".to_owned(),
            )),
            None => Ok(o.output.show_profile.then(String::new)),
        };
    };
    let table = config::load(&path, profile)?;
    let base = path.parent().unwrap_or(&here);
    config::apply(o, &table, base, &path.display().to_string())?;
    Ok(o.output
        .show_profile
        .then(|| toml::to_string(&table).unwrap_or_default()))
}

fn verify(o: &Verify, out: &mut dyn Write, colour: bool) -> Result<i32, Usage> {
    let plan = run::plan(o)?;
    let exemptions = run::exemptions(&o.exempt)?;
    let targets = run::targets(&o.files, &o.exclude)?;
    if targets.is_empty() {
        return Err(Usage("no PDFs to verify".to_owned()));
    }
    let jobs = usize::try_from(o.jobs.unwrap_or(DEFAULT_JOBS))
        .unwrap_or(usize::MAX)
        .min(targets.len());
    let next = AtomicUsize::new(0);
    let mut reports: Vec<(usize, FileReport)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..jobs)
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = targets.get(index) else { break mine };
                        mine.push((index, run::verify(path, &plan, &exemptions)));
                    }
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
    });
    reports.sort_by_key(|(index, _)| *index);
    let reports: Vec<FileReport> = reports.into_iter().map(|(_, r)| r).collect();
    let text = if o.output.json {
        output::json(&reports)
    } else if o.output.quiet {
        String::new()
    } else {
        output::text(&reports, o, &plan, &Style { colour })
    };
    // A closed pipe is the reader's business; the exit code still says what was found.
    let _ = out.write_all(text.as_bytes());
    Ok(output::exit_code(&reports))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pdfs");

    fn fixture(name: &str) -> String {
        format!("{FIXTURES}/{name}.pdf")
    }

    /// The exit code, standard output and standard error of `proof-pdf` with `args`.
    fn cli(args: &[&str]) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let words = std::iter::once("proof-pdf")
            .chain(args.iter().copied())
            .map(OsString::from);
        let code = main(words, &mut out, &mut err, false);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    #[test]
    fn help_version_and_usage_errors() {
        assert_eq!(cli(&["--version"]).0, 0);
        assert!(cli(&["verify", "--help"]).1.contains("--trim-mm"));
        let usage = |args: &[&str], says: &str| {
            let (code, _, err) = cli(args);
            assert_eq!(code, 64, "{args:?}");
            assert!(err.contains(says), "{args:?}: {err}");
        };
        let good = fixture("x4-good");
        usage(&["verify", "--bogus"], "unexpected argument");
        usage(&["verify", &good, "--warn-tac", "300"], "must be below --max-tac");
        usage(&["verify", &good, "--warn-dpi", "200"], "must be above --min-dpi");
        usage(&["verify", &good, "--trim-mm", "85"], "must look like 105x148");
        usage(&["verify", &good, "--trim-mm", "0x5"], "must be positive");
        usage(&["verify", &good, "--exempt", "nothing"], "GLOB=reason");
        usage(&["verify", &good, "--exempt", "[=x"], "is not a glob");
        usage(&["verify", FIXTURES, "--exclude", "["], "is not a glob");
        usage(&["verify", "--exclude", "*"], "no PDFs to verify");
    }

    #[test]
    fn text_reports_and_exit_codes() {
        let (code, out, _) = cli(&[
            "verify",
            &fixture("x4-good"),
            &fixture("x4-limit-nesting"),
            "--trim-mm",
            "105*148",
        ]);
        assert_eq!(code, 1);
        assert!(out.contains("x4-good.pdf"), "{out}");
        assert!(
            out.contains("x [x4.limits.graphics] (page 1) graphics states nested 29 deep"),
            "{out}"
        );
        assert!(
            out.ends_with(
                "FAIL -- 1 of 2 file(s) failed; checked: PDF/X as claimed, geometry, colour, missing glyphs, fonts\n"
            ),
            "{out}"
        );
        let (code, out, _) = cli(&["verify", &fixture("x4-good"), "--exempt", "x4-good.pdf=a proof copy"]);
        assert_eq!(code, 0);
        assert!(
            out.contains("EXEMPT\n      - a proof copy") && out.contains("(1 exempt)"),
            "{out}"
        );
        assert_eq!(
            cli(&["verify", &fixture("x4-no-header"), "--quiet"]),
            (2, String::new(), String::new())
        );
        let (code, out, _) = cli(&["verify", "no-such.pdf"]);
        assert_eq!(code, 2);
        // The OS's own words (they differ on Windows), without Rust's "(os error N)".
        assert!(
            out.contains("x cannot read the file: ") && !out.contains("os error"),
            "{out}"
        );
        let (code, out, _) = cli(&["verify", &fixture("corrupt")]);
        assert_eq!(code, 2);
        assert!(out.contains("x not evaluated, the file was refused: "), "{out}");
        assert!(out.contains(" checks\n"), "{out}");
        // A warning alone passes; repeated findings collapse into page ranges.
        let (code, out, _) = cli(&["verify", &fixture("x4-name-other-not-utf8"), "--no-colour"]);
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("! [x4.names.other-recommended]") && out.contains("(1 advisory warning(s))"),
            "{out}"
        );
        let (_, out, _) = cli(&["verify", &fixture("three-pages"), "--no-pdfx", "--trim-mm", "10x10"]);
        assert!(out.contains("(pages 1-3)"), "{out}");
    }

    #[test]
    fn every_switch_and_level_reaches_the_plan() {
        let args = [
            "proof-pdf",
            "verify",
            FIXTURES,
            "--exclude",
            "*",
            "--pdfa",
            "--flavour",
            "1b",
            "--tac",
            "--k-only-text",
            "--min-text-pt",
            "6",
            "--min-stroke-pt",
            "0.25",
            "--safe-mm",
            "3",
            "--min-delta-l",
            "40",
            "--allow-spot",
            "Gold",
            "--output-condition",
            "FOGRA51",
            "--require-bleed",
            "--no-marks",
            "--uniform-pages",
            "--pages-multiple-of",
            "4",
            "--binding",
            "right",
            "--binding-bleed-mm",
            "0",
            "--colour-mode",
            "rgb",
            "--proof-profile",
            "p.icc",
            "--pdfx",
            "x5n",
            "-j",
            "2",
        ];
        let words = args
            .iter()
            .map(OsString::from)
            .chain([OsString::from(fixture("x4-good"))]);
        let Command::Verify(o) = Cli::try_parse_from(words).unwrap().command;
        let plan = run::plan(&o).unwrap();
        assert_eq!(plan.pdfx, Some(crate::findings::Level::X5n));
        for id in [
            "k-only-text",
            "rgb-only",
            "page-size",
            "outside-bleed",
            "artwork-bleed",
            "contrast",
            "spot-ink",
        ] {
            assert!(plan.wanted.contains(id), "{id}");
        }
        let (_, out, _) = cli(&args[1..]
            .iter()
            .copied()
            .chain([fixture("x4-good").as_str()])
            .collect::<Vec<_>>());
        assert!(
            out.contains("PDF/A-1b, PDF/X-5n, peak ink, colour, K-only text"),
            "{out}"
        );
        for (level, name) in [
            ("x4", "PDF/X-4"),
            ("x4p", "PDF/X-4p"),
            ("x5g", "PDF/X-5g"),
            ("x5pg", "PDF/X-5pg"),
            ("x1a-2001", "PDF/X-1a:2001"),
            ("x3-2002", "PDF/X-3:2002"),
            ("x1a-2003", "PDF/X-1a:2003"),
            ("x3-2003", "PDF/X-3:2003"),
        ] {
            assert!(cli(&["verify", &fixture("x4-good"), "--pdfx", level]).1.contains(name));
        }
        let gray = cli(&[
            "verify",
            &fixture("x4-good"),
            "--colour-mode",
            "gray",
            "--no-fonts",
            "--no-missing-glyphs",
        ]);
        assert!(gray.1.contains("; checked: PDF/X as claimed, colour\n"), "{}", gray.1);
    }

    #[test]
    fn a_file_claiming_an_older_level_is_verified_at_it_unless_a_level_is_asked_for() {
        use std::fmt::Write;
        let path = std::env::temp_dir().join(format!("proof-pdf-cli-x3-{}.pdf", std::process::id()));
        // A one-page file whose Info claims PDF/X-3:2002, with its cross-reference table worked out here.
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>",
            "<< /GTS_PDFXVersion (PDF/X-3:2002) >>",
        ];
        let mut bytes = String::from("%PDF-1.3\n");
        let mut xref = String::from("xref\n0 5\n0000000000 65535 f\r\n");
        for (n, object) in objects.iter().enumerate() {
            let _ = write!(xref, "{:010} 00000 n\r\n", bytes.len());
            let _ = write!(bytes, "{} 0 obj\n{object}\nendobj\n", n + 1);
        }
        let start = bytes.len();
        let _ = write!(
            bytes,
            "{xref}trailer\n<< /Size 5 /Root 1 0 R /Info 4 0 R >>\nstartxref\n{start}\n%%EOF\n"
        );
        std::fs::write(&path, bytes).unwrap();
        let file = path.display().to_string();
        let (_, out, _) = cli(&["verify", &file, "--no-colour"]);
        assert!(out.contains("FAIL (as PDF/X-3:2002)"), "{out}");
        let (_, out, _) = cli(&["verify", &file, "--json"]);
        assert!(out.contains("\"level\": \"PDF/X-3:2002\""), "{out}");
        let (_, out, _) = cli(&["verify", &file, "--pdfx", "x4", "--json"]);
        assert!(out.contains("\"level\": \"PDF/X-4\""), "{out}");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn json_reports() {
        let (code, out, _) = cli(&[
            "verify",
            &fixture("x4-limit-nesting"),
            &fixture("x4-good"),
            "--json",
            "-j",
            "1",
        ]);
        assert_eq!(code, 1);
        let records: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(records[0]["ok"], false);
        assert_eq!(records[0]["findings"][0]["check"], "x4.limits.graphics");
        assert_eq!(records[1]["ok"], true);
        let (_, out, _) = cli(&["verify", "no-such.pdf", "--json"]);
        assert!(out.contains("\"unreadable\": \"cannot read the file"), "{out}");
    }

    #[test]
    fn config_files_and_profiles() {
        let dir = std::env::temp_dir().join(format!("proof-pdf-cli-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("proof-pdf.toml");
        std::fs::write(
            &file,
            "[verify]\ntrim-mm = '210x297'\n[profiles.big.verify]\ntrim-mm = '297x420'\n",
        )
        .unwrap();
        let config = file.to_str().unwrap();
        let (code, out, _) = cli(&["--config", config, "verify", &fixture("x4-good")]);
        assert_eq!(code, 1);
        assert!(out.contains("expected 210 x 297 mm"), "{out}");
        let (_, out, _) = cli(&["--config", config, "--profile", "big", "verify", &fixture("x4-good")]);
        assert!(out.contains("expected 297 x 420 mm"), "{out}");
        let (code, out, _) = cli(&["--config", config, "--profile", "big", "verify", "--show-profile"]);
        assert_eq!((code, out.as_str()), (0, "trim-mm = \"297x420\"\n"));
        assert_eq!(cli(&["verify", "--show-profile"]), (0, String::new(), String::new()));
        assert_eq!(
            cli(&["--no-config", "verify", "--show-profile"]),
            (0, String::new(), String::new())
        );
        assert_eq!(
            cli(&["--no-config", "verify", "--show-profile", "--trim-mm", "x"]).0,
            64
        );
        let (code, _, _) = cli(&["--no-config", "--profile", "big", "verify", &fixture("x4-good")]);
        assert_eq!(code, 0);
        let (code, _, err) = cli(&["--profile", "big", "verify", &fixture("x4-good")]);
        assert_eq!(code, 64);
        assert!(err.contains("--profile needs a config file"), "{err}");
        let (code, _, err) = cli(&["--config", config, "--profile", "none", "verify", &fixture("x4-good")]);
        assert_eq!(code, 64);
        assert!(err.contains("has no profile \"none\""), "{err}");
        std::fs::write(&file, "[verify]\nbleed = 3\n").unwrap();
        assert!(
            cli(&["--config", config, "verify", &fixture("x4-good")])
                .2
                .contains("has no option")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn what_the_reading_tiers_find_is_reported_once() {
        let checks = |args: &[&str]| -> Vec<String> {
            let (_, out, _) = cli(args);
            let records: serde_json::Value = serde_json::from_str(&out).unwrap();
            let findings = records[0]["findings"].as_array().unwrap();
            findings
                .iter()
                .map(|f| f["check"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(
            checks(&["verify", &fixture("x4-bad-xref"), "--no-pdfx", "--json"]),
            ["file.xref"]
        );
        assert_eq!(
            checks(&["verify", &fixture("x4-bad-xref"), "--json"]),
            ["file.xref", "x4.pdf-reference.conforms"]
        );
    }

    #[test]
    fn a_check_left_unjudged_is_named_with_why() {
        let (code, out, _) = cli(&["verify", &fixture("x5g-good"), "--pdfx", "x5g", "--file-alone"]);
        assert_eq!(code, 1);
        let why = "x not evaluated, the file is verified alone, without its targets: x5.g-target-conformance-level, ";
        assert!(out.contains(why), "{out}");
    }

    #[test]
    fn a_pdfx_4p_file_is_verified_with_its_external_profile_unless_alone() {
        let (code, _, _) = cli(&["verify", &fixture("x4p-good"), "--pdfx", "x4p"]);
        assert_eq!(code, 0);
        let (code, out, _) = cli(&["verify", &fixture("x4p-good"), "--pdfx", "x4p", "--file-alone"]);
        assert_eq!(code, 1);
        assert!(
            out.contains("not evaluated, the file is verified alone, without its external profile"),
            "{out}"
        );
    }

    #[test]
    fn a_target_of_another_version_passes_only_when_accepted() {
        let file = fixture("x5g-target-other-version");
        assert_eq!(cli(&["verify", &file, "--pdfx", "x5g"]).0, 1);
        assert_eq!(
            cli(&["verify", &file, "--pdfx", "x5g", "--target-versions", "accept"]).0,
            0
        );
        assert_eq!(
            cli(&["verify", &file, "--pdfx", "x5g", "--target-versions", "reject"]).0,
            1
        );
    }

    #[test]
    fn the_python_spellings_still_work() {
        let words = [
            "proof-pdf",
            "verify",
            "x.pdf",
            "--no-colour",
            "--colour",
            "--no-pdfa",
            "--fonts",
            "--no-fonts",
        ];
        let Command::Verify(o) = Cli::try_parse_from(words).unwrap().command;
        assert_eq!(
            (o.inks.no_colour, o.layers.pdfa, o.lettering.no_fonts),
            (false, false, true)
        );
        let words = [
            "proof-pdf",
            "verify",
            "x.pdf",
            "--pdfa",
            "--no-pdfa",
            "--missing-glyphs",
        ];
        let Command::Verify(o) = Cli::try_parse_from(words).unwrap().command;
        assert_eq!((o.layers.pdfa, o.lettering.no_missing_glyphs), (false, false));
    }

    #[test]
    fn a_panic_exits_70() {
        assert_eq!(guarded(|| panic!("boom")), INTERNAL);
        assert_eq!(guarded(|| 300), INTERNAL);
        assert_eq!(guarded(|| 1), 1);
    }

    #[test]
    fn colour_on_a_terminal() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let argv = ["proof-pdf", "verify", &fixture("x4-good")].map(OsString::from);
        assert_eq!(main(argv, &mut out, &mut err, true), 0);
        assert!(String::from_utf8_lossy(&out).contains("\u{1b}[32mOK\u{1b}[0m"));
    }
}
