use std::{env, fs, process::Command};
fn main() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|_| "CWD")?;
    let dir = root
        .join("target-verify")
        .join("t0398-link-env-probe")
        .join("systemdrive-temp");
    fs::create_dir_all(&dir).map_err(|_| "CREATE")?;
    let src = dir.join("probe.rs");
    fs::write(&src, b"fn main() {}\n").map_err(|_| "WRITE")?;
    let out = dir.join("probe.exe");
    let tmp = dir.join("tmp");
    fs::create_dir_all(&tmp).map_err(|_| "TMP")?;
    let tool_path = env::var_os("USERPROFILE")
        .map(|profile| {
            std::path::PathBuf::from(profile)
                .join(".rustup\\toolchains\\stable-x86_64-pc-windows-msvc\\bin")
        })
        .ok_or("USERPROFILE")?;
    let result = Command::new(tool_path.join("rustc.exe"))
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .env_clear()
        .env("PATH", &tool_path)
        .env("SystemDrive", "C:")
        .env("TEMP", &tmp)
        .env("TMP", &tmp)
        .output()
        .map_err(|_| "SPAWN")?;
    println!("EXIT={:?}|OUTPUT={}", result.status.code(), out.is_file());
    for line in String::from_utf8_lossy(&result.stderr).lines().take(30) {
        println!("{line}");
    }
    Ok(())
}
