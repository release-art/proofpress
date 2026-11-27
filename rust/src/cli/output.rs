//! `verify`'s reports: text for people, JSON for machines, and the exit code.

use std::path::Path;

use super::args::{Pdfx, Verify};
use super::run::{FileReport, Plan};
use crate::findings::{Finding, Outcome, Severity, Verdict};

/// The exit codes (brief, stage 4): with several files the worst wins, refused, then a timeout, then findings.
pub(crate) const PASS: i32 = 0;
pub(crate) const FINDINGS: i32 = 1;
pub(crate) const REFUSED: i32 = 2;
pub(crate) const TIMEOUT: i32 = 3;
pub(crate) const USAGE: i32 = 64;

pub(crate) fn exit_code(reports: &[FileReport]) -> i32 {
    let refused =
        |r: &FileReport| r.unreadable.is_some() || r.report.as_ref().is_some_and(|r| r.verdict == Verdict::Refused);
    if reports.iter().any(refused) {
        REFUSED
    } else if reports.iter().any(|r| r.timed_out) {
        TIMEOUT
    } else if reports.iter().all(FileReport::ok) {
        PASS
    } else {
        FINDINGS
    }
}

/// Colour codes for a terminal, none otherwise.
pub(crate) struct Style {
    pub(crate) colour: bool,
}

impl Style {
    fn paint(&self, code: &str, text: &str) -> String {
        if self.colour {
            format!("\u{1b}[{code}m{text}\u{1b}[0m")
        } else {
            text.to_owned()
        }
    }
}

/// Each file's report, then the summary.
pub(crate) fn text(reports: &[FileReport], o: &Verify, plan: &Plan, style: &Style) -> String {
    let mut lines = Vec::new();
    for report in reports {
        let path = relative(&report.path);
        if let Some(reason) = &report.exempt {
            lines.push(format!("  {path:<52} {}", style.paint("33", "EXEMPT")));
            lines.push(format!("      - {reason}"));
            continue;
        }
        let status = if report.ok() {
            style.paint("32", "OK")
        } else {
            style.paint("31", "FAIL")
        };
        // A file verified at the older level it claims, not the default, says so.
        let claimed = report
            .report
            .as_ref()
            .and_then(|r| r.level)
            .filter(|l| plan.detect && *l != "PDF/X-4");
        let at = claimed.map_or_default(|l| format!(" (as {l})"));
        lines.push(format!("  {path:<52} {status}{at}"));
        lines.extend(
            report
                .unreadable
                .iter()
                .map(|why| format!("      {} {why}", style.paint("31", "x"))),
        );
        for (finding, pages) in collapse(report.findings()) {
            let mark = match finding.severity {
                Severity::Error => style.paint("31", "x"),
                Severity::Warning => style.paint("33", "!"),
            };
            lines.push(format!("      {mark} {}", labelled(finding, &pages)));
        }
        for (reason, ids) in unjudged(report) {
            let mark = style.paint("31", "x");
            lines.push(format!("      {mark} not evaluated, {reason}: {}", ids.join(", ")));
        }
    }
    lines.push(String::new());
    lines.push(summary(reports, o, plan, style));
    lines.join("\n") + "\n"
}

/// The checks that could not be judged, grouped by why, in first-seen order: a damaged file leaves most of them
/// unjudged for the same reason, which is one line, not one per check.
fn unjudged(report: &FileReport) -> Vec<(&str, Vec<&str>)> {
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    let results = report.report.iter().flat_map(|r| &r.checks);
    for (id, reason) in results.filter_map(|c| match &c.outcome {
        Outcome::NotEvaluated(reason) => Some((c.id.id, reason.as_str())),
        Outcome::Pass | Outcome::Fail => None,
    }) {
        match groups.iter_mut().find(|(r, _)| *r == reason) {
            Some((_, ids)) => ids.push(id),
            None => groups.push((reason, vec![id])),
        }
    }
    groups
}

/// Findings that differ only by page, as one line naming the pages, in first-seen order.
fn collapse(findings: &[Finding]) -> Vec<(&Finding, Vec<u32>)> {
    let mut groups: Vec<(&Finding, Vec<u32>)> = Vec::new();
    for finding in findings {
        let same =
            |f: &&Finding| f.check == finding.check && f.severity == finding.severity && f.message == finding.message;
        match groups.iter_mut().find(|(f, _)| same(f)) {
            Some((_, pages)) => pages.extend(finding.page),
            None => groups.push((finding, finding.page.into_iter().collect())),
        }
    }
    groups
}

fn labelled(finding: &Finding, pages: &[u32]) -> String {
    let id = finding.check.id;
    match pages {
        [] => format!("[{id}] {}", finding.message),
        [page] => format!("[{id}] (page {page}) {}", finding.message),
        _ => format!("[{id}] (pages {}) {}", ranges(pages), finding.message),
    }
}

