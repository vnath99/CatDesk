//! Separately buildable, zero-argument stable supervisor listener.
//!
//! It reads only an already provisioned supervisor root, then binds the
//! compiled loopback front door. It never starts or reconfigures a tunnel.

#![allow(dead_code)]

#[path = "../control_plane_supervisor.rs"]
mod control_plane_supervisor;
#[path = "../windows_protected_fs.rs"]
mod windows_protected_fs;
#[allow(dead_code)]
#[path = "../windows_supervisor_control_pipe.rs"]
mod windows_supervisor_control_pipe;

use control_plane_supervisor::run_fixed_stable_supervisor_runtime;

#[tokio::main]
async fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!("CONTROL_PLANE_SUPERVISOR_ZERO_ARGUMENT_ONLY");
        std::process::exit(2);
    }
    if let Err(reason) = run_fixed_stable_supervisor_runtime().await {
        if matches!(
            reason,
            control_plane_supervisor::FixedStableSupervisorRuntimeErrorV1::NotInstalledOrUnavailable
                | control_plane_supervisor::FixedStableSupervisorRuntimeErrorV1::StateUnavailable
        ) {
            println!("{}", serde_json::json!({"readiness": reason.as_str()}));
            return;
        }
        eprintln!("{}", reason.as_str());
        std::process::exit(1);
    }
}
