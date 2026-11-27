//! Per-project and per-printer defaults for `verify`, from `proof-pdf.toml` or a `[tool.proof-pdf]` table in
//! `pyproject.toml`, found by walking up from the working directory (or named with `--config`):
//!
//! ```toml
//! schema = 1
//!
//! [verify]
//! trim-mm = "105x148"
//! output-condition = "FOGRA51"
//! proof-profile = "profiles/PSOcoated_v3.icc"   # relative to this file
//!
//! [verify.exempt]
//! "manual.pdf" = "internal documentation, never printed"
//!
//! [profiles.deluxe]
//! extends = "bookish"                            # another profile, merged first
//! verify = { bleed-mm = 5 }
//! ```
//!
//! A key is a `verify` option, spelt with dashes or underscores; the Python command's spellings still work
//! (`pdfx = false`, `colour = false`, `fonts = false`, `missing-glyphs = false`, `verapdf-bin`, `gs-bin`). An option
//! given on the command line wins. Unknown keys, tables and a higher `schema` are errors, so a typo cannot turn a check
//! off. Tables of the commands still in Python (`inspect`, `preview`, `texlog`, `extract`, `crop`) are left to them.

use std::path::{Path, PathBuf};

use clap::ValueEnum;
use toml::{Table, Value};

use super::args::Verify;
use super::run::{Usage, io_message};

const NAME: &str = "proof-pdf.toml";
const SCHEMA: i64 = 1;
const PYTHON_COMMANDS: [&str; 5] = ["inspect", "preview", "texlog", "extract", "crop"];

/// The nearest `proof-pdf.toml`, or `pyproject.toml` with a `[tool.proof-pdf]` table, from `start` upwards.
pub(crate) fn find(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|dir| {
        let own = dir.join(NAME);
        let pyproject = dir.join("pyproject.toml");
        if own.is_file() {
            Some(own)
        } else {
            read(&pyproject).ok().and_then(|t| tool_table(&t)).map(|_| pyproject)
        }
    })
}

fn read(path: &Path) -> Result<Table, Usage> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Usage(format!("cannot read {}: {}", path.display(), io_message(&e))))?;
    text.parse::<Table>()
        .map_err(|e| Usage(format!("cannot read {}: {e}", path.display())))
}

fn tool_table(pyproject: &Table) -> Option<Table> {
    pyproject.get("tool")?.get("proof-pdf")?.as_table().cloned()
}

/// The `[verify]` table the file at `path` describes, with `profile`'s chain merged over it, key by key.
pub(crate) fn load(path: &Path, profile: Option<&str>) -> Result<Table, Usage> {
    let file = read(path)?;
    let mut table = if path.file_name().is_some_and(|n| n == "pyproject.toml") {
        tool_table(&file).ok_or_else(|| Usage(format!("{} has no [tool.proof-pdf] table", path.display())))?
    } else {
        file
    };
    let at = path.display();
    match table.remove("schema") {
        None | Some(Value::Integer(SCHEMA)) => {}
        Some(other) => {
            return Err(Usage(format!(
                "{at}: schema {other} is not supported; this proof-pdf reads schema {SCHEMA}"
            )));
        }
    }
    let profiles = table.remove("profiles").unwrap_or_else(|| Value::Table(Table::new()));
    let mut merged = command(&table, "verify", &format!("{at}: [verify]"))?;
    for (key, _) in &table {
        if key != "verify" && !PYTHON_COMMANDS.contains(&key.as_str()) {
            return Err(Usage(format!(
                "{at}: [{key}] is not a command or setting this proof-pdf knows"
            )));
        }
    }
    let Some(name) = profile else { return Ok(merged) };
    let profiles = profiles.as_table().cloned().unwrap_or_default();
    let mut chain: Vec<&str> = Vec::new();
    let mut next = Some(name);
    while let Some(name) = next {
        if chain.contains(&name) {
            return Err(Usage(format!(
                "{at}: profile {name:?} extends itself, through {}",
                chain.join(", ")
            )));
        }
        let Some(own) = profiles.get(name).and_then(Value::as_table) else {
            let known: Vec<&str> = profiles.keys().map(String::as_str).collect();
            let known = if known.is_empty() {
                "it defines none".to_owned()
            } else {
                format!("profiles: {}", known.join(", "))
            };
            return Err(Usage(format!("{at} has no profile {name:?} ({known})")));
        };
        chain.push(name);
        next = own.get("extends").and_then(Value::as_str);
    }
    for name in chain.iter().rev() {
        let own = profiles
            .get(*name)
            .and_then(Value::as_table)
            .cloned()
            .unwrap_or_default();
        merged.extend(command(&own, "verify", &format!("{at}: [profiles.{name}.verify]"))?);
        let unknown = own
            .keys()
            .find(|k| *k != "verify" && *k != "extends" && !PYTHON_COMMANDS.contains(&k.as_str()));
        if let Some(key) = unknown {
            return Err(Usage(format!("{at}: [profiles.{name}] has no setting {key:?}")));
        }
    }
    Ok(merged)
}

