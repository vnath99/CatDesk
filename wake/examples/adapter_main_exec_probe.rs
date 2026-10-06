use catdesk_wake::{process_job::Job, runtime, store::Store};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let target = store
        .config()?
        .targets
        .get("catdesk")
        .ok_or("TARGET_NOT_CONFIGURED")?
        .clone();
    let event = store
        .events()?
        .into_iter()
        .find(|event| event.event_id == "manual-wake-dev26-001")
        .ok_or("EVENT_NOT_FOUND")?;
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
    let loader = r#"
import pathlib, sys
adapter_path, root = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
source = adapter_path.read_text(encoding='utf-8')
probe = r'''
def probe_only(bridge, sb, event, target):
    url = bridge.canonical_conversation_url(target["url"])
    if url != target["url"] or bridge.digest(url) != target["digest"]:
        raise bridge.Attention("TARGET_IDENTITY_MISMATCH")
    if event["targetGeneration"] != target["generation"]:
        raise bridge.Attention("TARGET_GENERATION_STALE")
    sink = bridge.CdpSink(url, ROOT / "browser-profile", 30, 20)
    cdp = prepare_cdp_target(bridge, sb, url)
    emit({"stage": "TARGET_OPEN"})
    def observer(route, reason):
        emit({"stage": "READINESS_OBSERVED", "route": route, "reason": reason})
    sink.wait_for_page_readiness(sb, cdp, observer=observer)
    emit({"stage": "PROBE_READY"})
'''
needle = '\ndef main():\n'
if needle not in source:
    raise SystemExit('MAIN_NOT_FOUND')
source = source.replace(needle, '\n' + probe + '\ndef main():\n', 1)
call = 'attempt(bridge, sb, request["event"], request["target"])'
if call not in source:
    raise SystemExit('ATTEMPT_CALL_NOT_FOUND')
source = source.replace(call, 'probe_only(bridge, sb, request["event"], request["target"])', 1)
sys.argv = [str(adapter_path), str(root)]
namespace = {
    "__name__": "__main__",
    "__file__": str(adapter_path),
    "__package__": None,
}
exec(compile(source, str(adapter_path), 'exec'), namespace, namespace)
"#;
    let mut command = Command::new(&python);
    command
        .arg("-I")
        .arg("-c")
        .arg(loader)
        .arg(&adapter)
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| "MAIN_PROBE_START_FAILED")?;
    let _job = Job::assign(&child)?;
    let mut input = child.stdin.take().ok_or("STDIN_FAILED")?;
    let stdout = child.stdout.take().ok_or("STDOUT_FAILED")?;
    let mut reader = BufReader::new(stdout);

    input
        .write_all(b"{\"initialize\":true}\n")
        .map_err(|_| "WRITE_FAILED")?;
    input.flush().map_err(|_| "WRITE_FAILED")?;

    let mut line = String::new();
    reader.read_line(&mut line).map_err(|_| "READ_FAILED")?;
    println!("{}", line.trim());
    let ready: Value = serde_json::from_str(line.trim()).map_err(|_| "READY_INVALID")?;
    if ready.get("stage").and_then(Value::as_str) != Some("READY") {
        return Err("READY_FAILED".into());
    }

    let request = json!({"event": event, "target": target});
    let mut bytes = serde_json::to_vec(&request).map_err(|_| "REQUEST_INVALID")?;
    bytes.push(b'\n');
    input.write_all(&bytes).map_err(|_| "WRITE_FAILED")?;
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
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(_) => continue,
        };
        match value.get("stage").and_then(Value::as_str) {
            Some("PROBE_READY" | "ATTENTION") => break,
            _ => {}
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}
