//! Command-line entry point for the WinPasswordLock library.

/// Reports command errors to stderr and exits with a nonzero status.
fn main() {
    if let Err(error) = win_password_lock::run_cli() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
