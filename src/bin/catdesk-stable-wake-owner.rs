//! Fixed no-argument Rust wake owner entrypoint.
//!
//! Host installation/activation is deliberately separate.  When invoked from
//! its project workspace it requires the durable owner selector to be `rust`,
//! selects one canonical pending record deterministically, and has no caller
//! supplied executable, profile, target, state, inbox, or record authority.

#![allow(dead_code)] // Shared state modules also expose test-only probe seams.

#[path = "../stable_wake_adapter_runtime.rs"]
mod stable_wake_adapter_runtime;
#[path = "../stable_wake_core.rs"]
mod stable_wake_core;
#[path = "../stable_wake_delivery.rs"]
mod stable_wake_delivery;
#[path = "../stable_wake_owner.rs"]
mod stable_wake_owner;
#[path = "../stable_wake_owner_mode.rs"]
mod stable_wake_owner_mode;
#[path = "../windows_protected_fs.rs"]
mod windows_protected_fs;

use stable_wake_core::{EventClass, STABLE_PROJECT_ID, discover};
use stable_wake_owner::{FixedScriptBrowserAdapter, OwnerOutcome, dispatch_if_rust_selected};
use std::{
    env,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() -> ExitCode {
    if env::args_os().count() != 1 {
        eprintln!("stable wake owner accepts no arguments");
        return ExitCode::from(2);
    }
    let workspace = match env::current_dir() {
        Ok(value) => value,
        Err(_) => return ExitCode::from(2),
    };
    let record_id = match discover(&workspace, STABLE_PROJECT_ID) {
        Ok(events) => events.into_iter().find_map(|event| match event {
            EventClass::Pending(record) => Some(record.record_id),
            EventClass::Stale(_) => None,
        }),
        Err(_) => return ExitCode::from(2),
    };
    let Some(record_id) = record_id else {
        println!("NO_PENDING_RECORD");
        return ExitCode::SUCCESS;
    };
    let adapter = match FixedScriptBrowserAdapter::open(&workspace) {
        Ok(value) => value,
        Err(_) => return ExitCode::from(2),
    };
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(value) if value.as_secs_f64().is_finite() && value.as_secs_f64() > 0.0 => {
            value.as_secs_f64()
        }
        _ => return ExitCode::from(2),
    };
    match dispatch_if_rust_selected(&workspace, &record_id, now, &adapter) {
        Ok(OwnerOutcome::Sent) => println!("SENT"),
        Ok(OwnerOutcome::PreSubmitRetryable) => println!("PRE_SUBMIT_RETRYABLE"),
        Ok(OwnerOutcome::AlreadyOwned(_)) => println!("ALREADY_OWNED"),
        Ok(OwnerOutcome::OperatorAttention) | Err(_) => println!("OPERATOR_ATTENTION"),
    }
    ExitCode::SUCCESS
}
