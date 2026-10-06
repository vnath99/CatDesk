use sha2::{Digest, Sha256};
fn main() -> Result<(), String> {
    let p = std::env::current_dir()
        .map_err(|_| "CWD")?
        .join("target/release/catdesk.exe");
    let b = std::fs::read(&p).map_err(|_| "RELEASE_MISSING")?;
    let m = std::fs::metadata(&p).map_err(|_| "META")?;
    println!("PATH={}", p.display());
    println!("LEN={}", m.len());
    println!("SHA256={:x}", Sha256::digest(&b));
    Ok(())
}
