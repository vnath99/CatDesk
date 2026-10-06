use catdesk_wake::{process_job::Job, runtime, store::Store};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
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
    let pointer: Value = serde_json::from_slice(
        &std::fs::read(root.join("current.json")).map_err(|_| "CURRENT_UNAVAILABLE")?,
    )
    .map_err(|_| "CURRENT_INVALID")?;
    let directory = pointer
        .get("directory")
        .and_then(Value::as_str)
        .ok_or("CURRENT_INVALID")?;
    let adapter = root.join("versions").join(directory).join("adapter.py");
    let python = root.join("runtime").join("python.exe");
    let script = r#"
import contextlib, importlib.util, json, os, pathlib, sys
adapter_path, root, url = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3]
channel = sys.stdout
spec = importlib.util.spec_from_file_location('protocol_wake_probe', adapter_path)
adapter = importlib.util.module_from_spec(spec); sys.modules[spec.name] = adapter; spec.loader.exec_module(adapter)
def out(value): channel.write(value + '\n'); channel.flush()
with open(os.devnull, 'w') as quiet:
    with contextlib.redirect_stdout(quiet), contextlib.redirect_stderr(quiet):
        bridge = adapter.load_primitives()
        from seleniumbase import SB
        with SB(uc=True, user_data_dir=str(root / 'browser-profile')) as sb:
            out('READY')
            line = sys.stdin.buffer.readline(8193)
            if not line: out('RESULT=NO_REQUEST')
            else:
                cdp = adapter.prepare_cdp_target(bridge, sb, url)
                out('TARGET_OPEN')
                def observer(route, reason): out('OBS=' + route + ':' + reason)
                sink = bridge.CdpSink(url, root / 'browser-profile', 30, 20)
                try:
                    sink.wait_for_page_readiness(sb, cdp, observer=observer)
                    out('RESULT=READY')
                except bridge.Attention as e:
                    out('RESULT=ATTENTION_' + str(e))
"#;
    let mut command = Command::new(&python);
    command
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(&adapter)
        .arg(&root)
        .arg(&target.url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| "PROTOCOL_PROBE_START_FAILED")?;
    let _job = Job::assign(&child)?;
    let mut input = child.stdin.take().ok_or("STDIN_FAILED")?;
    let stdout = child.stdout.take().ok_or("STDOUT_FAILED")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|_| "READ_FAILED")?;
    println!("{}", line.trim());
    if line.trim() != "READY" {
        return Err("PROTOCOL_READY_FAILED".into());
    }
    std::thread::sleep(Duration::from_secs(2));
    input
        .write_all(b"{\"go\":true}\n")
        .map_err(|_| "WRITE_FAILED")?;
    input.flush().map_err(|_| "WRITE_FAILED")?;
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(|_| "READ_FAILED")? == 0 {
            break;
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            println!("{trimmed}");
        }
        if trimmed.starts_with("RESULT=") {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}
