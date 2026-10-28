use catdesk_wake::store::Store;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        println!("{}", serde_json::json!({"probe":"ERROR","code":error}));
        std::process::exit(1);
    }
}

fn open_probe_store(root: &Path) -> Result<Store, &'static str> {
    #[cfg(feature = "test-support")]
    if let Some(parent) = std::env::var_os("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT") {
        return Store::open_scoped_for_test(root, Path::new(&parent))
            .map_err(|_| "STORE_UNAVAILABLE");
    }
    Store::open(root).map_err(|_| "STORE_UNAVAILABLE")
}

fn run() -> Result<(), &'static str> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA_UNAVAILABLE")?;
    let wake_root = PathBuf::from(&local).join("CatDeskWake");
    let python = wake_root.join("runtime").join("python.exe");
    let profile = wake_root.join("browser-profile");
    if !python.is_file() {
        return Err("RUNTIME_UNAVAILABLE");
    }
    if !profile.is_dir() {
        return Err("PROFILE_UNAVAILABLE");
    }

    let store = open_probe_store(&wake_root)?;
    let control: catdesk_wake::runtime::Control =
        catdesk_wake::store::read(&wake_root.join("control.json"))
            .map_err(|_| "CONTROL_UNAVAILABLE")?;
    if control.schema_version != 1 || control.desired != "PAUSED" {
        return Err("PROBE_REQUIRES_PAUSED_WAKE");
    }
    let status: catdesk_wake::runtime::Status =
        catdesk_wake::store::read(&wake_root.join("status.json"))
            .map_err(|_| "HOST_STATUS_UNAVAILABLE")?;
    // Desired PAUSED alone is insufficient while an in-flight attempt drains.
    if status.host != "PAUSED" {
        return Err("PROBE_REQUIRES_PAUSE_ACKNOWLEDGEMENT");
    }
    let config = store.config().map_err(|_| "CONFIG_UNAVAILABLE")?;
    let target = config.targets.get("catdesk").ok_or("TARGET_UNAVAILABLE")?;

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("REPO_ROOT_UNAVAILABLE")?;
    let probe = repo_root
        .join("scripts")
        .join("wake_readiness_visual_probe.py");
    if !probe.is_file() {
        return Err("PROBE_UNAVAILABLE");
    }

    let mut command = Command::new(python);
    command
        .arg("-I")
        .arg(probe)
        .arg("--await-start")
        .arg("--conversation-url")
        .arg(&target.url)
        .arg("--seconds")
        .arg("20");
    for line in run_bounded(&mut command, Duration::from_secs(60))? {
        println!("{line}");
    }
    Ok(())
}

fn run_bounded(command: &mut Command, timeout: Duration) -> Result<Vec<String>, &'static str> {
    catdesk_wake::readiness_probe::run_bounded(command, timeout)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    // This fixture intentionally exits before its child. The enclosing
    // run_bounded owner terminates its retained Job and proves all members exit.
    #[allow(clippy::zombie_processes)]
    fn fixture() {
        let Ok(mode) = std::env::var("CATDESK_PROBE_TEST_MODE") else {
            return;
        };
        if mode == "descendant" {
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        assert_eq!(line, "START\n");
        if mode == "hang" {
            std::thread::sleep(Duration::from_secs(30));
        }
        if mode == "parent" {
            let _child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "tests::fixture", "--nocapture"])
                .env("CATDESK_PROBE_TEST_MODE", "descendant")
                .stdin(std::process::Stdio::null())
                .spawn()
                .unwrap();
        }
        println!("PROBE:arbitrary-private-content");
        println!("PROBE:TARGET_TAB_EXACT private-content-must-be-filtered");
        for state in ["EXACT", "MISSING", "AMBIGUOUS", "DRIFT", "UNAVAILABLE"] {
            println!("PROBE:TARGET_TAB_{state}");
        }
        println!("PROBE:EXACT_READY_IDLE");
        println!("PROBE:FINAL:EXACT_READY_IDLE");
    }

    fn fixture_command(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "tests::fixture", "--nocapture"])
            .env("CATDESK_PROBE_TEST_MODE", mode);
        command
    }

    #[test]
    fn probe_reports_only_fixed_states() {
        let rows = run_bounded(&mut fixture_command("success"), Duration::from_secs(5)).unwrap();
        assert_eq!(
            rows,
            [
                "PROBE:TARGET_TAB_EXACT",
                "PROBE:TARGET_TAB_MISSING",
                "PROBE:TARGET_TAB_AMBIGUOUS",
                "PROBE:TARGET_TAB_DRIFT",
                "PROBE:TARGET_TAB_UNAVAILABLE",
                "PROBE:EXACT_READY_IDLE",
                "PROBE:FINAL:EXACT_READY_IDLE"
            ]
        );
    }

    #[test]
    fn probe_timeout_terminates_the_owned_process() {
        let start = Instant::now();
        assert_eq!(
            run_bounded(&mut fixture_command("hang"), Duration::from_millis(150)).unwrap_err(),
            "PROBE_PROCESS_TIMEOUT"
        );
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn probe_closes_owned_descendant_and_its_inherited_output_pipe() {
        let start = Instant::now();
        let rows = run_bounded(&mut fixture_command("parent"), Duration::from_secs(5)).unwrap();
        assert!(rows.contains(&"PROBE:FINAL:EXACT_READY_IDLE".to_string()));
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
