use catdesk_wake::runtime;
use std::process::Command;

fn probe(label: &str, profile: &std::path::Path, python: &std::path::Path) -> Result<(), String> {
    let script = r#"
import sys, time
from urllib.parse import urlparse
from seleniumbase import SB
profile, url = sys.argv[1], sys.argv[2]
try:
    with SB(uc=True, user_data_dir=profile) as sb:
        sb.activate_cdp_mode(url)
        time.sleep(4)
        current = str(sb.cdp.get_current_url())
        def vis(sel):
            try: return bool(sb.cdp.is_element_visible(sel))
            except Exception: return False
        login = urlparse(current).path.startswith('/auth/') or vis("a[href*='/auth/login']") or vis("button[data-testid*='login' i]")
        editor = vis('#prompt-textarea')
        print('AUTH=' + ('LOGIN_REQUIRED' if login else ('EDITOR_READY' if editor else 'UNKNOWN')))
except Exception as e:
    print('AUTH=ERROR:' + type(e).__name__)
"#;
    let output = Command::new(python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(profile)
        .arg("https://chatgpt.com/c/6aa98932-3a84-83e9-afa7-ad10a5c495a5")
        .output()
        .map_err(|_| "PROFILE_PROBE_START_FAILED")?;
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    println!("{label}:{line}");
    Ok(())
}

fn main() -> Result<(), String> {
    let repo = std::env::current_dir().map_err(|_| "CWD_FAILED")?;
    let root = runtime::default_root()?;
    let python = root.join("runtime").join("python.exe");
    probe(
        "legacy",
        &repo
            .join(".catdesk")
            .join("wake-bridge")
            .join("browser-profile"),
        &python,
    )?;
    probe("independent", &root.join("browser-profile"), &python)?;
    Ok(())
}
