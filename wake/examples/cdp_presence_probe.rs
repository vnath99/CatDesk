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
import sys
from seleniumbase import SB
profile, url = sys.argv[1], sys.argv[2]
with SB(uc=True, user_data_dir=profile) as sb:
    cdp = getattr(sb, 'cdp', None)
    print(
        'PRE_CDP_OBJECT=' + ('yes' if cdp is not None else 'no') +
        ';PRE_CDP_GET=' + ('yes' if callable(getattr(cdp, 'get', None)) else 'no') +
        ';UC_OPEN=' + ('yes' if callable(getattr(sb, 'uc_open_with_reconnect', None)) else 'no'),
        flush=True
    )
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
        .map_err(|_| "PRESENCE_PROBE_START_FAILED")?;
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
    let result = rx.recv_timeout(Duration::from_secs(30));
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Ok(Ok(line)) if !line.trim().is_empty() => {
            println!("{}", line.trim());
            Ok(())
        }
        _ => Err("PRESENCE_PROBE_FAILED".into()),
    }
}
