//! The command line's arguments, as clap reads them. The `verify` options keep the Python command's names; PDF/A and
//! peak ink are opt-in (`--pdfa`, `--tac`), since they need veraPDF and Ghostscript, and `--pdfx` takes a level.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Verify and proof print PDFs (PDF/X-4, PDF/A and the print checks).
#[derive(Debug, Parser)]
#[command(name = "proof-pdf", version)]
pub(crate) struct Cli {
    /// Config file (default: the nearest proof-pdf.toml, or [tool.proof-pdf] in pyproject.toml).
    #[arg(long, global = true, env = "PROOF_PDF_CONFIG")]
    pub(crate) config: Option<PathBuf>,
    /// Read no config file, even one nearby.
    #[arg(long, global = true, conflicts_with = "config")]
    pub(crate) no_config: bool,
    /// Named profile in the config file, e.g. a printer.
    #[arg(long, global = true, env = "PROOF_PDF_PROFILE")]
    pub(crate) profile: Option<String>,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Verify each PDF against the PDF/X level, the print checks and, when asked, PDF/A and peak ink.
    Verify(Box<Verify>),
}

/// The job's colour model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum ColourMode {
    #[default]
    Cmyk,
    Rgb,
    Gray,
}

/// The bound edge of page 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum Binding {
    #[default]
    Left,
    Right,
}

/// Whether a PDF/X-5 target that is another version of the document a reference names is accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum TargetVersions {
    Accept,
    Reject,
}

/// The PDF/X level `--pdfx` holds files to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum Pdfx {
    #[default]
    X4,
    X4p,
    X5g,
    X5pg,
    X5n,
    #[value(name = "x1a-2001")]
    X1a2001,
    #[value(name = "x3-2002")]
    X32002,
    #[value(name = "x1a-2003")]
    X1a2003,
    #[value(name = "x3-2003")]
    X32003,
}

/// The `verify` command's options.
#[derive(Debug, Default, Args)]
pub(crate) struct Verify {
    /// PDFs to verify. A directory contributes every PDF beneath it.
    pub(crate) files: Vec<PathBuf>,
    /// Glob, relative to a directory argument, of PDFs to skip. Repeatable.
    #[arg(long)]
    pub(crate) exclude: Vec<String>,
    /// GLOB=reason: skip matching PDFs, listing them with the reason. Matches at any directory level.
    #[arg(long)]
    pub(crate) exempt: Vec<String>,
    /// Files to verify at once [default: 4].
    #[arg(long, short, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) jobs: Option<u64>,
    /// veraPDF executable [default: verapdf].
    #[arg(long)]
    pub(crate) verapdf: Option<String>,
    /// Ghostscript executable [default: gs].
    #[arg(long)]
    pub(crate) gs: Option<String>,
    /// Per-tool timeout, in seconds [default: 900].
    #[arg(long)]
    pub(crate) timeout: Option<f64>,
    /// PDF/X-5: accept or reject a target that is another version of the document a reference names. With neither,
    /// such a target is not evaluated.
    #[arg(long, value_enum)]
    pub(crate) target_versions: Option<TargetVersions>,
    /// Verify each file alone: checks that need a file it refers to (a PDF/X-4p file's external profile) are not
    /// evaluated. By default the files a PDF refers to are looked for beside it.
    #[arg(long)]
    pub(crate) file_alone: bool,
    #[command(flatten)]
    pub(crate) layers: Layers,
    #[command(flatten)]
    pub(crate) inks: Inks,
    #[command(flatten)]
    pub(crate) lettering: Type,
    #[command(flatten)]
    pub(crate) sizes: Sizes,
    #[command(flatten)]
    pub(crate) output: Output,
    #[command(flatten)]
    pub(crate) legacy: Legacy,
    #[command(flatten)]
    pub(crate) legacy_text: LegacyText,
}

impl Verify {
    /// The first option set for an opt-in layer that is off, as (option, the layer's switch, the layer). The tool
    /// paths and `--timeout` are left out: a shared config names them once for runs with and without the layers.
    pub(crate) fn idle(&self, set: impl Fn(&str) -> bool) -> Option<(&'static str, &'static str, &'static str)> {
        let l = &self.layers;
        [
            ("flavour", l.flavour.is_some() && !l.pdfa, "pdfa", "PDF/A"),
            ("max-tac", l.max_tac.is_some() && !l.tac, "tac", "total ink"),
            ("warn-tac", l.warn_tac.is_some() && !l.tac, "tac", "total ink"),
        ]
        .into_iter()
        .find(|(option, idle, _, _)| *idle && set(option))
        .map(|(option, _, switch, layer)| (option, switch, layer))
    }
}

/// Which layers run: the PDF/X level, PDF/A and peak ink.
#[derive(Debug, Default, Args)]
pub(crate) struct Layers {
    /// PDF/X level to hold the files to.
    #[arg(long, value_enum)]
    pub(crate) pdfx: Option<Pdfx>,
    /// Skip the PDF/X checks.
    #[arg(long, conflicts_with = "pdfx")]
    pub(crate) no_pdfx: bool,
    /// Run veraPDF's PDF/A validation (needs veraPDF).
    #[arg(long, overrides_with = "no_pdfa")]
    pub(crate) pdfa: bool,
    /// veraPDF PDF/A flavour: 2b, 2a, 2u, 1b, 3b, 4...
    #[arg(long)]
    pub(crate) flavour: Option<String>,
    /// Measure peak ink with Ghostscript (needs Ghostscript).
    #[arg(long)]
    pub(crate) tac: bool,
    /// Peak ink ceiling, in percent [default: 300].
    #[arg(long)]
    pub(crate) max_tac: Option<f64>,
    /// Warn when peak ink is above this, below --max-tac, in percent. No default.
    #[arg(long)]
    pub(crate) warn_tac: Option<f64>,
}

