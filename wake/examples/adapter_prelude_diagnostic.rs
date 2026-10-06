use catdesk_wake::runtime;
use std::process::Command;

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let current: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("current.json")).map_err(|_| "CURRENT_READ_FAILED")?,
    )
    .map_err(|_| "CURRENT_PARSE_FAILED")?;
    let id = current
        .get("directory")
        .and_then(|v| v.as_str())
        .ok_or("CURRENT_INVALID")?;
    let installed = root.join("versions").join(id);
    let (adapter, _) = runtime::verify_installed_browser_artifacts(&root, &installed)?;
    let python = root.join("runtime").join("python.exe");
    let script = r#"
import contextlib, importlib.util, sys
from pathlib import Path
adapter = Path(sys.argv[1])
steps=[]
try:
    nul = adapter.anchor + 'NUL'
    steps.append('nul=' + nul)
    with open(nul, 'w') as quiet:
        steps.append('nul_open_ok')
        with contextlib.redirect_stdout(quiet), contextlib.redirect_stderr(quiet):
            path = adapter.parent / 'wake_bridge.py'
            spec = importlib.util.spec_from_file_location('wake_primitives_diag', path)
            module = importlib.util.module_from_spec(spec)
            sys.modules[spec.name] = module
            spec.loader.exec_module(module)
            steps.append('primitives_ok')
            from seleniumbase import SB
            steps.append('selenium_import_ok')
    print('OK|' + '|'.join(steps))
except Exception as e:
    print('ERR_CLASS=' + type(e).__module__ + '.' + type(e).__name__)
    print('ERR_TEXT=' + str(e).replace('\r',' ').replace('\n',' ')[:500])
    print('STEPS=' + '|'.join(steps))
"#;
    let output = Command::new(python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(adapter)
        .output()
        .map_err(|_| "DIAGNOSTIC_START_FAILED")?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    Ok(())
}
