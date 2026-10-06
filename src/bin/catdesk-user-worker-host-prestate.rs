//! Zero-argument, read-only T-0379 protected-host prestate inspector.

#![allow(dead_code)]

#[path = "../control_plane_supervisor.rs"]
mod control_plane_supervisor;
#[path = "../user_worker_release.rs"]
mod user_worker_release;
#[path = "../windows_protected_fs.rs"]
mod windows_protected_fs;
#[allow(dead_code)]
#[path = "../windows_supervisor_control_pipe.rs"]
mod windows_supervisor_control_pipe;

fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!("USER_WORKER_HOST_PRESTATE_ZERO_ARGUMENT_ONLY");
        std::process::exit(2);
    }
    match user_worker_release::fixed_protected_host_prestate_json() {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(output) => println!("{output}"),
            Err(_) => {
                eprintln!("USER_WORKER_HOST_PRESTATE_SERIALIZATION_FAILED");
                std::process::exit(1);
            }
        },
        Err(_) => {
            eprintln!("USER_WORKER_HOST_PRESTATE_SERIALIZATION_FAILED");
            std::process::exit(1);
        }
    }
}
