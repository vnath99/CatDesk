//! Closed, no-argument owner-selector activation tool.
//!
//! This is intentionally not scheduled or invoked by the CatDesk server. It
//! exists solely as the fixed product-owned future cutover boundary; running
//! it is a separate host-live operation after independent review.

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

use stable_wake_owner_mode::{WakeOwnerMode, activate_reviewed_rust_owner};
use std::{env, process::ExitCode};

fn main() -> ExitCode {
    if env::args_os().count() != 1 {
        eprintln!("stable wake activation accepts no arguments");
        return ExitCode::from(2);
    }
    let workspace = match env::current_dir() {
        Ok(workspace) => workspace,
        Err(_) => return ExitCode::from(2),
    };
    match activate_reviewed_rust_owner(&workspace, WakeOwnerMode::LegacyPython) {
        Ok(WakeOwnerMode::Rust) => {
            println!("RUST_OWNER_SELECTED");
            ExitCode::SUCCESS
        }
        Ok(WakeOwnerMode::LegacyPython) | Err(_) => ExitCode::from(2),
    }
}
