use catdesk_wake::{process_job::Job, runtime};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn probe(with_job: bool) -> Result<(), String> {
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
    let mut child = Command::new(python)
        .arg("-I")
        .arg(adapter)
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "ADAPTER_START_FAILED")?;
    let _job = if with_job {
        Some(Job::assign(&child)?)
    } else {
        None
    };
    let mut input = child.stdin.take().ok_or("STDIN_FAILED")?;
    input
        .write_all(b"{\"initialize\":true}\n")
        .and_then(|_| input.flush())
        .map_err(|_| "WRITE_FAILED")?;
    let output = child.stdout.take().ok_or("STDOUT_FAILED")?;
    let mut reader = BufReader::new(output);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|_| "READ_FAILED")?;
    println!(
        "mode={} response={}",
        if with_job { "job" } else { "plain" },
        line.trim()
    );
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

fn main() -> Result<(), String> {
    probe(false)?;
    probe(true)?;
    Ok(())
}
