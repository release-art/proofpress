//! What `verify` does with its options: which checks they turn on, which files they name, and the report on each.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use globset::{Glob, GlobMatcher};

use super::args::{Binding, ColourMode, Pdfx, TargetVersions, Verify};
use super::tools;
use crate::checks::{self, Files, Geometry, Params, Settings};
use crate::findings::{Check, CheckResult, Finding, Level, PDFA, Report, TAC, Verdict};

/// A usage error: the options do not make sense together.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Usage(pub(crate) String);

/// One file's outcome.
#[derive(Debug)]
pub(crate) struct FileReport {
    pub(crate) path: PathBuf,
    /// Why the file was declared exempt and not checked.
    pub(crate) exempt: Option<String>,
    /// The checks run on it, merged across levels; `None` when exempt or unreadable.
    pub(crate) report: Option<Report>,
    /// Why the file could not be read at all, so nothing was checked.
    pub(crate) unreadable: Option<String>,
    /// Whether an external tool ran out of time on it.
    pub(crate) timed_out: bool,
}

impl FileReport {
    /// Whether nothing failed: no error finding, no check left unjudged, the file read.
    pub(crate) fn ok(&self) -> bool {
        self.unreadable.is_none() && self.report.as_ref().is_none_or(|r| r.verdict == Verdict::Pass)
    }

    pub(crate) fn findings(&self) -> &[Finding] {
        self.report.as_ref().map_or(&[], |r| r.findings.as_slice())
    }
}

/// The print checks' parameters and which of them the options turn on, and the external tools to run.
pub(crate) struct Plan {
    pub(crate) pdfx: Option<Level>,
    /// Whether a file claiming PDF/X-1a or PDF/X-3 is verified at the level it claims: no level was asked for.
    pub(crate) detect: bool,
    pub(crate) settings: Settings,
    pub(crate) wanted: BTreeSet<&'static str>,
    /// Every print check an option switches, on or off.
    pub(crate) switchable: BTreeSet<&'static str>,
    pub(crate) tools: Vec<Tool>,
    /// Whether each file is verified without the files it refers to.
    pub(crate) file_alone: bool,
}

/// An external tool `verify` runs on each file.
pub(crate) enum Tool {
    Verapdf {
        program: String,
        flavour: String,
        timeout: Duration,
    },
    Ghostscript {
        program: String,
        max: f64,
        warn: Option<f64>,
        timeout: Duration,
    },
}

impl Tool {
    fn run(&self, path: &Path) -> (&'static Check, tools::Verdict) {
        match self {
            Self::Verapdf {
                program,
                flavour,
                timeout,
            } => (PDFA, tools::verapdf(program, flavour, path, *timeout)),
            Self::Ghostscript {
                program,
                max,
                warn,
                timeout,
            } => (TAC, tools::peak_ink(program, path, *max, *warn, *timeout)),
        }
    }
}

