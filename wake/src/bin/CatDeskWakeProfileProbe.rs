use catdesk_wake::store::Store;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "PROBE_LAUNCH_FAILED")?;
    let job = match catdesk_wake::process_job::Job::assign(&child) {
        Ok(job) => job,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("PROBE_CONTAINMENT_FAILED");
        }
    };
    // The script waits for this handshake before importing Selenium or opening Chrome.
    let result = (|| {
        child
            .stdin
            .take()
            .ok_or("PROBE_PIPE_FAILED")?
            .write_all(b"START\n")
            .map_err(|_| "PROBE_PIPE_FAILED")?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait().map_err(|_| "PROBE_WAIT_FAILED")? {
                return if status.success() {
                    Ok(())
                } else {
                    Err("PROBE_FAILED")
                };
            }
            if Instant::now() >= deadline {
                return Err("PROBE_PROCESS_TIMEOUT");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })();
    // Close only this probe's job, including any child retaining its output pipe.
    drop(job);
    let _ = child.kill();
    let _ = child.wait();
    result?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or("PROBE_PIPE_FAILED")?
        .take(16_385)
        .read_to_end(&mut bytes)
        .map_err(|_| "PROBE_OUTPUT_FAILED")?;
    if bytes.len() > 16_384 {
        return Err("PROBE_OUTPUT_TOO_LARGE");
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "PROBE_OUTPUT_INVALID")?;
    Ok(text
        .lines()
        .filter(|line| {
            let Some(state) = line.strip_prefix("PROBE:") else {
                return false;
            };
            let state = state.strip_prefix("FINAL:").unwrap_or(state);
            matches!(
                state,
                "EXACT_READY_IDLE"
                    | "EXACT_READY_BUSY"
                    | "EXACT_NO_EDITOR"
                    | "LOGIN_VISIBLE"
                    | "CAPTCHA_VISIBLE"
                    | "HOME"
                    | "TARGET_DRIFT"
                    | "CHROME_ERROR"
                    | "URL_UNAVAILABLE"
                    | "URL_INVALID"
            )
        })
        .take(64)
        .map(str::to_owned)
        .collect())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn fixture() {
        let Ok(mode) = std::env::var("CATDESK_PROBE_TEST_MODE") else {
            return;
        };
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        assert_eq!(line, "START\n");
        if mode == "hang" {
            std::thread::sleep(Duration::from_secs(30));
        }
        println!("PROBE:arbitrary-private-content");
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
            ["PROBE:EXACT_READY_IDLE", "PROBE:FINAL:EXACT_READY_IDLE"]
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
}
