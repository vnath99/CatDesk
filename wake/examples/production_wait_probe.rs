use catdesk_wake::{process_job::Job, runtime, store::Store};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let target = store
        .config()?
        .targets
        .get("catdesk")
        .ok_or("TARGET_NOT_CONFIGURED")?
        .clone();
    let python = root.join("runtime").join("python.exe");
    let profile = root.join("browser-profile");
    let bridge = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("REPO_ROOT")?
        .join("scripts")
        .join("wake_bridge.py");
    let script = r#"
import importlib.util, pathlib, sys, time
from seleniumbase import SB
profile, url, bridge_path = sys.argv[1], sys.argv[2], pathlib.Path(sys.argv[3])
spec = importlib.util.spec_from_file_location('wake_wait_bridge', bridge_path)
bridge = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = bridge
spec.loader.exec_module(bridge)
try:
    with SB(uc=True, user_data_dir=profile) as sb:
        opener = getattr(sb, 'uc_open_with_reconnect', None)
        if not callable(opener):
            print('RESULT=UC_UNAVAILABLE', flush=True)
            raise SystemExit(0)
        try: opener(url, reconnect_time=3)
        except TypeError: opener(url, 3)
        sb.activate_cdp_mode(url)
        sink = bridge.CdpSink(url, pathlib.Path(profile), 30, 20)
        started = time.monotonic()
        try:
            cdp = sink.wait_for_page_readiness(sb, sb.cdp)
            elapsed = int((time.monotonic() - started) * 1000)
            exact = 'yes' if sink.exact(str(cdp.get_current_url())) else 'no'
            try: editor = 'yes' if bool(cdp.is_element_visible('#prompt-textarea')) else 'no'
            except Exception: editor = 'unknown'
            print(f'RESULT=READY;ELAPSED_MS={elapsed};EXACT={exact};EDITOR={editor}', flush=True)
        except bridge.Attention as e:
            elapsed = int((time.monotonic() - started) * 1000)
            print(f'RESULT=ATTENTION_{e};ELAPSED_MS={elapsed}', flush=True)
except Exception as e:
    print('RESULT=EXCEPTION_' + type(e).__name__, flush=True)
"#;
    let mut child = Command::new(&python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(&profile)
        .arg(&target.url)
        .arg(&bridge)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "WAIT_PROBE_START_FAILED")?;
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
    let result = rx.recv_timeout(Duration::from_secs(150));
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Ok(Ok(line)) if !line.trim().is_empty() => {
            println!("{}", line.trim());
            Ok(())
        }
        _ => Err("WAIT_PROBE_FAILED".into()),
    }
}
