//! The pipeline's public contract, checked with mocks. Mocks live here, under `tests/`, not in `src/`: their generated
//! code would otherwise count against the coverage gate.

use mockall::{Sequence, mock};
use proof_pdf::pipeline::{Listener, publish};

mock! {
    Heard {}
    impl Listener<u32> for Heard {
        fn on_event(&mut self, event: &u32);
    }
}

#[test]
fn observers_hear_an_event_before_readers() {
    let mut order = Sequence::new();
    let (mut first, mut second, mut reader) = (MockHeard::new(), MockHeard::new(), MockHeard::new());
    for listener in [&mut first, &mut second, &mut reader] {
        listener
            .expect_on_event()
            .withf(|e| *e == 42)
            .times(1)
            .in_sequence(&mut order)
            .return_const(());
    }
    publish(&42, &mut [&mut first, &mut second], &mut [&mut reader]);
}