fn command(table: &Table, name: &str, at: &str) -> Result<Table, Usage> {
    match table.get(name) {
        None => Ok(Table::new()),
        Some(Value::Table(own)) => Ok(own.clone()),
        Some(_) => Err(Usage(format!("{at} is not a table"))),
    }
}

/// Fill the options the command line left unset from `table`, whose relative paths are relative to `base`.
pub(crate) fn apply(o: &mut Verify, table: &Table, base: &Path, at: &str) -> Result<(), Usage> {
    let (cli_level, cli_pdfa, cli_colour, cli_glyphs, cli_fonts) = given(o);
    for (key, value) in table {
        let key = key.replace('_', "-");
        let bad = |what: &str| Usage(format!("{at}: {key} must be {what}, got {value}"));
        let flag = || value.as_bool().ok_or_else(|| bad("true or false"));
        let number = || {
            value
                .as_float()
                .or_else(|| value.as_integer().map(float))
                .ok_or_else(|| bad("a number"))
        };
        let text = || value.as_str().map(str::to_owned).ok_or_else(|| bad("a string"));
        let texts = || match value {
            Value::String(s) => Ok(vec![s.clone()]),
            Value::Array(items) => items
                .iter()
                .map(|i| i.as_str().map(str::to_owned).ok_or_else(|| bad("strings")))
                .collect(),
            _ => Err(bad("a string or a list of strings")),
        };
        match key.as_str() {
            "exclude" => fill_list(&mut o.exclude, texts()?),
            "exempt" => fill_list(
                &mut o.exempt,
                exemptions(value).ok_or_else(|| bad("a table of glob = reason"))?,
            ),
            "pdfx" => {
                if let Value::Boolean(on) = value {
                    switch(&mut o.layers.no_pdfx, !on, cli_level);
                } else {
                    let level = enumerated(
                        &text()?,
                        bad("x4, x4p, x5g, x5pg, x5n, x1a-2001, x3-2002, x1a-2003 or x3-2003"),
                    )?;
                    if !cli_level {
                        o.layers.pdfx = Some(level);
                    }
                }
            }
            "no-pdfx" => switch(&mut o.layers.no_pdfx, flag()?, cli_level),
            "file-alone" => o.file_alone |= flag()?,
            "target-versions" => fill(&mut o.target_versions, enumerated(&text()?, bad("accept or reject"))?),
            "pdfa" => switch(&mut o.layers.pdfa, flag()?, cli_pdfa),
            "flavour" => fill(&mut o.layers.flavour, text()?),
            "tac" => o.layers.tac |= flag()?,
            "max-tac" => fill(&mut o.layers.max_tac, number()?),
            "warn-tac" => fill(&mut o.layers.warn_tac, number()?),
            "colour" => switch(&mut o.inks.no_colour, !flag()?, cli_colour),
            "no-colour" => switch(&mut o.inks.no_colour, flag()?, cli_colour),
            "colour-mode" => fill(&mut o.inks.colour_mode, enumerated(&text()?, bad("cmyk, rgb or gray"))?),
            "strict-cmyk" => o.inks.strict_cmyk |= flag()?,
            "allow-k-only" => o.inks.allow_k_only |= flag()?,
            "small-text-pt" => fill(&mut o.inks.small_text_pt, number()?),
            "min-dpi" => fill(&mut o.inks.min_dpi, number()?),
            "warn-dpi" => fill(&mut o.inks.warn_dpi, number()?),
            "max-dpi" => fill(&mut o.inks.max_dpi, number()?),
            "max-stretch-pct" => fill(&mut o.inks.max_stretch_pct, number()?),
            "allow-spot" => fill(&mut o.inks.allow_spot, texts()?),
            "output-condition" => fill(&mut o.inks.output_condition, text()?),
            "min-delta-l" => fill(&mut o.inks.min_delta_l, number()?),
            "proof-profile" => fill(&mut o.inks.proof_profile, base.join(text()?)),
            "k-only-text" => o.lettering.k_only_text |= flag()?,
            "missing-glyphs" => switch(&mut o.lettering.no_missing_glyphs, !flag()?, cli_glyphs),
            "no-missing-glyphs" => switch(&mut o.lettering.no_missing_glyphs, flag()?, cli_glyphs),
            "fonts" => switch(&mut o.lettering.no_fonts, !flag()?, cli_fonts),
            "no-fonts" => switch(&mut o.lettering.no_fonts, flag()?, cli_fonts),
            "min-text-pt" => fill(&mut o.lettering.min_text_pt, number()?),
            "min-stroke-pt" => fill(&mut o.lettering.min_stroke_pt, number()?),
            "safe-mm" => fill(&mut o.lettering.safe_mm, number()?),
            "trim-mm" => fill(&mut o.sizes.trim_mm, text()?),
            "bleed-mm" => fill(&mut o.sizes.bleed_mm, number()?),
            "binding-bleed-mm" => fill(&mut o.sizes.binding_bleed_mm, number()?),
            "binding" => fill(&mut o.sizes.binding, enumerated(&text()?, bad("left or right"))?),
            "tolerance-mm" => fill(&mut o.sizes.tolerance_mm, number()?),
            "require-bleed" => o.sizes.require_bleed |= flag()?,
            "no-marks" => o.sizes.no_marks |= flag()?,
            "uniform-pages" => o.sizes.uniform_pages |= flag()?,
            "pages-multiple-of" => fill(
                &mut o.sizes.pages_multiple_of,
                count(value).ok_or_else(|| bad("a whole number from 1"))?,
            ),
            "jobs" | "j" => fill(
                &mut o.jobs,
                count(value)
                    .and_then(|n| u64::try_from(n).ok())
                    .ok_or_else(|| bad("a whole number from 1"))?,
            ),
            "verapdf" | "verapdf-bin" => fill(&mut o.verapdf, text()?),
            "gs" | "gs-bin" => fill(&mut o.gs, text()?),
            "timeout" => fill(&mut o.timeout, number()?),
            _ => return Err(Usage(format!("{at}: [verify] has no option {key:?}"))),
        }
    }
    // Checked once the command line's switches are merged in, so `--tac` on it serves a file's `max-tac`.
    match o.idle(|option| table.keys().any(|k| k.replace('_', "-") == option)) {
        Some((option, switch, layer)) => Err(Usage(format!(
            "{at}: {option} needs {switch} = true: {layer} is opt-in since 0.4.0"
        ))),
        None => Ok(()),
    }
}

