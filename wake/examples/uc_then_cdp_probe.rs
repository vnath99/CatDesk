use catdesk_wake::{process_job::Job, runtime, store::Store};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let config = store.config()?;
    let target = config
        .targets
        .get("catdesk")
        .ok_or("TARGET_NOT_CONFIGURED")?;
    let python = root.join("runtime").join("python.exe");
    let profile = root.join("browser-profile");
    let script = r#"
import sys, time
from urllib.parse import urlparse
from seleniumbase import SB

profile, url = sys.argv[1], sys.argv[2]
expected_id = urlparse(url).path.split('/')[-1]

def classify(value):
    try:
        p = urlparse(value)
        if p.scheme == 'chrome-error': return 'chrome_error'
        if p.hostname not in {'chatgpt.com','chat.openai.com'}: return 'other_host'
        parts = p.path.split('/')[1:]
        if p.path.startswith('/auth/'): return 'auth'
        if p.path in {'','/'}: return 'home'
        if len(parts) == 2 and parts[0] == 'c':
            return 'same_c' if parts[1] == expected_id else 'different_c'
        if len(parts) == 4 and parts[0] == 'g' and parts[2] == 'c':
            return 'same_g_c' if parts[3] == expected_id else 'different_g_c'
        return 'other_route'
    except Exception:
        return 'invalid'

try:
    with SB(uc=True, user_data_dir=profile) as sb:
        opener = getattr(sb, 'uc_open_with_reconnect', None)
        if not callable(opener):
            print('STEP=uc_unavailable', flush=True)
        else:
            try: opener(url, reconnect_time=3)
            except TypeError: opener(url, 3)
            pre = classify(str(sb.driver.current_url))
            try:
                pre_editor = len([e for e in sb.driver.find_elements('css selector','#prompt-textarea') if e.is_displayed()]) == 1
            except Exception:
                pre_editor = False
            sb.activate_cdp_mode()
            deadline = time.time() + 20
            post, post_editor = 'unknown', False
            while time.time() < deadline:
                try:
                    current = str(sb.cdp.get_current_url())
                    post = classify(current)
                    try: post_editor = bool(sb.cdp.is_element_visible('#prompt-textarea'))
                    except Exception: post_editor = False
                    if post_editor or post not in {'same_c','same_g_c','unknown'}:
                        break
                except Exception:
                    post = 'driver_unready'
                time.sleep(.25)
            print(
                'PRE=' + pre + ';PRE_EDITOR=' + ('yes' if pre_editor else 'no') +
                ';POST=' + post + ';POST_EDITOR=' + ('yes' if post_editor else 'no'),
                flush=True
            )
except Exception as e:
    print('STEP=exception_' + type(e).__name__, flush=True)
"#;
    let mut child = Command::new(&python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(&profile)
        .arg(&target.url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "UC_CDP_PROBE_START_FAILED")?;
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
    let result = rx.recv_timeout(Duration::from_secs(60));
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Ok(Ok(line)) if !line.trim().is_empty() => {
            println!("{}", line.trim());
            Ok(())
        }
        _ => Err("UC_CDP_PROBE_FAILED".into()),
    }
}
