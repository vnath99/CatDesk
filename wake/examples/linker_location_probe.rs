use std::{env, path::PathBuf};
fn main() {
    let path = env::var_os("PATH").unwrap_or_default();
    for dir in env::split_paths(&path) {
        let link = dir.join("link.exe");
        if link.is_file() {
            println!("LINK={}", link.display());
        }
        let lld = dir.join("lld-link.exe");
        if lld.is_file() {
            println!("LLD_LINK={}", lld.display());
        }
    }
    let rustc = PathBuf::from(env::var_os("USERPROFILE").unwrap_or_default())
        .join(".rustup\\toolchains\\stable-x86_64-pc-windows-msvc\\bin");
    for name in ["link.exe", "lld-link.exe", "rust-lld.exe"] {
        let p = rustc.join(name);
        if p.is_file() {
            println!("RUST_BIN={}", p.display());
        }
    }
}
