//! The `proof-pdf` binary itself: arguments in, exit code out.
#![cfg(feature = "cli")]

#[test]
fn the_binary_runs() {
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_proof-pdf"))
            .args(args)
            .output()
            .unwrap()
            .status
            .code()
    };
    assert_eq!(run(&["--version"]), Some(0));
    assert_eq!(run(&["verify", "--bogus"]), Some(64));
}
