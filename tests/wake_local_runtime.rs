use std::path::PathBuf;
use std::process::Command;

#[test]
fn wake_crate_tests_pass_through_root_suite() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = root.join("wake").join("Cargo.toml");
    let target_dir = root.join("target").join("wake-local-runtime-verification");
    let cargo = std::env::var_os("CARGO").expect("Cargo must expose its own executable path");

    let output = Command::new(cargo)
        .current_dir(&root)
        .arg("test")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--features")
        .arg("test-support")
        .arg("--offline")
        .arg("--target-dir")
        .arg(&target_dir)
        .output()
        .expect("run Wake-local Cargo tests");

    if !output.status.success() {
        let diagnostic = format!(
            "Wake-local Cargo tests failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        let _ = std::fs::create_dir_all(&target_dir);
        let _ = std::fs::write(target_dir.join("last-failure.log"), diagnostic.as_bytes());
        panic!("{diagnostic}");
    }
}
