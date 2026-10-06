use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let bin = root.join("target").join("release").join("catdesk.exe");
    let sha = root
        .join("target")
        .join("release")
        .join("catdesk.exe.sha256");
    let bin_bytes = fs::read(&bin).expect("read binary");
    let actual = hex(&Sha256::digest(&bin_bytes));
    let expected = fs::read_to_string(&sha)
        .expect("read sha")
        .trim()
        .to_ascii_lowercase();
    println!("BINARY_EXISTS=yes");
    println!("BINARY_LEN={}", bin_bytes.len());
    println!("ACTUAL_SHA256={}", actual);
    println!("EXPECTED_SHA256={}", expected);
    println!("MATCH={}", if actual == expected { "yes" } else { "no" });
}
