//! Fixed SCM entry point. No install, bootstrap, or console mutation mode.

#[cfg(windows)]
fn main() {
    if let Err(error) = catdesk_wake::install_broker_service_runtime::run_fixed_service_binary() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("WAKE_BROKER_WINDOWS_REQUIRED");
    std::process::exit(1);
}
