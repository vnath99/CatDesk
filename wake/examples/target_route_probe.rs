use catdesk_wake::{process_job::Job, runtime, store::Store};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), String> {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "route".to_string());
    if mode != "route" && mode != "network-recovery" {
        return Err("PROFILE_PROBE_MODE_INVALID".into());
    }

    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let live = runtime::status(&store)?;
    if mode == "network-recovery" {
        if live.host != "PAUSED" {
            return Err("WAKE_MUST_BE_PAUSED".into());
        }
        if live.queue_depth != 0 {
            return Err("WAKE_QUEUE_NOT_EMPTY".into());
        }
    }

    let config = store.config()?;
    let target = config
        .targets
        .get("catdesk")
        .ok_or("TARGET_NOT_CONFIGURED")?;
    let python = root.join("runtime").join("python.exe");
    let profile = root.join("browser-profile");
    let install = runtime::reviewed_install_status(&store)?;
    let current = install
        .current_directory
        .ok_or("CURRENT_INSTALL_UNAVAILABLE")?;
    let bridge = root.join("versions").join(current).join("wake_bridge.py");
    if !python.is_file() || !profile.is_dir() || !bridge.is_file() {
        return Err("PROFILE_PROBE_RUNTIME_UNAVAILABLE".into());
    }

    let script = r#"
import importlib.util
import sys
import time
from pathlib import Path
from urllib.parse import urlparse
from seleniumbase import SB

profile, url, mode, bridge_path = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
expected = urlparse(url)
expected_parts = expected.path.split('/')[1:]
expected_id = expected_parts[-1] if expected_parts else ''

def classify(value):
    try:
        p = urlparse(value)
        if p.scheme == 'chrome-error':
            return 'chrome_error'
        if p.hostname not in {'chatgpt.com', 'chat.openai.com'}:
            return 'other_host'
        parts = p.path.split('/')[1:]
        if p.path.startswith('/auth/'):
            return 'auth'
        if p.path in {'', '/'}:
            return 'home'
        if len(parts) == 2 and parts[0] == 'c':
            return 'same_c' if parts[1] == expected_id else 'different_c'
        if len(parts) == 4 and parts[0] == 'g' and parts[2] == 'c':
            return 'same_g_c' if parts[3] == expected_id else 'different_g_c'
        if len(parts) >= 2 and parts[0] == 'g':
            return 'project_other'
        return 'other_route'
    except Exception:
        return 'invalid'

def load_bridge(path):
    spec = importlib.util.spec_from_file_location("catdesk_installed_wake_bridge", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("bridge-load")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

try:
    with SB(uc=True, user_data_dir=profile) as sb:
        sb.activate_cdp_mode(url)

        if mode == 'route':
            deadline = time.time() + 60
            route = 'unknown'
            editor = False
            network = False
            while time.time() < deadline:
                try:
                    current = str(sb.cdp.get_current_url())
                    route = classify(current)
                    network = route == 'chrome_error'
                    try:
                        editor = bool(sb.cdp.is_element_visible('#prompt-textarea'))
                    except Exception:
                        editor = False
                    if editor or route in {'auth','home','different_c','different_g_c','project_other','other_route','other_host','chrome_error'}:
                        break
                except Exception:
                    route = 'driver_unready'
                time.sleep(.25)
            print('ROUTE=' + route + ';EDITOR=' + ('yes' if editor else 'no') + ';NETWORK=' + ('yes' if network else 'no'), flush=True)
        else:
            bridge = load_bridge(bridge_path)
            sink = bridge.CdpSink(url, Path(profile), 30, 20)
            cdp = sb.cdp

            # RFC 2606 reserves .invalid. Navigating there produces a browser
            # network error without changing host networking or proxy state.
            try:
                cdp.get('https://catdesk-wake-network-probe.invalid/')
            except Exception:
                pass

            saw_network = False
            fault_deadline = time.monotonic() + 15.0
            while time.monotonic() < fault_deadline:
                try:
                    current = str(cdp.get_current_url())
                    if sink.readiness_reason(cdp, current) == 'BROWSER_NETWORK_ERROR':
                        saw_network = True
                        break
                except bridge.Attention as error:
                    if str(error) == 'BROWSER_NETWORK_ERROR':
                        saw_network = True
                        break
                except Exception:
                    pass
                time.sleep(.25)

            if not saw_network:
                print('NETWORK_RECOVERY=fail;FAULT=not_observed;WRITE=no', flush=True)
                raise SystemExit(3)

            observations = []
            allowed_routes = {
                'CHROME_ERROR', 'SAME_CONVERSATION', 'HOME', 'OTHER_HOST',
                'OTHER_ROUTE', 'INVALID_ROUTE', 'PROJECT_OTHER',
            }
            allowed_reasons = {
                'BROWSER_NETWORK_ERROR', 'DOCUMENT_LOADING', 'EDITOR_SELECTOR',
                'READY', 'NETWORK_ERR_NAME_NOT_RESOLVED', 'NETWORK_UNKNOWN',
            }

            def observer(route, reason):
                if route in allowed_routes and reason in allowed_reasons:
                    observations.append((route, reason))

            recovered = sink.wait_for_page_readiness(sb, cdp, observer=observer)
            current = str(recovered.get_current_url())
            editor_status, editor_empty = sink.editor_state(recovered)
            exact = sink.exact(current)
            observed_network = any(
                reason == 'BROWSER_NETWORK_ERROR' for _, reason in observations
            )
            observed_ready = any(
                route == 'SAME_CONVERSATION' and reason == 'READY'
                for route, reason in observations
            )
            if not exact or editor_status != 'ready' or not editor_empty:
                print(
                    'NETWORK_RECOVERY=fail;FAULT=browser_network_error;'
                    'EXACT=' + ('yes' if exact else 'no') + ';'
                    'EDITOR=' + ('yes' if editor_status == 'ready' and editor_empty else 'no') + ';'
                    'OBSERVED_NETWORK=' + ('yes' if observed_network else 'no') + ';'
                    'OBSERVED_READY=' + ('yes' if observed_ready else 'no') + ';WRITE=no',
                    flush=True,
                )
                raise SystemExit(4)

            print(
                'NETWORK_RECOVERY=pass;FAULT=browser_network_error;EXACT=yes;EDITOR=yes;'
                'OBSERVED_NETWORK=' + ('yes' if observed_network else 'no') + ';'
                'OBSERVED_READY=' + ('yes' if observed_ready else 'no') + ';WRITE=no',
                flush=True,
            )
            sink.close_owned_browser(sb)
except SystemExit:
    raise
except Exception as error:
    print(
        ('NETWORK_RECOVERY=fail;FAULT=probe_exception;WRITE=no'
         if mode == 'network-recovery'
         else 'ROUTE=exception_' + type(error).__name__ + ';EDITOR=no;NETWORK=no'),
        flush=True,
    )
    raise SystemExit(5 if mode == 'network-recovery' else 0)
"#;

    let mut child = Command::new(&python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(&profile)
        .arg(&target.url)
        .arg(&mode)
        .arg(&bridge)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "PROFILE_PROBE_START_FAILED")?;
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
            if mode == "network-recovery" && !line.starts_with("NETWORK_RECOVERY=pass;") {
                Err("NETWORK_RECOVERY_NOT_PROVEN".into())
            } else {
                Ok(())
            }
        }
        Ok(Ok(_)) => Err("PROFILE_PROBE_EMPTY".into()),
        Ok(Err(error)) => Err(error),
        Err(_) => Err("PROFILE_PROBE_TIMEOUT".into()),
    }
}
