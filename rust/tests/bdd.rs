//! Runs the Gherkin scenarios in `tests/features`.

use std::cell::RefCell;
use std::rc::Rc;

use cucumber::{World, given, then, when};
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

#[derive(Debug, Default, World)]
struct Pipeline {
    observer: String,
    reader: String,
    log: Log,
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

fn main() {
    futures::executor::block_on(Pipeline::cucumber().fail_on_skipped().run_and_exit("tests/features"));
}
