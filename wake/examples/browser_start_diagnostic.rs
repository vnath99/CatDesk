use catdesk_wake::runtime;
use std::process::Command;

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let python = root.join("runtime").join("python.exe");
    let profile = root.join("browser-profile");
    let script = r#"
import sys
from seleniumbase import SB
profile = sys.argv[1]
try:
    with SB(uc=True, user_data_dir=profile) as sb:
        print('OK')
except Exception as e:
    text = str(e).replace('\r',' ').replace('\n',' ')
    print('ERR_CLASS=' + type(e).__module__ + '.' + type(e).__name__)
    print('ERR_TEXT=' + text[:500])
"#;
    let output = Command::new(&python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(&profile)
        .output()
        .map_err(|_| "DIAGNOSTIC_PYTHON_START_FAILED")?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err("DIAGNOSTIC_PYTHON_FAILED".into());
    }
    Ok(())
}
