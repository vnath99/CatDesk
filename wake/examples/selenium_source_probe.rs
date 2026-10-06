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
src = inspect.getsource(BaseCase.activate_cdp_mode)
for line in src.splitlines()[:120]:
    print(line)
"#;
    let out = Command::new(python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .output()
        .map_err(|_| "SOURCE_PROBE_FAILED")?;
    if !out.status.success() {
        return Err("SOURCE_PROBE_FAILED".into());
    }
    print!("{}", String::from_utf8_lossy(&out.stdout));
    Ok(())
}