/// The plan `verify`'s options describe.
pub(crate) fn plan(o: &Verify) -> Result<Plan, Usage> {
    if let Some((option, switch, layer)) = o.idle(|_| true) {
        return Err(Usage(format!(
            "--{option} needs --{switch}: {layer} is opt-in since 0.4.0"
        )));
    }
    let colour_mode = o.inks.colour_mode.unwrap_or_default();
    let max_tac = o.layers.max_tac.unwrap_or(300.0);
    if let Some(warn) = o.layers.warn_tac.filter(|w| *w >= max_tac) {
        return Err(Usage(format!(
            "--warn-tac ({warn}) must be below --max-tac ({max_tac})"
        )));
    }
    let min_dpi = o.inks.min_dpi.unwrap_or(300.0);
    if let Some(warn) = o.inks.warn_dpi.filter(|w| *w <= min_dpi) {
        return Err(Usage(format!(
            "--warn-dpi ({warn}) must be above --min-dpi ({min_dpi})"
        )));
    }
    let geometry = o.sizes.trim_mm.as_deref().map(|trim| geometry(o, trim)).transpose()?;
    let params = Params {
        accept_target_versions: o.target_versions.map(|v| v == TargetVersions::Accept),
        small_text_pt: o.inks.small_text_pt.unwrap_or(12.0),
        uniform_pages: o.sizes.uniform_pages,
        pages_multiple_of: o.sizes.pages_multiple_of,
        output_condition: o.inks.output_condition.clone(),
        allow_spots: o.inks.allow_spot.clone(),
        strict_cmyk: o.inks.strict_cmyk,
        colour_mode: match colour_mode {
            ColourMode::Cmyk => checks::ColourMode::Cmyk,
            ColourMode::Rgb => checks::ColourMode::Rgb,
            ColourMode::Gray => checks::ColourMode::Gray,
        },
        allow_k_only: o.inks.allow_k_only,
        min_dpi,
        warn_dpi: o.inks.warn_dpi,
        max_dpi: o.inks.max_dpi,
        max_stretch_pct: o.inks.max_stretch_pct,
        min_text_pt: o.lettering.min_text_pt,
        min_stroke_pt: o.lettering.min_stroke_pt,
        safe_mm: o.lettering.safe_mm,
        min_delta_l: o.inks.min_delta_l,
        proof_profile: o.inks.proof_profile.as_ref().map(|p| p.display().to_string()),
        geometry,
    };
    let pdfx = (!o.layers.no_pdfx).then(|| match o.layers.pdfx.unwrap_or_default() {
        Pdfx::X4 => Level::X4,
        Pdfx::X4p => Level::X4p,
        Pdfx::X5g => Level::X5g,
        Pdfx::X5pg => Level::X5pg,
        Pdfx::X5n => Level::X5n,
        Pdfx::X1a2001 => Level::X1a2001,
        Pdfx::X32002 => Level::X32002,
        Pdfx::X1a2003 => Level::X1a2003,
        Pdfx::X32003 => Level::X32003,
    });
    let timeout = Duration::from_secs_f64(o.timeout.unwrap_or(900.0).max(0.0));
    let mut tools = Vec::new();
    if o.layers.pdfa {
        let program = o.verapdf.clone().unwrap_or_else(|| "verapdf".to_owned());
        let flavour = o.layers.flavour.clone().unwrap_or_else(|| "2b".to_owned());
        tools.push(Tool::Verapdf {
            program,
            flavour,
            timeout,
        });
    }
    if o.layers.tac {
        let program = o.gs.clone().unwrap_or_else(|| "gs".to_owned());
        tools.push(Tool::Ghostscript {
            program,
            max: max_tac,
            warn: o.layers.warn_tac,
            timeout,
        });
    }
    let (wanted, switchable) = wanted(o, &params);
    Ok(Plan {
        wanted,
        switchable,
        tools,
        pdfx,
        detect: o.layers.pdfx.is_none(),
        settings: Settings {
            level: Level::Print,
            params,
            files: Files::Alone,
        },
        file_alone: o.file_alone,
    })
}

fn geometry(o: &Verify, trim: &str) -> Result<Geometry, Usage> {
    let normal = trim.to_lowercase().replace('*', "x");
    let sizes: Vec<Option<f64>> = normal.split('x').map(|p| p.trim().parse().ok()).collect();
    let [Some(width), Some(height)] = sizes[..] else {
        return Err(Usage(format!("--trim-mm must look like 105x148, got {trim:?}")));
    };
    if width <= 0.0 || height <= 0.0 {
        return Err(Usage(format!("--trim-mm must be positive, got {trim:?}")));
    }
    Ok(Geometry {
        trim_w_mm: width,
        trim_h_mm: height,
        bleed_mm: o.sizes.bleed_mm.unwrap_or(3.0),
        tolerance_mm: o.sizes.tolerance_mm.unwrap_or(0.25),
        binding_bleed_mm: o.sizes.binding_bleed_mm,
        binding: match o.sizes.binding.unwrap_or_default() {
            Binding::Left => "left".to_owned(),
            Binding::Right => "right".to_owned(),
        },
    })
}

