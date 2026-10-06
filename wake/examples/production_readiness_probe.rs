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
from urllib.parse import urlparse
from seleniumbase import SB

profile, url, bridge_path = sys.argv[1], sys.argv[2], pathlib.Path(sys.argv[3])
spec = importlib.util.spec_from_file_location('wake_probe_bridge', bridge_path)
bridge = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = bridge
spec.loader.exec_module(bridge)
expected_id = urlparse(url).path.split('/')[-1]

def route_class(value):
    try:
        p = urlparse(value)
        if p.scheme == 'chrome-error':
            return 'chrome_error'
        if p.hostname not in {'chatgpt.com','chat.openai.com'}:
            return 'other_host'
        parts = p.path.split('/')[1:]
        if p.path.startswith('/auth/'):
            return 'auth'
        if p.path in {'','/'}:
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

try:
    with SB(uc=True, user_data_dir=profile) as sb:
        opener = getattr(sb, 'uc_open_with_reconnect', None)
        if not callable(opener):
            print('RESULT=UC_UNAVAILABLE', flush=True)
            raise SystemExit(0)
        try:
            opener(url, reconnect_time=3)
        except TypeError:
            opener(url, 3)
        sb.activate_cdp_mode(url)
        sink = bridge.CdpSink(url, pathlib.Path(profile), 30, 20)
        samples = []
        for _ in range(40):
            try:
                current = str(sb.cdp.get_current_url())
                route = route_class(current)
                try:
                    reason = sink.readiness_reason(sb.cdp)
                except bridge.Attention as e:
                    reason = 'ATTN_' + str(e)
                samples.append(route + ':' + ('READY' if reason is None else reason))
            except Exception as e:
                samples.append('exception:' + type(e).__name__)
            time.sleep(.5)
        compressed = []
        for item in samples:
            if not compressed or compressed[-1][0] != item:
                compressed.append([item,1])
            else:
                compressed[-1][1] += 1
        print('RESULT=' + ','.join(item + 'x' + str(count) for item,count in compressed), flush=True)
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
        .map_err(|_| "READINESS_PROBE_START_FAILED")?;
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
    let result = rx.recv_timeout(Duration::from_secs(90));
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Ok(Ok(line)) if !line.trim().is_empty() => {
            println!("{}", line.trim());
            Ok(())
        }
        _ => Err("READINESS_PROBE_FAILED".into()),
    }
}
