use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
fn show(root: &PathBuf, rel: &str) {
    let p = root.join(rel);
    match fs::read(&p) {
        Ok(bytes) => println!(
            "{}|len={}|sha256={}",
            rel,
            bytes.len(),
            hex(&Sha256::digest(&bytes))
        ),
        Err(_) => println!("{}|MISSING", rel),
    }
}
fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().to_path_buf();
    show(&root, "target/release/catdesk.exe");
    show(
        &root,
        ".catdesk/verification-targets/autonomy-release/release/catdesk.exe",
    );
    show(&root, ".catdesk/release-recovery/slot-a/catdesk.exe");
}
