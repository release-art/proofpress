//! The `proof-pdf` command line; see `proof_pdf::cli`.

fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(proof_pdf::cli::run(std::env::args_os()))
}