/// The print checks the options turn on, as the Python profile did, and every print check they could.
fn wanted(o: &Verify, params: &Params) -> (BTreeSet<&'static str>, BTreeSet<&'static str>) {
    let colour = !o.inks.no_colour;
    let mode = o.inks.colour_mode.unwrap_or_default();
    let switches: [(bool, &[&'static str]); 15] = [
        (
            true,
            &[
                "outside-media",
                "xobject-missing",
                "page-count",
                "text-size",
                "hairline",
                "safe-area",
            ],
        ),
        (!o.lettering.no_missing_glyphs, &["missing-glyph"]),
        (!o.lettering.no_fonts, &["font-embedded", "font-type3"]),
        (
            colour && mode == ColourMode::Cmyk,
            &["overprint", "white-overprint", "rich-black", "colour-space"],
        ),
        (colour && mode == ColourMode::Rgb, &["rgb-only"]),
        (colour && mode == ColourMode::Gray, &["gray-only"]),
        (
            colour,
            &[
                "blend-space",
                "icc-profile",
                "image-dpi",
                "image-dpi-max",
                "image-stretch",
                "spot-definition",
            ],
        ),
        (o.lettering.k_only_text, &["k-only-text"]),
        (params.geometry.is_some(), &["geometry", "bleed", "centring"]),
        (params.uniform_pages, &["page-size", "page-rotate"]),
        (params.output_condition.is_some(), &["output-condition"]),
        (o.sizes.require_bleed, &["artwork-bleed"]),
        (o.sizes.no_marks, &["outside-bleed"]),
        (params.allow_spots.is_some(), &["spot-ink"]),
        (params.min_delta_l.is_some(), &["contrast"]),
    ];
    let all = |on: bool| {
        switches
            .iter()
            .filter(|(o, _)| on || *o)
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect()
    };
    (all(false), all(true))
}

/// The PDFs the arguments name: a file as given, a directory as every `.pdf` beneath it, sorted, but for paths
/// relative to it that match an `--exclude` glob; each once, in first-seen order.
pub(crate) fn targets(paths: &[PathBuf], exclude: &[String]) -> Result<Vec<PathBuf>, Usage> {
    let excluded = exclude
        .iter()
        .map(|g| matcher(g, "--exclude"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out: Vec<PathBuf> = Vec::new();
    for path in paths {
        let found = if path.is_dir() {
            let mut found = Vec::new();
            pdfs_under(path, &mut found);
            found.sort();
            found.retain(|p| {
                let relative = p.strip_prefix(path).unwrap_or(p);
                !excluded.iter().any(|g| g.is_match(relative))
            });
            found
        } else {
            vec![path.clone()]
        };
        for p in found {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    Ok(out)
}

fn pdfs_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "pdf") {
            out.push(path);
        }
    }
}

fn matcher(glob: &str, flag: &str) -> Result<GlobMatcher, Usage> {
    Glob::new(glob)
        .map(|g| g.compile_matcher())
        .map_err(|e| Usage(format!("{flag} {glob:?} is not a glob: {e}")))
}

/// `GLOB=reason` exemptions; a missing reason is refused, since the reason is the point.
pub(crate) fn exemptions(specs: &[String]) -> Result<Vec<(GlobMatcher, String)>, Usage> {
    specs
        .iter()
        .map(|spec| {
            let (glob, reason) = spec.split_once('=').unwrap_or((spec, ""));
            if glob.trim().is_empty() || reason.trim().is_empty() {
                return Err(Usage(format!("--exempt must look like GLOB=reason, got {spec:?}")));
            }
            // A glob matches at any directory level.
            let glob = glob.trim();
            let any_level = if glob.starts_with("**/") {
                glob.to_owned()
            } else {
                format!("{{{glob},**/{glob}}}")
            };
            Ok((matcher(&any_level, "--exempt")?, reason.trim().to_owned()))
        })
        .collect()
}

/// Verify one file at the print level and, when asked, its PDF/X level: the print findings `plan` turns on, the
/// PDF/X findings, and the checks the core could not judge as failures.
pub(crate) fn verify(path: &Path, plan: &Plan, exemptions: &[(GlobMatcher, String)]) -> FileReport {
    if let Some((_, reason)) = exemptions.iter().find(|(g, _)| g.is_match(path)) {
        return FileReport {
            path: path.to_owned(),
            exempt: Some(reason.clone()),
            report: None,
            unreadable: None,
            timed_out: false,
        };
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            let unreadable = Some(format!("cannot read the file: {}", io_message(&e)));
            return FileReport {
                path: path.to_owned(),
                exempt: None,
                report: None,
                unreadable,
                timed_out: false,
            };
        }
    };
    // The tools run while the core checks the file.
    let (tools, report) = std::thread::scope(|scope| {
        let tools: Vec<_> = plan
            .tools
            .iter()
            .map(|tool| scope.spawn(move || tool.run(path)))
            .collect();
        let report = combined(&path.display().to_string(), &bytes, plan);
        (
            tools.into_iter().filter_map(|t| t.join().ok()).collect::<Vec<_>>(),
            report,
        )
    });
    let timed_out = tools.iter().any(|(_, v)| v.timed_out);
    let mut checks = report.checks;
    let mut findings = report.findings;
    for (check, verdict) in tools {
        checks.push(CheckResult {
            id: check,
            outcome: verdict.outcome,
        });
        findings.extend(verdict.findings);
    }
    let report = Report {
        level: report.level,
        ..Report::new(report.file, report.structure, checks, findings)
    };
    FileReport {
        path: path.to_owned(),
        exempt: None,
        report: Some(report),
        unreadable: None,
        timed_out,
    }
}

/// An I/O error's message without the "(os error N)" the OS's own words already say.
pub(crate) fn io_message(e: &std::io::Error) -> String {
    let text = e.to_string();
    text.split(" (os error ").next().unwrap_or_default().to_owned()
}

fn combined(name: &str, bytes: &[u8], plan: &Plan) -> Report {
    let print = checks::verify_settings(name, bytes, &plan.settings);
    // A print check is kept when its option is on; what the reading tiers found, when no PDF/X level reports it too.
    let keep = |id: &str| {
        if plan.switchable.contains(id) {
            plan.wanted.contains(id)
        } else {
            plan.pdfx.is_none()
        }
    };
    let mut checks: Vec<_> = print.checks.into_iter().filter(|c| keep(c.id.id)).collect();
    let mut findings: Vec<Finding> = print.findings.into_iter().filter(|f| keep(f.check.id)).collect();
    let mut structure = print.structure;
    let mut level = None;
    if let Some(at) = plan.pdfx {
        let folder = Path::new(name).parent().map_or_default(Path::to_owned);
        let settings = Settings {
            level: at,
            params: plan.settings.params.clone(),
            files: if plan.file_alone {
                Files::Alone
            } else {
                Files::Set(folder)
            },
        };
        let pdfx = if plan.detect {
            checks::verify_claimed(name, bytes, &settings)
        } else {
            checks::verify_settings(name, bytes, &settings)
        };
        checks.extend(pdfx.checks);
        findings.extend(pdfx.findings);
        structure = pdfx.structure;
        level = pdfx.level;
    }
    Report {
        level,
        ..Report::new(name, structure, checks, findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Command};
    use clap::Parser;

    #[test]
    fn directories_are_walked_and_exemptions_match_at_any_level() {
        let root = std::env::temp_dir().join(format!("proof-pdf-cli-{}", std::process::id()));
        let deep = root.join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        for name in ["a/one.pdf", "a/b/two.pdf", "a/notes.txt", "a/b/skip.pdf"] {
            std::fs::write(root.join(name), b"%PDF-1.7").unwrap();
        }
        let found = targets(&[root.clone(), root.join("a/one.pdf")], &["**/skip.pdf".to_owned()]).unwrap();
        assert_eq!(found, [root.join("a/b/two.pdf"), root.join("a/one.pdf")]);
        let exempt = exemptions(&["**/two.pdf=draft".to_owned(), "one.pdf = proof ".to_owned()]).unwrap();
        assert!(exempt[0].0.is_match(root.join("a/b/two.pdf")) && exempt[1].0.is_match(root.join("a/one.pdf")));
        assert_eq!(exempt[1].1, "proof");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_binding_edge() {
        let words = [
            "proof-pdf",
            "verify",
            "x.pdf",
            "--trim-mm",
            "100x200",
            "--binding",
            "right",
        ];
        let Command::Verify(o) = Cli::try_parse_from(words).unwrap().command;
        let geometry = plan(&o).unwrap().settings.params.geometry.unwrap();
        assert_eq!((geometry.binding.as_str(), geometry.trim_w_mm), ("right", 100.0));
    }
}
