//! Test-only process probe for the stable delivery kernel mutex.
//!
//! It has no browser, inbox, target, or state-transition authority.  The
//! integration test uses it to prove that abrupt process death releases the
//! exact mutex used by production claim/receipt transitions.

#[allow(dead_code)]
#[path = "../stable_wake_core.rs"]
mod stable_wake_core;
#[allow(dead_code)]
#[path = "../stable_wake_delivery.rs"]
mod stable_wake_delivery;

use std::{env, io::Write, path::PathBuf, process::ExitCode, thread, time::Duration};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("--workspace") {
        return ExitCode::from(2);
    }
    let Some(workspace) = args.next() else {
        return ExitCode::from(2);
    };
    let Some(mode) = args.next() else {
        return ExitCode::from(2);
    };
    if workspace.len() > 32_768 {
        return ExitCode::from(2);
    }
    match mode.as_str() {
        "--hold-ms" => {
            let Some(hold_ms) = args.next().and_then(|value| value.parse::<u64>().ok()) else {
                return ExitCode::from(2);
            };
            if args.next().is_some() || hold_ms > 60_000 {
                return ExitCode::from(2);
            }
            match stable_wake_delivery::StableWakeDelivery::acquire_lock_for_probe(&PathBuf::from(
                workspace,
            )) {
                Ok(_lock) => {
                    println!("READY");
                    let _ = std::io::stdout().flush();
                    thread::sleep(Duration::from_millis(hold_ms));
                    ExitCode::SUCCESS
                }
                Err(error) if error == "stable wake state busy" => {
                    println!("BUSY");
                    ExitCode::from(3)
                }
                Err(_) => ExitCode::from(2),
            }
        }
        "--claim" => {
            let Some(record_id) = args.next() else {
                return ExitCode::from(2);
            };
            if args.next().is_some() {
                return ExitCode::from(2);
            }
            match stable_wake_delivery::StableWakeDelivery::claim_for_probe(
                &PathBuf::from(workspace),
                &record_id,
            ) {
                Ok(true) => {
                    println!("TRANSITIONED");
                    ExitCode::SUCCESS
                }
                Ok(false) => {
                    println!("NO_TRANSITION");
                    ExitCode::SUCCESS
                }
                Err(error) if error == "stable wake state busy" => {
                    println!("BUSY");
                    ExitCode::from(3)
                }
                Err(_) => ExitCode::from(2),
            }
        }
        _ => ExitCode::from(2),
    }
}