/// The colour checks.
#[derive(Debug, Default, Args)]
pub(crate) struct Inks {
    /// Skip the ink, overprint, RGB, rich black and image resolution checks.
    #[arg(long, overrides_with = "colour")]
    pub(crate) no_colour: bool,
    /// Colour model the job is proofed against.
    #[arg(long, value_enum)]
    pub(crate) colour_mode: Option<ColourMode>,
    /// Fail, rather than warn, on RGB vector fills.
    #[arg(long)]
    pub(crate) strict_cmyk: bool,
    /// With --colour-mode gray, also accept device CMYK that uses only the K plate.
    #[arg(long)]
    pub(crate) allow_k_only: bool,
    /// Rich-black cutoff, in points [default: 12].
    #[arg(long)]
    pub(crate) small_text_pt: Option<f64>,
    /// Minimum effective resolution for placed images [default: 300].
    #[arg(long)]
    pub(crate) min_dpi: Option<f64>,
    /// Warn on images below this resolution but at or above --min-dpi. No default.
    #[arg(long)]
    pub(crate) warn_dpi: Option<f64>,
    /// Warn on images placed above this resolution. No default.
    #[arg(long)]
    pub(crate) max_dpi: Option<f64>,
    /// Warn when an image's two axes differ by more than this percent. No default.
    #[arg(long)]
    pub(crate) max_stretch_pct: Option<f64>,
    /// Fail any spot ink not named here (exact match; repeatable).
    #[arg(long)]
    pub(crate) allow_spot: Option<Vec<String>>,
    /// Fail unless an output intent names this printing condition, e.g. FOGRA51.
    #[arg(long)]
    pub(crate) output_condition: Option<String>,
    /// Fail text whose L* difference from its background is below this. No default.
    #[arg(long)]
    pub(crate) min_delta_l: Option<f64>,
    /// ICC profile to measure contrast through (default: the PDF's output intents).
    #[arg(long)]
    pub(crate) proof_profile: Option<PathBuf>,
}

/// The text and font checks.
#[derive(Debug, Default, Args)]
pub(crate) struct Type {
    /// Fail text painted with any ink but K.
    #[arg(long)]
    pub(crate) k_only_text: bool,
    /// Do not flag characters shown with the .notdef glyph.
    #[arg(long, overrides_with = "missing_glyphs")]
    pub(crate) no_missing_glyphs: bool,
    /// Do not flag fonts that are not embedded, or Type 3 fonts.
    #[arg(long, overrides_with = "fonts")]
    pub(crate) no_fonts: bool,
    /// Fail text set smaller than this, in points. No default.
    #[arg(long)]
    pub(crate) min_text_pt: Option<f64>,
    /// Fail lines thinner than this, in points. No default.
    #[arg(long)]
    pub(crate) min_stroke_pt: Option<f64>,
    /// Fail text outside this inset from the trim edge, in mm. No default.
    #[arg(long)]
    pub(crate) safe_mm: Option<f64>,
}

/// The page geometry checks.
#[derive(Debug, Default, Args)]
pub(crate) struct Sizes {
    /// Expected finished size as `WxH` in mm, e.g. 105x148. Turns the geometry checks on.
    #[arg(long)]
    pub(crate) trim_mm: Option<String>,
    /// Expected bleed per side, in mm [default: 3].
    #[arg(long)]
    pub(crate) bleed_mm: Option<f64>,
    /// Bleed on the binding edge, in mm (default: --bleed-mm). Book interiors: 0.
    #[arg(long)]
    pub(crate) binding_bleed_mm: Option<f64>,
    /// Which edge of page 1 is bound; the other side on page 2, and so on.
    #[arg(long, value_enum)]
    pub(crate) binding: Option<Binding>,
    /// Allowed geometry deviation, in mm [default: 0.25].
    #[arg(long)]
    pub(crate) tolerance_mm: Option<f64>,
    /// Fail artwork that reaches the trim edge but stops short of the bleed edge.
    #[arg(long)]
    pub(crate) require_bleed: bool,
    /// Fail anything painted beyond the bleed region: printer's marks, slug text.
    #[arg(long)]
    pub(crate) no_marks: bool,
    /// Fail pages whose finished size differs from the first page's.
    #[arg(long)]
    pub(crate) uniform_pages: bool,
    /// Fail a page count that is not a multiple of N.
    #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
    pub(crate) pages_multiple_of: Option<i64>,
}

/// How the reports are written.
#[derive(Debug, Default, Args)]
pub(crate) struct Output {
    /// Write JSON reports instead of text.
    #[arg(long)]
    pub(crate) json: bool,
    /// Print nothing; report by exit code only (for CI).
    #[arg(long)]
    pub(crate) quiet: bool,
    /// Print the settings the config file and profile give, and exit.
    #[arg(long)]
    pub(crate) show_profile: bool,
}

/// The Python command's spellings of the defaults, still accepted: the last of `--x` and `--no-x` wins.
#[derive(Debug, Default, Args)]
pub(crate) struct Legacy {
    #[arg(long, hide = true, overrides_with = "pdfa")]
    pub(crate) no_pdfa: bool,
    #[arg(long, hide = true, overrides_with = "no_colour")]
    pub(crate) colour: bool,
    #[arg(long, hide = true, overrides_with = "no_fonts")]
    pub(crate) fonts: bool,
}

/// More of the Python command's spellings of the defaults.
#[derive(Debug, Default, Args)]
pub(crate) struct LegacyText {
    #[arg(long, hide = true, overrides_with = "no_missing_glyphs")]
    pub(crate) missing_glyphs: bool,
}
