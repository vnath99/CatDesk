use std::{fs, path::PathBuf};
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".catdesk/logs");
    let mut names = fs::read_dir(root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect::<Vec<_>>();
    names.sort();
    for n in names.into_iter().rev().take(20) {
        println!("{n}");
    }
}