/// Which paired switches the command line gave either way (`--pdfa` or `--no-pdfa`, ...): those are not the file's
/// to change. In order: the PDF/X level, PDF/A, colour, missing glyphs, fonts.
fn given(o: &Verify) -> (bool, bool, bool, bool, bool) {
    (
        o.layers.pdfx.is_some() || o.layers.no_pdfx,
        o.layers.pdfa || o.legacy.no_pdfa,
        o.inks.no_colour || o.legacy.colour,
        o.lettering.no_missing_glyphs || o.legacy_text.missing_glyphs,
        o.lettering.no_fonts || o.legacy.fonts,
    )
}

/// Turn `slot` on from the file unless the command line gave the switch either way.
fn switch(slot: &mut bool, on: bool, given: bool) {
    if !given {
        *slot |= on;
    }
}

/// Set `slot` from the file unless the command line set it.
fn fill<T>(slot: &mut Option<T>, value: T) {
    slot.get_or_insert(value);
}

fn fill_list(slot: &mut Vec<String>, values: Vec<String>) {
    if slot.is_empty() {
        *slot = values;
    }
}

/// A whole number from 1.
fn count(value: &Value) -> Option<i64> {
    value.as_integer().filter(|n| *n >= 1)
}

/// `[verify.exempt]` as `glob = "reason"`, or a list of `GLOB=reason`.
fn exemptions(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Table(t) => t
            .iter()
            .map(|(glob, reason)| Some(format!("{glob}={}", reason.as_str()?)))
            .collect(),
        Value::Array(items) => items.iter().map(|i| i.as_str().map(str::to_owned)).collect(),
        _ => None,
    }
}

