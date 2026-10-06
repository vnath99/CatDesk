use catdesk_wake::{process_job::Job, runtime, store::Store};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let live = runtime::status(&store)?;
    if live.host != "PAUSED" {
        return Err("WAKE_MUST_BE_PAUSED".into());
    }
    if live.queue_depth != 0 {
        return Err("WAKE_QUEUE_NOT_EMPTY".into());
    }

    let config = store.config()?;
    let target = config
        .targets
        .get("catdesk")
        .ok_or("TARGET_NOT_CONFIGURED")?;
    let install = runtime::reviewed_install_status(&store)?;
    let current = install
        .current_directory
        .ok_or("CURRENT_INSTALL_UNAVAILABLE")?;
    let bridge = root.join("versions").join(current).join("wake_bridge.py");
    if !bridge.is_file() {
        return Err("INSTALLED_BRIDGE_UNAVAILABLE".into());
    }
    let python = root.join("runtime").join("python.exe");
    let profile = root.join("browser-profile");
    if !python.is_file() || !profile.is_dir() {
        return Err("WAKE_RUNTIME_UNAVAILABLE".into());
    }

    let script = r#"
import importlib.util
import json
import sys
import time
from pathlib import Path
from seleniumbase import SB

bridge_path, profile, url = sys.argv[1], sys.argv[2], sys.argv[3]
spec = importlib.util.spec_from_file_location("catdesk_installed_wake_bridge", bridge_path)
if spec is None or spec.loader is None:
    print(json.dumps({"state": "BRIDGE_LOAD_FAILED"}, sort_keys=True), flush=True)
    raise SystemExit(2)
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)

sink = bridge.CdpSink(url, Path(profile), 30, 20)
observed = []
allowed_routes = {
    "CHROME_ERROR", "SAME_CONVERSATION", "HOME", "OTHER_HOST",
    "OTHER_ROUTE", "INVALID_ROUTE", "PROJECT_OTHER",
}
allowed_reasons = {
    "BROWSER_NETWORK_ERROR", "DOCUMENT_LOADING", "EDITOR_SELECTOR",
    "READY", "NETWORK_ERR_NAME_NOT_RESOLVED", "NETWORK_UNKNOWN",
}

def observer(route, reason):
    if route in allowed_routes and reason in allowed_reasons:
        observed.append((route, reason))

try:
    with SB(uc=True, user_data_dir=profile) as sb:
        sb.activate_cdp_mode(url)
        cdp = sb.cdp

        # Reserved .invalid is guaranteed not to resolve. This creates a
        # browser-network error document without changing machine networking.
        try:
            cdp.get("https://catdesk-wake-network-probe.invalid/")
        except Exception:
            pass

        saw_network = False
        deadline = time.monotonic() + 15.0
        while time.monotonic() < deadline:
            try:
                current = str(cdp.get_current_url())
                reason = sink.readiness_reason(cdp, current)
                if reason == "BROWSER_NETWORK_ERROR":
                    saw_network = True
                    break
            except bridge.Attention as error:
                if str(error) == "BROWSER_NETWORK_ERROR":
                    saw_network = True
                    break
            except Exception:
                pass
            time.sleep(0.25)

        if not saw_network:
            print(json.dumps({"state": "FAULT_NOT_OBSERVED"}, sort_keys=True), flush=True)
            raise SystemExit(3)

        recovered = sink.wait_for_page_readiness(sb, cdp, observer=observer)
        current = str(recovered.get_current_url())
        editor_status, editor_empty = sink.editor_state(recovered)
        exact = sink.exact(current)
        saw_recovery_observation = any(
            reason == "BROWSER_NETWORK_ERROR" for _, reason in observed
        )
        final_ready_observed = any(
            route == "SAME_CONVERSATION" and reason == "READY"
            for route, reason in observed
        )

        if not exact or editor_status != "ready" or not editor_empty:
            print(json.dumps({
                "state": "RECOVERY_NOT_READY",
                "sawNetwork": True,
                "sawRecoveryObservation": saw_recovery_observation,
                "finalReadyObserved": final_ready_observed,
            }, sort_keys=True), flush=True)
            raise SystemExit(4)

        print(json.dumps({
            "state": "RECOVERED",
            "sawNetwork": True,
            "sawRecoveryObservation": saw_recovery_observation,
            "finalReadyObserved": final_ready_observed,
            "exactTarget": True,
            "editorReadyEmpty": True,
            "browserWrite": False,
        }, sort_keys=True), flush=True)
        sink.close_owned_browser(sb)
except SystemExit:
    raise
except Exception:
    print(json.dumps({"state": "PROBE_FAILED"}, sort_keys=True), flush=True)
    raise SystemExit(5)
"#;

    let mut child = Command::new(&python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(&bridge)
        .arg(&profile)
        .arg(&target.url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "NETWORK_PROBE_START_FAILED")?;
    let _job = Job::assign(&child)?;
    let stdout = child.stdout.take().ok_or("STDOUT_FAILED")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout)
            .read_line(&mut line)
            .map(|_| line)
            .map_err(|_| "READ_FAILED".to_string());
        let _ = tx.send(result);
    });

    let result = rx.recv_timeout(Duration::from_secs(120));
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Ok(Ok(line)) if !line.trim().is_empty() => {
            println!("{}", line.trim());
            if line.contains(r#""state": "RECOVERED""#) {
                Ok(())
            } else {
                Err("NETWORK_RECOVERY_NOT_PROVEN".into())
            }
        }
        Ok(Ok(_)) => Err("NETWORK_PROBE_EMPTY".into()),
        Ok(Err(error)) => Err(error),
        Err(_) => Err("NETWORK_PROBE_TIMEOUT".into()),
    }
}
