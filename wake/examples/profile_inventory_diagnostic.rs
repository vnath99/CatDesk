use catdesk_wake::runtime;
use std::path::Path;

fn summarize(label: &str, path: &Path) {
    let local_state = path.join("Local State").is_file();
    let default = path.join("Default");
    let cookie =
        default.join("Cookies").is_file() || default.join("Network").join("Cookies").is_file();
    let prefs = default.join("Preferences").is_file();
    let lock = default.join("LOCK").exists() || path.join("SingletonLock").exists();
    println!(
        "{label}: exists={} local_state={} default={} cookies={} preferences={} lock_marker={}",
        path.is_dir(),
        local_state,
        default.is_dir(),
        cookie,
        prefs,
        lock
    );
}

fn main() -> Result<(), String> {
    let repo = std::env::current_dir().map_err(|_| "CWD_FAILED")?;
    let legacy = repo
        .join(".catdesk")
        .join("wake-bridge")
        .join("browser-profile");
    let current = runtime::default_root()?.join("browser-profile");
    summarize("legacy", &legacy);
    summarize("independent", &current);
    Ok(())
}
