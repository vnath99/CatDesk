use catdesk_wake::{process_job::Job, runtime, store::Store};
use serde_json::Value;
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
import contextlib, importlib.util, os, pathlib, sys, time
adapter_path, root, url = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3]
channel = sys.stdout
spec = importlib.util.spec_from_file_location('exact_hidden_wake_adapter_probe', adapter_path)
adapter = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = adapter
spec.loader.exec_module(adapter)
def out(value):
    channel.write(value + '\n'); channel.flush()
with open(os.devnull, 'w') as quiet:
    with contextlib.redirect_stdout(quiet), contextlib.redirect_stderr(quiet):
        try:
            bridge = adapter.load_primitives()
            from seleniumbase import SB
            with SB(uc=True, user_data_dir=str(root / 'browser-profile')) as sb:
                cdp = adapter.prepare_cdp_target(bridge, sb, url)
                out('STAGE=TARGET_OPEN')
                observations = []
                def observer(route, reason):
                    label = route + ':' + reason
                    if not observations or observations[-1] != label:
                        observations.append(label); out('OBS=' + label)
                sink = bridge.CdpSink(url, root / 'browser-profile', 30, 20)
                try:
                    time.sleep(10)
                    message = "MANUAL WAKE DEBUG — Binagotchy MCP test manual-wake-mcp-1790893041039 — NOT natural acceptance."
                    out('ANCHOR=' + repr(sink.trusted_turn_anchor_state(cdp, message)))
                    out('SNAPSHOT=' + repr(sink.response_snapshot(cdp, message)))
                    out('RESULT=READY')
                except bridge.Attention as e:
                    out('RESULT=ATTENTION_' + str(e))
        except Exception as e:
            out('RESULT=EXCEPTION_' + type(e).__name__)
"#;
    let mut command = Command::new(&python);
    command
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(&adapter)
        .arg(&root)
        .arg(&target.url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| "HIDDEN_PROBE_START_FAILED")?;
    let _job = Job::assign(&child)?;
    let stdout = child.stdout.take().ok_or("STDOUT_FAILED")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() {
                        lines.push(trimmed.clone());
                    }
                    if trimmed.starts_with("RESULT=") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(lines);
    });
    let result = rx.recv_timeout(Duration::from_secs(150));
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            Ok(())
        }
        Err(_) => Err("HIDDEN_PROBE_TIMEOUT".into()),
    }
}
