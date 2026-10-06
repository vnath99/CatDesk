//! Fixed workspace migration command; --preflight is read-only.
#![allow(dead_code)]
#[path = "../stable_wake_adapter_runtime.rs"]
mod stable_wake_adapter_runtime;
#[path = "../stable_wake_core.rs"]
mod stable_wake_core;
#[path = "../stable_wake_delivery.rs"]
mod stable_wake_delivery;
#[path = "../stable_wake_owner_mode.rs"]
mod stable_wake_owner_mode;
#[path = "../windows_protected_fs.rs"]
mod windows_protected_fs;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let preflight = match args.as_slice() {
        [] => false,
        [arg] if arg == "--preflight" => true,
        _ => {
            eprintln!("MIGRATION_ARGUMENTS_INVALID");
            std::process::exit(2);
        }
    };
    let result = std::env::current_dir()
        .map_err(|_| "MIGRATION_WORKSPACE_UNAVAILABLE".into())
        .and_then(|workspace| {
            stable_wake_owner_mode::migrate_independent_owner(&workspace, preflight)
        });
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
