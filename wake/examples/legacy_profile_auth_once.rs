use catdesk_wake::{process_job::Job, runtime};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn main() -> Result<(), String> {
    let repo = std::env::current_dir().map_err(|_| "CWD_FAILED")?;
    let root = runtime::default_root()?;
    let python = root.join("runtime").join("python.exe");
    let profile = repo
        .join(".catdesk")
        .join("wake-bridge")
        .join("browser-profile");
    let script = r#"
import sys, time
from urllib.parse import urlparse
from seleniumbase import SB
profile, url = sys.argv[1], sys.argv[2]
try:
    with SB(uc=True, user_data_dir=profile) as sb:
        sb.activate_cdp_mode(url)
        deadline = time.time() + 15
        state = 'UNKNOWN'
        while time.time() < deadline:
            current = str(sb.cdp.get_current_url())
            def vis(sel):
                try: return bool(sb.cdp.is_element_visible(sel))
                except Exception: return False
            if urlparse(current).path.startswith('/auth/') or vis("a[href*='/auth/login']") or vis("button[data-testid*='login' i]"):
                state = 'LOGIN_REQUIRED'; break
            if vis('#prompt-textarea'):
                state = 'EDITOR_READY'; break
            time.sleep(.25)
        print('AUTH=' + state, flush=True)
        time.sleep(60)
except Exception as e:
    print('AUTH=ERROR:' + type(e).__name__, flush=True)
"#;
    let mut child = Command::new(&python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(&profile)
        .arg("https://chatgpt.com/c/6aa98932-3a84-83e9-afa7-ad10a5c495a5")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|_| "PROFILE_PROBE_START_FAILED")?;
    let _job = Job::assign(&child)?;
    let stdout = child.stdout.take().ok_or("STDOUT_FAILED")?;
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .map_err(|_| "READ_FAILED")?;
    println!("{}", line.trim());
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}
