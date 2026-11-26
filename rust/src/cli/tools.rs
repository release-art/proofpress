//! The external tools `verify` can ask for: veraPDF for PDF/A (`--pdfa`) and Ghostscript for peak ink (`--tac`).
//!
//! Each runs in its own process group (a job object on Windows), killed if it outlives its timeout or the run; its
//! output is read on threads as it comes. A tool that is not installed, fails, or times out leaves its check failed,
//! never silently passed: the point is to confirm a file.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
use process_wrap::std::JobObject;
#[cfg(unix)]
use process_wrap::std::ProcessGroup;
use process_wrap::std::{CommandWrap, KillOnDrop};

use crate::findings::{Finding, Location, Outcome, PDFA, Severity, TAC};

/// What a tool's check came to.
pub(crate) struct Verdict {
    pub(crate) findings: Vec<Finding>,
    pub(crate) outcome: Outcome,
    /// Whether the tool ran out of time, which the exit code reports.
    pub(crate) timed_out: bool,
}

impl Verdict {
    fn from(findings: Vec<Finding>) -> Self {
        let failed = findings.iter().any(|f| f.severity == Severity::Error);
        Self {
            findings,
            outcome: if failed { Outcome::Fail } else { Outcome::Pass },
            timed_out: false,
        }
    }

    fn failed(finding: Finding, timed_out: bool) -> Self {
        Self {
            findings: vec![finding],
            outcome: Outcome::Fail,
            timed_out,
        }
    }
}

/// Why a run did not finish with an exit status.
enum Failure {
    Missing,
    TimedOut,
    Broken(String),
}

