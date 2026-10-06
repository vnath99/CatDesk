use std::env;
fn main() {
    for key in [
        "PATH",
        "LIB",
        "LIBPATH",
        "INCLUDE",
        "SystemRoot",
        "TEMP",
        "TMP",
    ] {
        let v = env::var_os(key).unwrap_or_default();
        if key == "PATH" || key == "LIB" || key == "LIBPATH" || key == "INCLUDE" {
            println!("{key}_BEGIN");
            for p in env::split_paths(&v) {
                let s = p.to_string_lossy();
                let l = s.to_ascii_lowercase();
                if l.contains("visual studio")
                    || l.contains("windows kits")
                    || l.contains("\\windows\\system32")
                    || l.contains(".rustup")
                    || l.contains(".cargo")
                {
                    println!("{s}");
                }
            }
            println!("{key}_END");
        } else {
            println!("{key}={}", v.to_string_lossy());
        }
    }
}
