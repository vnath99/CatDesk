use std::process::Command;

fn main() -> Result<(), String> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA")?;
    let python = std::path::PathBuf::from(local)
        .join("CatDeskWake")
        .join("runtime")
        .join("python.exe");
    let script = r#"
import inspect
from seleniumbase.fixtures.base_case import BaseCase
for name in ('activate_cdp_mode','is_cdp_mode_active','uc_open_with_reconnect','reconnect'):
    value = getattr(BaseCase, name, None)
    print(name.upper() + '=' + (str(inspect.signature(value)) if callable(value) else 'unavailable'))
"#;
    let out = Command::new(python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .output()
        .map_err(|_| "SIGNATURE_PROBE_FAILED")?;
    if !out.status.success() {
        return Err("SIGNATURE_PROBE_FAILED".into());
    }
    print!("{}", String::from_utf8_lossy(&out.stdout));
    Ok(())
}
