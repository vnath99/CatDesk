use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use catdesk_wake::process_job::Job;
use serde_json::{Value, json};

fn main() {
    let local = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA");
    let root = std::path::Path::new(&local).join("CatDeskWake");
    let current: Value =
        serde_json::from_slice(&fs::read(root.join("current.json")).expect("read current.json"))
            .expect("parse current.json");
    let directory = current["directory"].as_str().expect("current directory");
    let install = root.join("versions").join(directory);
    let python = root.join("runtime").join("python.exe");
    let adapter = install.join("adapter.py");

    let mut child = Command::new(&python)
        .arg("-I")
        .arg(&adapter)
        .arg(&root)
        .current_dir(&install)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn installed adapter");
    let job = Job::assign(&child).expect("assign adapter diagnostic job");
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    writeln!(stdin, "{}", json!({"initialize": true})).expect("initialize write");
    stdin.flush().expect("initialize flush");

    let mut line = String::new();
    reader.read_line(&mut line).expect("READY read");
    println!("{}", line.trim());
    let ready: Value = serde_json::from_str(line.trim()).expect("READY json");
    if ready["stage"] != "READY" {
        drop(job);
        let _ = child.wait();
        std::process::exit(2);
    }

    let target = json!({
        "generation": 2,
        "url": "https://chatgpt.com/c/6aa98932-3a84-83e9-afa7-ad10a5c495a5",
        "digest": "621e4450b7abaa8a88508cad39a05d32decb919b192a09eb1259820b420f9a13"
    });
    let event = json!({
        "schemaVersion": 1,
        "eventId": "adapter-dev18-receipt-diagnostic-20260919-1745",
        "projectId": "catdesk",
        "eventType": "diagnostic",
        "createdUtc": 1789854300u64,
        "message": "MANUAL WAKE DEBUG — dev.18 installed-adapter receipt test — NOT natural acceptance.",
        "targetGeneration": 2,
        "auditReferences": []
    });
    writeln!(stdin, "{}", json!({"event": event, "target": target})).expect("event write");
    stdin.flush().expect("event flush");

    loop {
        line.clear();
        if reader.read_line(&mut line).expect("stage read") == 0 {
            println!("ADAPTER_EOF");
            drop(job);
            let _ = child.wait();
            std::process::exit(3);
        }
        let trimmed = line.trim();
        println!("{}", trimmed);
        let value: Value = serde_json::from_str(trimmed).expect("stage json");
        match value["stage"].as_str() {
            Some("BEFORE_SUBMIT") => {
                writeln!(stdin, "{}", json!({"submit": true})).expect("submit write");
                stdin.flush().expect("submit flush");
            }
            Some("SENT") => {
                drop(job);
                let _ = child.wait();
                return;
            }
            Some("ATTENTION") => {
                drop(job);
                let _ = child.wait();
                std::process::exit(4);
            }
            _ => {}
        }
    }
}
