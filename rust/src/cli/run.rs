//! What `verify` does with its options: which checks they turn on, which files they name, and the report on each.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher};

use super::args::{Binding, ColourMode, Pdfx, Verify};
use crate::checks::{self, Geometry, Params, Settings};
use crate::findings::{Finding, Level, Report, Verdict};

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

/// The print checks' parameters and which of them the options turn on.
pub(crate) struct Plan {
    pub(crate) pdfx: Option<Level>,
    pub(crate) settings: Settings,
    pub(crate) wanted: BTreeSet<&'static str>,
}

/// The plan `verify`'s options describe.
pub(crate) fn plan(o: &Verify) -> Result<Plan, Usage> {
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
    });
    Ok(Plan {
        pdfx,
        wanted: wanted(o, &params),
        settings: Settings {
            level: Level::Print,
            params,
        },
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

/// The print checks the options turn on, as the Python profile did.
fn wanted(o: &Verify, params: &Params) -> BTreeSet<&'static str> {
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
    switches
        .iter()
        .filter(|(on, _)| *on)
        .flat_map(|(_, ids)| ids.iter().copied())
        .collect()
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
        };
    }
    let (report, unreadable) = match std::fs::read(path) {
        Ok(bytes) => (Some(combined(&path.display().to_string(), &bytes, plan)), None),
        Err(e) => (None, Some(format!("cannot read the file: {e}"))),
    };
    FileReport {
        path: path.to_owned(),
        exempt: None,
        report,
        unreadable,
    }
}

fn combined(name: &str, bytes: &[u8], plan: &Plan) -> Report {
    let print = checks::verify_settings(name, bytes, &plan.settings);
    let mut checks: Vec<_> = print
        .checks
        .into_iter()
        .filter(|c| plan.wanted.contains(c.id.id))
        .collect();
    let mut findings: Vec<Finding> = print
        .findings
        .into_iter()
        .filter(|f| plan.wanted.contains(f.check.id))
        .collect();
    let mut structure = print.structure;
    if let Some(level) = plan.pdfx {
        let settings = Settings {
            level,
            params: plan.settings.params.clone(),
        };
        let pdfx = checks::verify_settings(name, bytes, &settings);
        checks.extend(pdfx.checks);
        findings.extend(pdfx.findings);
        structure = pdfx.structure;
    }
    Report::new(name, structure, checks, findings)
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