/// A finished run: its status and what it wrote.
struct Ran {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn run(program: &str, args: &[OsString], timeout: Duration) -> Result<Ran, Failure> {
    let mut command = CommandWrap::with_new(program, |c| {
        c.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);
    command.wrap(KillOnDrop);
    let mut child = command.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Failure::Missing,
        _ => Failure::Broken(e.to_string()),
    })?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            pipe.map_or_else(String::new, |mut pipe| {
                let mut text = String::new();
                // What was read before a read error is kept; the exit status is the verdict.
                let _ = pipe.read_to_string(&mut text);
                text
            })
        })
    };
    let stdout = drain(child.stdout().take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = drain(child.stderr().take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let started = Instant::now();
    // A child whose status cannot be read is waited on until the deadline, then killed.
    let status = loop {
        if let Some(status) = child.try_wait().ok().flatten() {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            return Err(Failure::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let text = |handle: std::thread::JoinHandle<String>| handle.join().unwrap_or_default();
    Ok(Ran {
        status,
        stdout: text(stdout),
        stderr: text(stderr),
    })
}

fn seconds(timeout: Duration) -> String {
    format!("{:.0}s", timeout.as_secs_f64())
}

/// veraPDF's verdict on `path` against PDF/A `flavour`: a finding per failed rule.
pub(crate) fn verapdf(program: &str, flavour: &str, path: &Path, timeout: Duration) -> Verdict {
    let fail = |message: String, timed_out| Verdict::failed(Finding::new(PDFA, message), timed_out);
    let args: Vec<OsString> = ["--format", "json", "-f", flavour]
        .iter()
        .map(OsString::from)
        .chain([path.into()])
        .collect();
    match run(program, &args, timeout) {
        Err(Failure::Missing) => fail(
            format!("veraPDF not found ({program}) -- cannot confirm PDF/A-{flavour}"),
            false,
        ),
        Err(Failure::TimedOut) => fail(
            format!(
                "veraPDF could not validate the file: timed out after {}",
                seconds(timeout)
            ),
            true,
        ),
        Err(Failure::Broken(why)) => fail(format!("veraPDF could not validate the file: {why}"), false),
        // veraPDF exits 0 for a valid file and 1 for an invalid one; anything else is not a verdict.
        Ok(ran) if !matches!(ran.status.code(), Some(0 | 1)) => {
            let said = useful(&ran.stderr);
            let why = if said.is_empty() {
                format!("veraPDF exited {}", ran.status)
            } else {
                said
            };
            fail(format!("veraPDF could not validate the file: {why}"), false)
        }
        Ok(ran) => match serde_json::from_str::<serde_json::Value>(&ran.stdout) {
            Ok(report) => Verdict::from(verapdf_findings(&report, flavour)),
            Err(e) => fail(
                format!("veraPDF could not validate the file: veraPDF emitted unreadable JSON: {e}"),
                false,
            ),
        },
    }
}

/// `stderr` without the Java runtime's and veraPDF's logger lines around the one that matters.
fn useful(stderr: &str) -> String {
    const JVM: [&str; 3] = [
        "WARNING: Final field",
        "WARNING: Use --enable-final-field-mutation",
        "WARNING: Mutating final fields",
    ];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let logger =
        |line: &str| line.contains(" org.verapdf.") && MONTHS.iter().any(|m| line.starts_with(&format!("{m} ")));
    let kept: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !JVM.iter().any(|j| l.starts_with(j)) && !logger(l))
        .map(|l| l.strip_prefix("WARNING: ").unwrap_or(l))
        .collect();
    kept.join(" ")
}

fn verapdf_findings(report: &serde_json::Value, flavour: &str) -> Vec<Finding> {
    let error = |message: String| Finding::new(PDFA, message);
    let jobs = report
        .pointer("/report/jobs")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if jobs.is_empty() {
        return vec![error("veraPDF returned no validation result".to_owned())];
    }
    let mut out = Vec::new();
    for job in &jobs {
        let results = job
            .get("validationResult")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        if results.is_empty() {
            out.push(error("veraPDF could not validate the file".to_owned()));
        }
        for result in results
            .iter()
            .filter(|r| r.get("compliant").and_then(serde_json::Value::as_bool) != Some(true))
        {
            let rules = result
                .pointer("/details/ruleSummaries")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            if rules.is_empty() {
                out.push(error(format!(
                    "veraPDF reports PDF/A-{flavour} non-compliant, no rule detail given"
                )));
            }
            out.extend(rules.iter().map(|rule| error(rule_message(rule, flavour))));
        }
    }
    out
}

fn rule_message(rule: &serde_json::Value, flavour: &str) -> String {
    let field = |key: &str| match rule.get(key).unwrap_or(&serde_json::Value::Null) {
        serde_json::Value::String(s) => s.trim().to_owned(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let or = |s: String| if s.is_empty() { "?".to_owned() } else { s };
    let (clause, test) = (or(field("clause")), or(field("testNumber")));
    let detail = rule
        .get("checks")
        .unwrap_or(&serde_json::Value::Null)
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("errorMessage").and_then(serde_json::Value::as_str).map(str::trim))
        .find(|m| !m.is_empty())
        .unwrap_or_default();
    let suffix = if detail.is_empty() {
        String::new()
    } else {
        format!(" -- {detail}")
    };
    // 6.2.4.3 is "a device colour space needs an output intent"; its message names the symptom, not the fix.
    let lowered = detail.to_lowercase();
    let hint = if clause == "6.2.4.3" && lowered.contains("without") && lowered.contains("output intent") {
        " (hint: the file has no PDF/A output intent for this colour space; declare one with an ICC profile, or avoid \
         device colour operators)"
    } else {
        ""
    };
    format!(
        "PDF/A-{flavour} {clause}-{test}: {}{suffix}{hint}",
        field("description")
    )
}

/// Ghostscript's peak ink on each page of `path` (one `tiffsep` plate per ink, spots at their tint): an error above
/// `max` percent, a warning above `warn`.
pub(crate) fn peak_ink(program: &str, path: &Path, max: f64, warn: Option<f64>, timeout: Duration) -> Verdict {
    let fail = |message: String, timed_out| Verdict::failed(Finding::new(TAC, message), timed_out);
    let dir = std::env::temp_dir().join(format!("proof-pdf-tac-{}-{}", std::process::id(), unique()));
    // A directory that cannot be made shows as plates that cannot be read.
    let _ = std::fs::create_dir_all(&dir);
    let pattern = dir.join("page-%03d.tif");
    let args: Vec<OsString> = [
        "-q",
        "-dNOPAUSE",
        "-dBATCH",
        "-dSAFER",
        "-sDEVICE=tiffsep",
        "-sCompression=none",
        "-r150",
    ]
    .iter()
    .map(OsString::from)
    .chain([format!("-sOutputFile={}", pattern.display()).into(), path.into()])
    .collect();
    let verdict = match run(program, &args, timeout) {
        Err(Failure::Missing) => fail(
            format!("Ghostscript not found ({program}) -- cannot measure ink coverage"),
            false,
        ),
        Err(Failure::TimedOut) => fail(
            format!(
                "Ghostscript could not rasterise the file: timed out after {}",
                seconds(timeout)
            ),
            true,
        ),
        Err(Failure::Broken(why)) => fail(format!("Ghostscript could not rasterise the file: {why}"), false),
        Ok(ran) if !ran.status.success() => {
            let said = ran.stderr.trim();
            fail(
                format!(
                    "Ghostscript could not rasterise the file: {}",
                    if said.is_empty() { "ghostscript failed" } else { said }
                ),
                false,
            )
        }
        Ok(_) => match peaks(&dir) {
            Ok(peaks) => Verdict::from(ink_findings(&peaks, max, warn)),
            Err(why) => fail(format!("Ghostscript could not rasterise the file: {why}"), false),
        },
    };
    let _ = std::fs::remove_dir_all(&dir);
    verdict
}

/// A number no other run in this process shares, for scratch directories.
fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn ink_findings(peaks: &[f64], max: f64, warn: Option<f64>) -> Vec<Finding> {
    let mut out = Vec::new();
    for (page, &peak) in (1..).zip(peaks) {
        let finding = if peak > max {
            Finding::new(TAC, format!("peak ink coverage {peak:.0}% exceeds the {max:.0}% limit"))
        } else if let Some(warn) = warn.filter(|w| peak > *w) {
            let mut finding = Finding::new(
                TAC,
                format!("peak ink coverage {peak:.0}% exceeds the {warn:.0}% advisory level"),
            );
            finding.severity = Severity::Warning;
            finding
        } else {
            continue;
        };
        out.push(finding.at(vec![Location::Page(page)]));
    }
    out
}

/// Each page's peak ink in percent, from the plates `tiffsep` wrote to `dir`: `page-NNN.tif` (a composite, unused) and
/// `page-NNN(Ink).tif` per ink, 0 for full ink.
fn peaks(dir: &Path) -> Result<Vec<f64>, String> {
    let mut plates: Vec<(u32, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let page = name
            .strip_prefix("page-")
            .and_then(|r| r.get(..3))
            .and_then(|n| n.parse().ok());
        match page {
            Some(page) if name.contains('(') => plates.push((page, entry.path())),
            Some(page) => plates.push((page, PathBuf::new())),
            None => {}
        }
    }
    let pages = plates
        .iter()
        .map(|(p, _)| *p)
        .max()
        .ok_or("Ghostscript wrote no page images")?;
    (1..=pages)
        .map(|page| {
            let mut total: Vec<u32> = Vec::new();
            for (_, plate) in plates
                .iter()
                .filter(|(p, path)| *p == page && !path.as_os_str().is_empty())
            {
                let ink = plate_ink(plate)?;
                total.resize(total.len().max(ink.len()), 0);
                for (sum, value) in total.iter_mut().zip(ink) {
                    *sum += u32::from(255 - value);
                }
            }
            Ok(f64::from(total.into_iter().max().unwrap_or(0)) / 255.0 * 100.0)
        })
        .collect()
}

/// A plate's 8-bit samples.
fn plate_ink(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = tiff::decoder::Decoder::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    match decoder.read_image().map_err(|e| e.to_string())? {
        tiff::decoder::DecodingResult::U8(samples) => Ok(samples),
        _ => Err(format!("{} is not an 8-bit plate", path.display())),
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    /// A fresh directory for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proof-pdf-tools-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An executable shell script at `dir/name`.
    fn script(dir: &Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    fn messages(verdict: &Verdict) -> Vec<String> {
        verdict.findings.iter().map(|f| f.message.clone()).collect()
    }

    #[test]
    fn verapdf_outcomes() {
        let dir = scratch("verapdf");
        let pdf = Path::new("x.pdf");
        let said = |body: &str| {
            let program = script(&dir, "verapdf", body);
            verapdf(&program, "2b", pdf, SECOND)
        };
        let missing = verapdf("/nonexistent/verapdf", "2b", pdf, SECOND);
        assert_eq!(
            messages(&missing),
            ["veraPDF not found (/nonexistent/verapdf) -- cannot confirm PDF/A-2b"]
        );
        let broken = verapdf(dir.to_str().unwrap(), "2b", pdf, SECOND);
        assert!(messages(&broken)[0].starts_with("veraPDF could not validate the file: "));
        let noisy = said(
            "echo 'WARNING: Final field x' >&2\necho 'Nov 26, 2026 9:00:00 AM org.verapdf.Main run' >&2\n\
             echo 'WARNING: x.pdf does not appear to be a valid PDF.' >&2\nexit 7",
        );
        assert_eq!(
            messages(&noisy),
            ["veraPDF could not validate the file: x.pdf does not appear to be a valid PDF."]
        );
        assert!(messages(&said("exit 3"))[0].contains("veraPDF exited"));
        assert!(messages(&said("echo nonsense"))[0].contains("unreadable JSON"));
        let slow = said("sleep 5");
        assert!(slow.timed_out);
        let slow = verapdf(
            &script(&dir, "verapdf", "sleep 5"),
            "2b",
            pdf,
            Duration::from_millis(50),
        );
        assert!(slow.timed_out && messages(&slow)[0].ends_with("timed out after 0s"));
        let report = |json: &str| said(&format!("cat <<'EOF'\n{json}\nEOF\nexit 1"));
        assert_eq!(messages(&report("{}")), ["veraPDF returned no validation result"]);
        assert_eq!(
            messages(&report(r#"{"report": {"jobs": [{}]}}"#)),
            ["veraPDF could not validate the file"]
        );
        let fine = report(r#"{"report": {"jobs": [{"validationResult": [{"compliant": true}]}]}}"#);
        assert!(fine.findings.is_empty() && fine.outcome == Outcome::Pass);
        let bare = report(r#"{"report": {"jobs": [{"validationResult": [{"compliant": false}]}]}}"#);
        assert_eq!(
            messages(&bare),
            ["veraPDF reports PDF/A-2b non-compliant, no rule detail given"]
        );
        let rules = report(
            r#"{"report": {"jobs": [{"validationResult": [{"compliant": false, "details": {"ruleSummaries": [
                {"clause": "6.2.4.3", "testNumber": 4, "description": " DeviceRGB ",
                 "checks": [{"errorMessage": " "}, {"errorMessage": "DeviceRGB used without output intent profile"}]},
                {"description": "odd"}]}}]}]}}"#,
        );
        assert_eq!(
            messages(&rules),
            [
                "PDF/A-2b 6.2.4.3-4: DeviceRGB -- DeviceRGB used without output intent profile (hint: the file has no \
                 PDF/A output intent for this colour space; declare one with an ICC profile, or avoid device colour \
                 operators)",
                "PDF/A-2b ?-?: odd",
            ]
        );
        assert_eq!(rules.outcome, Outcome::Fail);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An 8-bit plate `width` samples wide, 1 high.
    fn plate(path: &Path, samples: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let mut tiff = tiff::encoder::TiffEncoder::new(file).unwrap();
        let width = u32::try_from(samples.len()).unwrap();
        tiff.write_image::<tiff::encoder::colortype::Gray8>(width, 1, samples)
            .unwrap();
    }

    #[test]
    fn ghostscript_outcomes() {
        let dir = scratch("gs");
        let plates = dir.join("plates");
        std::fs::create_dir_all(&plates).unwrap();
        // Page 1 peaks at 200% + 100% = 300%; page 2 at 150%; page 3 at 50%.
        plate(&plates.join("page-001(Cyan).tif"), &[0, 255]);
        plate(&plates.join("page-001(Black).tif"), &[0, 128]);
        plate(&plates.join("page-001(Magenta).tif"), &[0, 255]);
        plate(&plates.join("page-002(Cyan).tif"), &[0, 128]);
        plate(&plates.join("page-002(Black).tif"), &[128, 128]);
        plate(&plates.join("page-003(Black).tif"), &[128]);
        std::fs::write(plates.join("page-001.tif"), b"a composite, never read").unwrap();
        std::fs::write(plates.join("notes.txt"), b"").unwrap();
        let copy = format!(
            "for a in \"$@\"; do case \"$a\" in -sOutputFile=*) out=\"${{a#-sOutputFile=}}\";; esac; done\n\
             cp {}/* \"$(dirname \"$out\")\"/",
            plates.display()
        );
        let gs = script(&dir, "gs", &copy);
        let pdf = Path::new("x.pdf");
        let found = peak_ink(&gs, pdf, 280.0, Some(140.0), SECOND);
        assert_eq!(
            messages(&found),
            [
                "peak ink coverage 300% exceeds the 280% limit",
                "peak ink coverage 150% exceeds the 140% advisory level"
            ]
        );
        assert_eq!(
            found.findings.iter().map(|f| f.page).collect::<Vec<_>>(),
            [Some(1), Some(2)]
        );
        assert_eq!(peak_ink(&gs, pdf, 400.0, None, SECOND).outcome, Outcome::Pass);
        let said = |body: &str| messages(&peak_ink(&script(&dir, "gs-other", body), pdf, 300.0, None, SECOND));
        assert_eq!(
            said("exit 0"),
            ["Ghostscript could not rasterise the file: Ghostscript wrote no page images"]
        );
        assert_eq!(
            said("echo 'Error: /undefined' >&2\nexit 1"),
            ["Ghostscript could not rasterise the file: Error: /undefined"]
        );
        assert_eq!(
            said("exit 1"),
            ["Ghostscript could not rasterise the file: ghostscript failed"]
        );
        let bad = "for a in \"$@\"; do case \"$a\" in -sOutputFile=*) out=\"${a#-sOutputFile=}\";; esac; done\n\
                   echo junk > \"$(dirname \"$out\")/page-001(Black).tif\"";
        assert!(said(bad)[0].starts_with("Ghostscript could not rasterise the file: "));
        let wide = dir.join("wide.tif");
        let file = std::fs::File::create(&wide).unwrap();
        tiff::encoder::TiffEncoder::new(file)
            .unwrap()
            .write_image::<tiff::encoder::colortype::Gray16>(1, 1, &[7])
            .unwrap();
        let sixteen = format!(
            "for a in \"$@\"; do case \"$a\" in -sOutputFile=*) out=\"${{a#-sOutputFile=}}\";; esac; done\n\
             cp {} \"$(dirname \"$out\")/page-001(Black).tif\"",
            wide.display()
        );
        assert!(said(&sixteen)[0].ends_with("is not an 8-bit plate"));
        // The scratch directory gone, a plate that cannot be opened, a plate whose data is cut short.
        let gone = "for a in \"$@\"; do case \"$a\" in -sOutputFile=*) out=\"${a#-sOutputFile=}\";; esac; done\n\
                    rm -r \"$(dirname \"$out\")\"";
        assert!(said(gone)[0].starts_with("Ghostscript could not rasterise the file: "));
        let dangling = "for a in \"$@\"; do case \"$a\" in -sOutputFile=*) out=\"${a#-sOutputFile=}\";; esac; done\n\
                        ln -s /nonexistent \"$(dirname \"$out\")/page-001(Black).tif\"";
        assert!(said(dangling)[0].starts_with("Ghostscript could not rasterise the file: "));
        // A plate whose strip offset (tag 273) points past the end of the file: its header reads, its data does not.
        let short = dir.join("short.tif");
        plate(&short, &[0; 16]);
        let mut bytes = std::fs::read(&short).unwrap();
        let ifd = usize::try_from(u32::from_le_bytes(bytes[4..8].try_into().unwrap())).unwrap();
        let entries = usize::from(u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()));
        for entry in (0..entries).map(|i| ifd + 2 + 12 * i) {
            if u16::from_le_bytes(bytes[entry..entry + 2].try_into().unwrap()) == 273 {
                bytes[entry + 8..entry + 12].copy_from_slice(&0x7FFF_0000_u32.to_le_bytes());
            }
        }
        std::fs::write(&short, bytes).unwrap();
        let truncated = format!(
            "for a in \"$@\"; do case \"$a\" in -sOutputFile=*) out=\"${{a#-sOutputFile=}}\";; esac; done\n\
             cp {} \"$(dirname \"$out\")/page-001(Black).tif\"",
            short.display()
        );
        assert!(said(&truncated)[0].starts_with("Ghostscript could not rasterise the file: "));
        assert_eq!(
            messages(&peak_ink("/nonexistent/gs", pdf, 300.0, None, SECOND)),
            ["Ghostscript not found (/nonexistent/gs) -- cannot measure ink coverage"]
        );
        assert!(
            peak_ink(dir.to_str().unwrap(), pdf, 300.0, None, SECOND).findings[0]
                .message
                .contains("rasterise")
        );
        let slow = peak_ink(
            &script(&dir, "gs-slow", "sleep 5"),
            pdf,
            300.0,
            None,
            Duration::from_millis(50),
        );
        assert!(slow.timed_out);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_timeout_exits_3() {
        let dir = scratch("exit");
        let program = script(&dir, "verapdf", "sleep 5");
        let pdf = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pdfs/x4-good.pdf");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let args = [
            "proof-pdf",
            "verify",
            pdf,
            "--pdfa",
            "--verapdf",
            &program,
            "--timeout",
            "0.1",
            "--quiet",
        ];
        assert_eq!(crate::cli::main(args.map(OsString::from), &mut out, &mut err, false), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