/// One of a `ValueEnum`'s names.
fn enumerated<T: ValueEnum>(text: &str, bad: Usage) -> Result<T, Usage> {
    T::from_str(text, true).map_err(|_| bad)
}

/// A whole number written for a number option.
#[expect(clippy::cast_precision_loss, reason = "option values are far below 2^53")]
fn float(n: i64) -> f64 {
    n as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Binding, Cli, ColourMode, Command, Pdfx, TargetVersions};
    use clap::Parser;

    /// A fresh directory for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proof-pdf-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn options(args: &[&str]) -> Verify {
        let words = ["proof-pdf", "verify"].iter().chain(args).copied();
        let Command::Verify(o) = Cli::try_parse_from(words).unwrap().command;
        *o
    }

    fn error(result: Result<impl std::fmt::Debug, Usage>) -> String {
        result.unwrap_err().0
    }

    #[test]
    fn files_are_found_walking_up() {
        let dir = scratch("find");
        let deep = dir.join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(find(&deep), None);
        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = 'x'\n").unwrap();
        assert_eq!(find(&deep), None);
        std::fs::write(dir.join("pyproject.toml"), "[tool.proof-pdf.verify]\nflavour = '1b'\n").unwrap();
        assert_eq!(find(&deep), Some(dir.join("pyproject.toml")));
        assert_eq!(
            load(&dir.join("pyproject.toml"), None).unwrap()["flavour"].as_str(),
            Some("1b")
        );
        std::fs::write(dir.join("a/proof-pdf.toml"), "").unwrap();
        assert_eq!(find(&deep), Some(dir.join("a/proof-pdf.toml")));
        std::fs::write(dir.join("pyproject.toml"), "[project]\n").unwrap();
        assert!(error(load(&dir.join("pyproject.toml"), None)).contains("has no [tool.proof-pdf] table"));
        assert!(error(load(&dir.join("missing.toml"), None)).contains("cannot read"));
        std::fs::write(dir.join("bad.toml"), "verify = [").unwrap();
        assert!(error(load(&dir.join("bad.toml"), None)).contains("cannot read"));
    }

    #[test]
    fn schemas_tables_and_profiles() {
        let dir = scratch("load");
        let file = dir.join("proof-pdf.toml");
        let load_text = |text: &str, profile: Option<&str>| {
            std::fs::write(&file, text).unwrap();
            load(&file, profile)
        };
        assert!(load_text("schema = 1\n[preview]\ndpi = 1\n", None).unwrap().is_empty());
        assert!(
            error(load_text("schema = 2\n", None)).contains("schema 2 is not supported; this proof-pdf reads schema 1")
        );
        assert!(error(load_text("[nonsense]\n", None)).contains("[nonsense] is not a command"));
        assert!(error(load_text("verify = 1\n", None)).contains("[verify] is not a table"));
        let profiles = "[verify]\nbleed-mm = 3\nflavour = '2b'\n\
            [profiles.base]\nverify = { bleed-mm = 4, tac = true }\n\
            [profiles.deluxe]\nextends = 'base'\nverify = { bleed-mm = 5 }\n\
            [profiles.loop]\nextends = 'loop'\n\
            [profiles.typo]\nverfy = {}\n\
            [profiles.flat]\nverify = 1\n";
        let deluxe = load_text(profiles, Some("deluxe")).unwrap();
        assert_eq!(deluxe["bleed-mm"].as_integer(), Some(5));
        assert_eq!(deluxe["tac"].as_bool(), Some(true));
        assert_eq!(deluxe["flavour"].as_str(), Some("2b"));
        assert!(error(load_text(profiles, Some("loop"))).contains("extends itself"));
        assert!(error(load_text(profiles, Some("nope"))).contains("profiles: base, deluxe, flat, loop, typo"));
        assert!(error(load_text(profiles, Some("typo"))).contains("has no setting \"verfy\""));
        assert!(error(load_text(profiles, Some("flat"))).contains("is not a table"));
        assert!(error(load_text("[profiles]\n", Some("x"))).contains("has no profile \"x\" (it defines none)"));
    }

    #[test]
    fn every_key_and_the_command_line_wins() {
        let table: Table = r#"
            exclude = "*.tmp.pdf"
            exempt = { "manual.pdf" = "never printed" }
            pdfx = "x4p"
            pdfa = true
            flavour = "1b"
            tac = true
            max-tac = 320
            warn_tac = 300.5
            colour = false
            colour-mode = "gray"
            strict-cmyk = true
            allow-k-only = true
            small-text-pt = 10
            min-dpi = 250
            warn-dpi = 280
            max-dpi = 900
            max-stretch-pct = 2
            allow-spot = ["Gold", "Dieline"]
            output-condition = "FOGRA51"
            min-delta-l = 40
            proof-profile = "p.icc"
            k-only-text = true
            missing-glyphs = false
            fonts = false
            min-text-pt = 6
            min-stroke-pt = 0.25
            safe-mm = 3
            trim-mm = "105x148"
            bleed-mm = 3
            binding-bleed-mm = 0
            binding = "right"
            tolerance-mm = 0.5
            require-bleed = true
            no-marks = true
            uniform-pages = true
            pages-multiple-of = 4
            jobs = 2
            verapdf-bin = "/opt/verapdf"
            gs = "gswin64c"
            timeout = 60
            no-pdfx = false
            no-colour = false
            no-missing-glyphs = false
            no-fonts = false
        "#
        .parse()
        .unwrap();
        let mut o = options(&["--flavour", "2u", "--exclude", "keep"]);
        apply(&mut o, &table, Path::new("/cfg"), "t").unwrap();
        assert_eq!(o.layers.flavour.as_deref(), Some("2u"));
        assert_eq!(o.exclude, ["keep"]);
        assert_eq!(o.exempt, ["manual.pdf=never printed"]);
        assert_eq!(
            (o.layers.pdfx, o.layers.pdfa, o.layers.tac),
            (Some(Pdfx::X4p), true, true)
        );
        assert_eq!((o.inks.no_colour, o.inks.colour_mode), (true, Some(ColourMode::Gray)));
        assert_eq!(o.inks.proof_profile, Some(PathBuf::from("/cfg/p.icc")));
        assert_eq!((o.lettering.no_missing_glyphs, o.lettering.no_fonts), (true, true));
        assert_eq!(
            (o.sizes.binding, o.sizes.pages_multiple_of, o.jobs),
            (Some(Binding::Right), Some(4), Some(2))
        );
        assert_eq!(
            (o.verapdf.as_deref(), o.gs.as_deref()),
            (Some("/opt/verapdf"), Some("gswin64c"))
        );
        let off: Table = "pdfx = false\nexempt = ['a=b']\nfile-alone = true\ntarget-versions = 'accept'\n"
            .parse()
            .unwrap();
        let mut o = options(&[]);
        apply(&mut o, &off, Path::new("/"), "t").unwrap();
        assert!(o.layers.no_pdfx && o.file_alone);
        assert_eq!(o.target_versions, Some(TargetVersions::Accept));
        assert_eq!(o.exempt, ["a=b"]);
    }

    #[test]
    fn a_switch_on_the_command_line_wins_either_way() {
        // A switch the command line gave either way wins over the file, in both directions.
        let on: Table = "pdfx = 'x5n'\npdfa = true\ncolour = true\nmissing-glyphs = true\nfonts = true\n"
            .parse()
            .unwrap();
        let args = [
            "--no-pdfx",
            "--no-pdfa",
            "--no-colour",
            "--no-missing-glyphs",
            "--no-fonts",
        ];
        let mut o = options(&args);
        apply(&mut o, &on, Path::new("/"), "t").unwrap();
        assert_eq!((o.layers.no_pdfx, o.layers.pdfx, o.layers.pdfa), (true, None, false));
        assert!(o.inks.no_colour && o.lettering.no_missing_glyphs && o.lettering.no_fonts);
        let off: Table = "pdfx = false\npdfa = false\ncolour = false\nmissing-glyphs = false\nfonts = false\n\
            no-pdfx = true\nno-colour = true\nno-missing-glyphs = true\nno-fonts = true\n"
            .parse()
            .unwrap();
        let args = ["--pdfx", "x4", "--pdfa", "--colour", "--missing-glyphs", "--fonts"];
        let mut o = options(&args);
        apply(&mut o, &off, Path::new("/"), "t").unwrap();
        assert_eq!(
            (o.layers.no_pdfx, o.layers.pdfx, o.layers.pdfa),
            (false, Some(Pdfx::X4), true)
        );
        assert!(!o.inks.no_colour && !o.lettering.no_missing_glyphs && !o.lettering.no_fonts);
        let mut o = options(&["--pdfx", "x4"]);
        apply(&mut o, &"pdfx = 'x5n'".parse().unwrap(), Path::new("/"), "t").unwrap();
        assert_eq!(o.layers.pdfx, Some(Pdfx::X4));
        let mut o = options(&["--no-pdfx"]);
        assert!(error(apply(&mut o, &"pdfx = 'x9'".parse().unwrap(), Path::new("/"), "t")).contains("x4, x4p"));
    }

    #[test]
    fn wrong_values_name_the_key() {
        let wrong = |text: &str| {
            let table: Table = text.parse().unwrap();
            error(apply(&mut options(&[]), &table, Path::new("/"), "f.toml"))
        };
        assert!(wrong("pdfa = 1").contains("f.toml: pdfa must be true or false, got 1"));
        assert!(wrong("max-tac = 'x'").contains("must be a number"));
        assert!(wrong("flavour = 1").contains("must be a string"));
        assert!(wrong("allow-spot = [1]").contains("must be strings"));
        assert!(wrong("allow-spot = 1").contains("a string or a list of strings"));
        assert!(wrong("exempt = 1").contains("a table of glob = reason"));
        assert!(wrong("exempt = { a = 1 }").contains("a table of glob = reason"));
        assert!(wrong("pdfx = 'x9'").contains("x4, x4p, x5g, x5pg, x5n, x1a-2001, x3-2002, x1a-2003 or x3-2003"));
        assert!(wrong("target-versions = 'maybe'").contains("accept or reject"));
        assert!(wrong("jobs = 0").contains("a whole number from 1"));
        assert!(wrong("pages-multiple-of = 'two'").contains("a whole number from 1"));
        assert!(wrong("bleed = 3").contains("[verify] has no option \"bleed\""));
        // Every option refuses a value of the wrong kind.
        let keys = [
            "exclude",
            "pdfx",
            "no-pdfx",
            "file-alone",
            "target-versions",
            "pdfa",
            "flavour",
            "tac",
            "max-tac",
            "warn-tac",
            "colour",
            "no-colour",
            "colour-mode",
            "strict-cmyk",
            "allow-k-only",
            "small-text-pt",
            "min-dpi",
            "warn-dpi",
            "max-dpi",
            "max-stretch-pct",
            "allow-spot",
            "output-condition",
            "min-delta-l",
            "proof-profile",
            "k-only-text",
            "missing-glyphs",
            "no-missing-glyphs",
            "fonts",
            "no-fonts",
            "min-text-pt",
            "min-stroke-pt",
            "safe-mm",
            "trim-mm",
            "bleed-mm",
            "binding-bleed-mm",
            "binding",
            "tolerance-mm",
            "require-bleed",
            "no-marks",
            "uniform-pages",
            "pages-multiple-of",
            "jobs",
            "verapdf",
            "gs",
            "timeout",
        ];
        for key in keys {
            assert!(
                wrong(&format!("{key} = {{}}")).contains(&format!("{key} must be")),
                "{key}"
            );
        }
        assert!(wrong("colour-mode = 'cmy'").contains("cmyk, rgb or gray"));
        // An opt-in layer's option without its switch, under either spelling.
        let ink = "needs tac = true: total ink is opt-in since 0.4.0";
        assert_eq!(wrong("max-tac = 280"), format!("f.toml: max-tac {ink}"));
        assert_eq!(wrong("warn_tac = 250"), format!("f.toml: warn-tac {ink}"));
        assert_eq!(
            wrong("flavour = '2u'"),
            "f.toml: flavour needs pdfa = true: PDF/A is opt-in since 0.4.0"
        );
        // A tool path alone is fine: a shared config names it once for runs with and without the layer.
        let fine = |args: &[&str], text: &str| {
            let table: Table = text.parse().unwrap();
            apply(&mut options(args), &table, Path::new("/"), "f.toml")
        };
        assert!(fine(&[], "gs = 'gs'\nverapdf = 'verapdf'\ntimeout = 60").is_ok());
        assert!(fine(&["--tac"], "max-tac = 280").is_ok());
        assert!(fine(&[], "pdfa = true\nflavour = '2u'\ntac = true\nwarn-tac = 250").is_ok());
        assert!(wrong("binding = 'top'").contains("left or right"));
    }
}
