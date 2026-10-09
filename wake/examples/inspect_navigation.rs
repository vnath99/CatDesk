use std::{
    fs,
    path::{Path, PathBuf},
};

fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if matches!(name, ".git" | ".catdesk" | "target" | "node_modules") {
            continue;
        }
        if p.is_dir() {
            visit(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("py") {
            out.push(p);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap_or(&manifest);
    let mut files = Vec::new();
    visit(root, &mut files);
    let needles = [
        "class CdpSink",
        "wait_for_page_readiness",
        "activate_cdp_mode",
        "READINESS_HOME_RECOVERY",
        "TARGET_DRIFT",
        "uc_open_with_reconnect",
    ];
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<_> = text.lines().collect();
        let mut hits = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if needles.iter().any(|n| line.contains(n)) {
                hits.push(i);
            }
        }
        if hits.is_empty() {
            continue;
        }
        println!(
            "FILE={}",
            path.strip_prefix(root).unwrap_or(&path).display()
        );
        for i in hits {
            let start = i.saturating_sub(18);
            let end = (i + 35).min(lines.len());
            println!("--- {}:{} ---", path.display(), i + 1);
            for (j, line) in lines.iter().enumerate().take(end).skip(start) {
                println!("{:05}: {}", j + 1, line);
            }
        }
    }
}