/// `[1, 2, 3, 7]` as `1-3, 7`.
fn ranges(pages: &[u32]) -> String {
    let mut sorted = pages.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut spans: Vec<(u32, u32)> = Vec::new();
    for page in sorted {
        match spans.last_mut() {
            Some((_, end)) if page == *end + 1 => *end = page,
            _ => spans.push((page, page)),
        }
    }
    let spans: Vec<String> = spans
        .iter()
        .map(|(a, b)| if a == b { a.to_string() } else { format!("{a}-{b}") })
        .collect();
    spans.join(", ")
}

/// `path` relative to the working directory when it is inside it.
fn relative(path: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let here = std::env::current_dir().unwrap_or_default();
    absolute.strip_prefix(&here).unwrap_or(path).display().to_string()
}

fn level_name(level: Pdfx) -> &'static str {
    match level {
        Pdfx::X4 => "PDF/X-4",
        Pdfx::X4p => "PDF/X-4p",
        Pdfx::X5g => "PDF/X-5g",
        Pdfx::X5pg => "PDF/X-5pg",
        Pdfx::X5n => "PDF/X-5n",
        Pdfx::X1a2001 => "PDF/X-1a:2001",
        Pdfx::X32002 => "PDF/X-3:2002",
        Pdfx::X1a2003 => "PDF/X-1a:2003",
        Pdfx::X32003 => "PDF/X-3:2003",
    }
}

fn summary(reports: &[FileReport], o: &Verify, plan: &Plan, style: &Style) -> String {
    let p = &plan.settings.params;
    let flavour = format!("PDF/A-{}", o.layers.flavour.as_deref().unwrap_or("2b"));
    let named: [(&str, bool); 17] = [
        (flavour.as_str(), o.layers.pdfa),
        (
            o.layers.pdfx.map_or("PDF/X as claimed", level_name),
            plan.pdfx.is_some(),
        ),
        ("peak ink", o.layers.tac),
        ("geometry", p.geometry.is_some()),
        ("colour", !o.inks.no_colour),
        ("K-only text", o.lettering.k_only_text),
        ("missing glyphs", !o.lettering.no_missing_glyphs),
        ("min text size", p.min_text_pt.is_some()),
        ("hairlines", p.min_stroke_pt.is_some()),
        ("safe area", p.safe_mm.is_some()),
        ("contrast", p.min_delta_l.is_some()),
        ("fonts", !o.lettering.no_fonts),
        ("spot inks", p.allow_spots.is_some()),
        ("output condition", p.output_condition.is_some()),
        ("artwork bleed", o.sizes.require_bleed),
        ("no marks", o.sizes.no_marks),
        ("page set", p.uniform_pages || p.pages_multiple_of.is_some()),
    ];
    let layers: Vec<&str> = named.iter().filter(|(_, on)| *on).map(|(n, _)| *n).collect();
    let layers = layers.join(", ");
    let failed = reports.iter().filter(|r| !r.ok()).count();
    let exempt = reports.iter().filter(|r| r.exempt.is_some()).count();
    let checked = reports.len() - exempt;
    if failed > 0 {
        let line = format!("FAIL -- {failed} of {checked} file(s) failed: {layers}");
        return style.paint("31", &line);
    }
    let warnings: usize = reports
        .iter()
        .map(|r| r.findings().iter().filter(|f| f.severity == Severity::Warning).count())
        .sum();
    let mut notes = Vec::new();
    if warnings > 0 {
        notes.push(format!("{warnings} advisory warning(s)"));
    }
    if exempt > 0 {
        notes.push(format!("{exempt} exempt"));
    }
    let note = if notes.is_empty() {
        String::new()
    } else {
        format!(" ({})", notes.join(", "))
    };
    let line = format!("OK -- all {checked} file(s) passed: {layers}{note}");
    style.paint("32", &line)
}

/// Every file's record: the core's report, with the path, whether it passed, and why it was exempt or unread.
pub(crate) fn json(reports: &[FileReport]) -> String {
    let records: Vec<serde_json::Value> = reports
        .iter()
        .map(|r| {
            let mut record = serde_json::Map::new();
            record.insert("path".into(), r.path.display().to_string().into());
            record.insert("ok".into(), r.ok().into());
            record.insert("exempt".into(), r.exempt.clone().into());
            record.insert("unreadable".into(), r.unreadable.clone().into());
            record.insert("findings".into(), serde_json::Value::Array(Vec::new()));
            record.insert("checks".into(), serde_json::Value::Array(Vec::new()));
            let core = r.report.as_ref().and_then(|c| serde_json::to_value(c).ok());
            record.extend(
                core.as_ref()
                    .and_then(serde_json::Value::as_object)
                    .into_iter()
                    .flatten()
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
            serde_json::Value::Object(record)
        })
        .collect();
    serde_json::to_string_pretty(&records).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ranges_and_relative_paths() {
        assert_eq!(ranges(&[7, 1, 2, 3, 3]), "1-3, 7");
        assert_eq!(relative(Path::new("")), "");
        assert_eq!(relative(&std::env::current_dir().unwrap().join("a.pdf")), "a.pdf");
    }
}
