use sha2::{Digest, Sha256};
use std::fs;
fn main() {
    let p = std::env::args().nth(1).expect("path");
    let b = fs::read(&p).expect("read");
    let h = Sha256::digest(&b);
    println!("PATH={p}");
    println!("LEN={}", b.len());
    println!("SHA256={:x}", h);
}
