//! Runs the Gherkin scenarios in `tests/features`.

use std::cell::RefCell;
use std::rc::Rc;

use cucumber::{World, given, then, when};
use proof_pdf::checks::{Files, Settings, verify_settings};
use proof_pdf::findings::{Level, Outcome, Report, Verdict};
use proof_pdf::pipeline::{Listener, publish};

/// Who heard what, in the order they heard it, shared by every listener in a scenario.
type Log = Rc<RefCell<Vec<(String, u32)>>>;

struct Named {
    name: String,
    log: Log,
}

impl Listener<u32> for Named {
    fn on_event(&mut self, event: &u32) {
        self.log.borrow_mut().push((self.name.clone(), *event));
    }
}

/// One scenario's state: the pipeline steps use the listeners and log, the PDF/X steps the fixture and its report.
#[derive(Debug, Default, World)]
struct Pipeline {
    observer: String,
    reader: String,
    log: Log,
    fixture: (String, Vec<u8>),
    folder: std::path::PathBuf,
    report: Option<Report>,
}

impl Pipeline {
    fn publish(&mut self, count: u32) {
        let mut observer = Named {
            name: self.observer.clone(),
            log: Rc::clone(&self.log),
        };
        let mut reader = Named {
            name: self.reader.clone(),
            log: Rc::clone(&self.log),
        };
        for event in 0..count {
            publish(&event, &mut [&mut observer], &mut [&mut reader]);
        }
    }

    fn position(&self, name: &str) -> usize {
        self.log
            .borrow()
            .iter()
            .position(|(heard, _)| heard == name)
            .unwrap_or(usize::MAX)
    }
}

#[given(expr = "an observer {string} and a reader {string}")]
fn listeners(world: &mut Pipeline, observer: String, reader: String) {
    world.observer = observer;
    world.reader = reader;
}

#[when("an event is published")]
fn one_event(world: &mut Pipeline) {
    world.publish(1);
}

#[when(expr = "{int} events are published")]
fn events(world: &mut Pipeline, count: u32) {
    world.publish(count);
}

#[then(expr = "{string} hears it before {string}")]
fn before(world: &mut Pipeline, first: String, second: String) {
    assert!(world.position(&first) < world.position(&second));
}

#[then(expr = "{string} heard {int} events")]
fn heard(world: &mut Pipeline, name: String, count: usize) {
    assert_eq!(
        world.log.borrow().iter().filter(|(heard, _)| *heard == name).count(),
        count
    );
}

/// A fixture by name: built ones are in `tests/fixtures/pdfs`, PDF/X-only ones in `tests/fixtures/pdfx`.
#[given(expr = "the fixture {string}")]
fn fixture(world: &mut Pipeline, name: String) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures");
    let file = format!("{name}.pdf");
    let path = [root.join("pdfs").join(&file), root.join("pdfx").join(&file)]
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("no fixture {file}"));
    world.fixture = (file, std::fs::read(&path).unwrap());
    world.folder = path.parent().unwrap().to_owned();
}

impl Pipeline {
    /// Verify the fixture at `level`, with the files beside it unless `alone`.
    fn verify(&mut self, level: Level, params: &str, alone: bool) {
        let settings = Settings {
            level,
            params: serde_json::from_str(params).expect("parameters as JSON"),
            files: if alone {
                Files::Alone
            } else {
                Files::Set(self.folder.clone())
            },
        };
        self.report = Some(verify_settings(&self.fixture.0, &self.fixture.1, &settings));
    }
}

#[when(regex = r"^it is verified as PDF/X-(4|4p|5g|5pg|5n)$")]
fn verified(world: &mut Pipeline, level: String) {
    world.verify(Level::from_name(&format!("x{level}")).unwrap(), "{}", false);
}

/// Without the files it refers to (ISO 15930-7 A.1).
#[when(regex = r"^it is verified alone as PDF/X-(4|4p|5g|5pg|5n)$")]
fn verified_alone(world: &mut Pipeline, level: String) {
    world.verify(Level::from_name(&format!("x{level}")).unwrap(), "{}", true);
}

#[when("it is verified for print")]
fn verified_for_print(world: &mut Pipeline) {
    world.verify(Level::Print, "{}", false);
}

/// For print with the profile's parameters, given as JSON.
#[when(expr = "it is verified for print with {string}")]
fn verified_for_print_with(world: &mut Pipeline, params: String) {
    world.verify(Level::Print, &params, false);
}

fn report(world: &Pipeline) -> &Report {
    world.report.as_ref().expect("a When step verifies the fixture first")
}

#[then("it passes")]
fn passes(world: &mut Pipeline) {
    let report = report(world);
    assert_eq!(report.verdict, Verdict::Pass, "{:#?}", report.findings);
}

#[then(expr = "check {string} is not evaluated")]
fn not_evaluated(world: &mut Pipeline, id: String) {
    let report = report(world);
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.id.id == id && matches!(c.outcome, Outcome::NotEvaluated(_))),
        "{id} was evaluated: {:#?}",
        report.checks
    );
}

#[then(expr = "check {string} fails on page {int}")]
fn fails_on_page(world: &mut Pipeline, id: String, page: u32) {
    let report = report(world);
    assert!(
        report.findings.iter().any(|f| f.check.id == id && f.page == Some(page)),
        "{id} did not fail on page {page}: {:#?}",
        report.findings
    );
}

#[then(expr = "check {string} fails")]
fn fails(world: &mut Pipeline, id: String) {
    let report = report(world);
    assert!(
        report.findings.iter().any(|f| f.check.id == id),
        "{id} did not fail: {:#?}",
        report.findings
    );
}

#[then(expr = "no check fails but {string} and {string}")]
fn only_two(world: &mut Pipeline, id: String, other: String) {
    let others: Vec<_> = report(world)
        .findings
        .iter()
        .filter(|f| f.check.id != id && f.check.id != other)
        .collect();
    assert!(others.is_empty(), "other checks failed: {others:#?}");
}

#[then("no check fails")]
fn none_fails(world: &mut Pipeline) {
    let findings = &report(world).findings;
    assert!(findings.is_empty(), "checks failed: {findings:#?}");
}

#[then(expr = "no check fails but {string}")]
fn only(world: &mut Pipeline, id: String) {
    let others: Vec<_> = report(world).findings.iter().filter(|f| f.check.id != id).collect();
    assert!(others.is_empty(), "other checks failed: {others:#?}");
}

fn main() {
    futures::executor::block_on(Pipeline::cucumber().fail_on_skipped().run_and_exit("tests/features"));
}
