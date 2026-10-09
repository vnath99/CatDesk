use catdesk_wake::{process_job::Job, runtime, store::Store};
use serde_json::{Value, json};
use std::fs::{self, File};
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
        &fs::read(root.join("current.json")).map_err(|_| "CURRENT_UNAVAILABLE")?,
    )
    .map_err(|_| "CURRENT_INVALID")?;
    let directory = pointer
        .get("directory")
        .and_then(Value::as_str)
        .ok_or("CURRENT_INVALID")?;
    let installed = root.join("versions").join(directory);
    let source =
        fs::read_to_string(installed.join("adapter.py")).map_err(|_| "ADAPTER_READ_FAILED")?;
    let bridge = fs::read(installed.join("wake_bridge.py")).map_err(|_| "BRIDGE_READ_FAILED")?;

    let probe_dir = root.join(format!("script-probe-{}", std::process::id()));
    let _ = fs::remove_dir_all(&probe_dir);
    fs::create_dir(&probe_dir).map_err(|_| "PROBE_DIR_FAILED")?;
    fs::write(probe_dir.join("wake_bridge.py"), bridge).map_err(|_| "BRIDGE_WRITE_FAILED")?;

    let probe = r#"
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
"#;
    let needle = "\ndef main():\n";
    let call = "attempt(bridge, sb, request[\"event\"], request[\"target\"])";
    if !source.contains(needle) || !source.contains(call) {
        return Err("ADAPTER_SHAPE_CHANGED".into());
    }
    let modified = source
        .replacen(needle, &format!("\n{probe}\ndef main():\n"), 1)
        .replacen(
            call,
            "probe_only(bridge, sb, request[\"event\"], request[\"target\"])",
            1,
        );
    let probe_adapter = probe_dir.join("adapter.py");
    fs::write(&probe_adapter, modified).map_err(|_| "ADAPTER_WRITE_FAILED")?;

    let python = root.join("runtime").join("python.exe");
    let mut command = Command::new(&python);
    command
        .arg("-I")
        .arg(&probe_adapter)
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(
            File::options()
                .create(true)
                .append(true)
                .open(root.join("logs/adapter-script-probe.log"))
                .map_err(|_| "PROBE_LOG_UNAVAILABLE")?,
        ));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| "SCRIPT_PROBE_START_FAILED")?;
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
        if let Some("PROBE_READY" | "ATTENTION") = value.get("stage").and_then(Value::as_str) {
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    drop(_job);
    let _ = fs::remove_dir_all(probe_dir);
    Ok(())
}
