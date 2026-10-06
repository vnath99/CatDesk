use serde_json::Value;
use std::{fs, path::PathBuf};
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let text =
        fs::read_to_string(root.join(".catdesk/autonomy/execution-accounting.json")).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let id = "adc-t0395-bootstrap-snapshot-producer-reconciliation-20260921-T-0395-BOOTSTRAP-SNAPSHOT-PRODUCER-RECONCILIATION";
    let r = v["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["recordId"].as_str() == Some(id))
        .unwrap();
    let s = r["verificationReference"].as_str().unwrap_or("");
    for needle in [
        "FAILED",
        "failures:",
        "panicked at",
        "test result:",
        "catdesk_binagotchy_command",
        "CARGO_FMT",
        "CARGO_TEST",
        "CARGO_BUILD_RELEASE_ISOLATED",
    ] {
        println!("=== {needle} ===");
        let mut start = 0;
        while let Some(i) = s[start..].find(needle) {
            let at = start + i;
            let lo = at.saturating_sub(700);
            let hi = (at + 1200).min(s.len());
            println!("{}", &s[lo..hi]);
            start = at + needle.len();
        }
    }
}
