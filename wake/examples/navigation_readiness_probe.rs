use catdesk_wake::{process_job::Job, runtime, store::Store};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), String> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "current".into());
    if !matches!(mode.as_str(), "current" | "uc-warmup") {
        return Err("NAVIGATION_PROBE_MODE_INVALID".into());
    }
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let target = store
        .config()?
        .targets
        .get("catdesk")
        .ok_or("TARGET_NOT_CONFIGURED")?
        .clone();
    let pointer: Value = serde_json::from_slice(
        &std::fs::read(root.join("current.json")).map_err(|_| "CURRENT_UNAVAILABLE")?,
    )
    .map_err(|_| "CURRENT_INVALID")?;
    let directory = pointer
        .get("directory")
        .and_then(Value::as_str)
        .ok_or("CURRENT_INVALID")?;
    let installed = root.join("versions").join(directory);
    let adapter = installed.join("adapter.py");
    let python = root.join("runtime").join("python.exe");
    let script = r#"
import contextlib, importlib.util, os, pathlib, sys, time
adapter_path, root, url, mode = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3], sys.argv[4]
channel = sys.stdout
spec = importlib.util.spec_from_file_location('navigation_probe_adapter', adapter_path)
adapter = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = adapter
spec.loader.exec_module(adapter)
def out(value):
    channel.write(value + '\n'); channel.flush()
with open(os.devnull, 'w') as quiet:
    with contextlib.redirect_stdout(quiet), contextlib.redirect_stderr(quiet):
        try:
            bridge = adapter.load_primitives()
            from seleniumbase import SB
            with SB(uc=True, user_data_dir=str(root / 'browser-profile')) as sb:
                if mode == 'current':
                    cdp = adapter.prepare_cdp_target(bridge, sb, url)
                    out('METHOD=CURRENT_CDP')
                else:
                    opener = getattr(sb, 'uc_open_with_reconnect', None)
                    if not callable(opener):
                        raise RuntimeError('UC_OPEN_UNAVAILABLE')
                    try:
                        opener(url, reconnect_time=3)
                    except TypeError:
                        opener(url, 3)
                    sb.activate_cdp_mode()
                    cdp = sb.cdp
                    out('METHOD=UC_WARMUP_CDP')
                sink = bridge.CdpSink(url, root / 'browser-profile', 30, 20)
                deadline = time.monotonic() + 20
                last = None
                ready_since = None
                while time.monotonic() < deadline:
                    try:
                        current = str(cdp.get_current_url())
                    except Exception:
                        current = ''
                    route = bridge.bounded_route_class(current, url) if current else 'INVALID_ROUTE'
                    reason = sink.readiness_reason(cdp, current) if current else 'EDITOR_SELECTOR'
                    label = route + ':' + ('READY' if reason is None else reason)
                    if label != last:
                        out('OBS=' + label)
                        last = label
                    if reason is None:
                        if ready_since is None:
                            ready_since = time.monotonic()
                        if time.monotonic() - ready_since >= 10:
                            out('RESULT=STABLE_READY')
                            break
                    else:
                        ready_since = None
                    time.sleep(.25)
                else:
                    out('RESULT=NOT_READY')
        except Exception as e:
            out('RESULT=EXCEPTION_' + type(e).__name__)
"#;
    let mut child = Command::new(&python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(&adapter)
        .arg(&root)
        .arg(&target.url)
        .arg(&mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "NAVIGATION_PROBE_START_FAILED")?;
    let _job = Job::assign(&child)?;
    let stdout = child.stdout.take().ok_or("STDOUT_FAILED")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() {
                        lines.push(trimmed.clone());
                    }
                    if trimmed.starts_with("RESULT=") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(lines);
    });
    let result = rx.recv_timeout(Duration::from_secs(40));
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            Ok(())
        }
        Err(_) => Err("NAVIGATION_PROBE_TIMEOUT".into()),
    }
}
