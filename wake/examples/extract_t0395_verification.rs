use serde_json::Value;
use std::{fs, path::PathBuf};
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let p = root.join(".catdesk/autonomy/execution-accounting.json");
    let text = fs::read_to_string(p).expect("accounting");
    let v: Value = serde_json::from_str(&text).expect("json");
    let records = v.get("records").and_then(Value::as_array).expect("records");
    let id = "adc-t0395-bootstrap-snapshot-producer-reconciliation-20260921-T-0395-BOOTSTRAP-SNAPSHOT-PRODUCER-RECONCILIATION";
    let r = records
        .iter()
        .find(|r| r.get("recordId").and_then(Value::as_str) == Some(id))
        .expect("record");
    println!(
        "{}",
        r.get("verificationReference")
            .and_then(Value::as_str)
            .unwrap_or("")
    );
}
