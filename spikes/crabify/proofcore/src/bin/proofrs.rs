fn main() {
    let mut failed = false;
    for path in std::env::args().skip(1) {
        match proofcore::check_pdfx(&path) {
            Ok(found) => {
                failed |= found.iter().any(|f| f.severity == "error");
                println!("{path}: {} finding(s)", found.len());
            }
            Err(e) => {
                failed = true;
                println!("{path}: cannot open: {e}");
            }
        }
    }
    std::process::exit(failed as i32);
}
