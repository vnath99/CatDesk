use std::{env, fs, path::PathBuf, process::Command};
fn main() -> Result<(), String> {
    let root = env::current_dir().map_err(|_| "CWD")?;
    let dir = root.join("target-verify").join("t0398-env-diff");
    fs::create_dir_all(&dir).map_err(|_| "CREATE")?;
    let src = dir.join("probe.rs");
    fs::write(&src, b"fn main() {}\n").map_err(|_| "WRITE")?;
    let toolchain = PathBuf::from(env::var_os("USERPROFILE").ok_or("USERPROFILE")?)
        .join(".rustup\\toolchains\\stable-x86_64-pc-windows-msvc\\bin");
    let rustc = toolchain.join("rustc.exe");
    let mut vars: Vec<_> = env::vars_os().collect();
    vars.sort_by_key(|(k, _)| k.to_string_lossy().to_ascii_lowercase());
    let tmp = dir.join("tmp");
    fs::create_dir_all(&tmp).map_err(|_| "TMP")?;
    for (idx, (k, v)) in vars.into_iter().enumerate() {
        let name = k.to_string_lossy().to_string();
        if ["PATH", "TEMP", "TMP", "SystemRoot"]
            .iter()
            .any(|x| x.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        let out = dir.join(format!("p{idx}.exe"));
        let mut cmd = Command::new(&rustc);
        cmd.arg(&src)
            .arg("-o")
            .arg(&out)
            .env_clear()
            .env("PATH", &toolchain)
            .env("SystemRoot", r"C:\Windows")
            .env("TEMP", &tmp)
            .env("TMP", &tmp)
            .env(&k, &v);
        if let Ok(result) = cmd.output()
            && result.status.success()
        {
            println!("SINGLE_VAR_RESTORES={name}");
        }
        let _ = fs::remove_file(out);
    }
    Ok(())
}
