//! Foreground development hosts belong to the invoking process. The child
//! cannot enter its runtime before its private kill-on-close Job is assigned.

use crate::{Result, process_job::Job};
use std::{
    io::{Read, Write},
    os::windows::process::CommandExt,
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

pub(super) const OWNER_GATE: &[u8] = b"CATDESK_DEV_FOREGROUND_OWNER_V1\n";

pub(super) fn require_owner(reader: impl Read) -> Result<()> {
    let mut value = Vec::new();
    reader
        .take((OWNER_GATE.len() + 1) as u64)
        .read_to_end(&mut value)
        .map_err(|_| "DEV_RUNTIME_FOREGROUND_OWNER_UNAVAILABLE")?;
    if value != OWNER_GATE {
        return Err("DEV_RUNTIME_FOREGROUND_OWNER_UNAVAILABLE".into());
    }
    Ok(())
}

#[cfg(test)]
fn run(command: &mut Command) -> Result<(u32, ExitStatus)> {
    run_with_deadline(command, None, || Ok(())).map(|(pid, exit, _)| (pid, exit))
}

pub(super) fn run_with_deadline(
    command: &mut Command,
    budget: Option<Duration>,
    stop: impl FnOnce() -> Result<()>,
) -> Result<(u32, ExitStatus, bool)> {
    let mut child = command
        .stdin(Stdio::piped())
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|_| "DEV_RUNTIME_START_FAILED")?;
    let job = match Job::assign(&child) {
        Ok(job) => job,
        Err(_) => {
            // Before the gate, EOF also makes the child exit without runtime
            // activity. Reap our exact child even if Job assignment failed.
            drop(child.stdin.take());
            let _ = child.kill();
            let _ = child.wait();
            return Err("DEV_RUNTIME_FOREGROUND_JOB_UNAVAILABLE".into());
        }
    };
    let gate = child
        .stdin
        .take()
        .ok_or_else(|| "DEV_RUNTIME_FOREGROUND_OWNER_UNAVAILABLE".to_owned())
        .and_then(|mut input| {
            input
                .write_all(OWNER_GATE)
                .and_then(|()| input.flush())
                .map_err(|_| "DEV_RUNTIME_FOREGROUND_OWNER_UNAVAILABLE".to_owned())
        });
    let result = gate.and_then(|()| {
        let Some(budget) = budget else {
            return child
                .wait()
                .map(|exit| (child.id(), exit, false))
                .map_err(|_| "DEV_RUNTIME_CHILD_WAIT_UNAVAILABLE".to_owned());
        };
        let started = Instant::now();
        let mut stop = Some(stop);
        let mut stopping = None;
        loop {
            if let Some(exit) = child
                .try_wait()
                .map_err(|_| "DEV_RUNTIME_CHILD_WAIT_UNAVAILABLE")?
            {
                return Ok((child.id(), exit, stopping.is_some()));
            }
            if stopping.is_none() && started.elapsed() >= budget {
                stop.take().expect("deadline stop called once")()?;
                stopping = Some(Instant::now());
            }
            if stopping.is_some_and(|at| at.elapsed() >= Duration::from_secs(30)) {
                return Err("DEV_RUNTIME_FOREGROUND_STOP_DEADLINE".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    // Normal host exit must not leave descendants behind either. Retain the
    // Job until accounting proves the exact owned tree empty.
    let cleanup = job.terminate_and_wait_empty(Duration::from_secs(10));
    let _ = child.kill();
    let _ = child.wait();
    cleanup.map_err(|_| "DEV_RUNTIME_FOREGROUND_EXIT_UNPROVEN")?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path, thread, time::Instant};

    fn fixture_root() -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "catdesk-foreground-owner-{}-{}-{}",
            std::process::id(),
            crate::store::now(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        root
    }

    fn cleanup(root: &Path) {
        let resolved = root.canonicalize().unwrap();
        assert_eq!(
            resolved.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(
            resolved
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("catdesk-foreground-owner-")
        );
        fs::remove_dir_all(resolved).unwrap();
    }

    #[test]
    fn owner_gate_requires_exact_complete_input() {
        assert!(require_owner(OWNER_GATE).is_ok());
        for value in [
            b"".as_slice(),
            b"CATDESK_DEV_FOREGROUND_OWNER_V0\n".as_slice(),
            &OWNER_GATE[..OWNER_GATE.len() - 1],
            &[OWNER_GATE, b"extra"].concat(),
        ] {
            assert_eq!(
                require_owner(value).unwrap_err(),
                "DEV_RUNTIME_FOREGROUND_OWNER_UNAVAILABLE"
            );
        }
    }

    fn helper_command(root: &Path) -> Command {
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-File"])
            .arg(root.join("helper.ps1"))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    #[test]
    fn internal_deadline_requests_stop_once_without_external_controller() {
        let root = fixture_root();
        fs::write(root.join("helper.ps1"), concat!(
            "if ([Console]::In.ReadToEnd() -cne \"CATDESK_DEV_FOREGROUND_OWNER_V1`n\") { exit 42 }\n",
            "while (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'stop.txt'))) { Start-Sleep -Milliseconds 20 }\n",
            "exit 0\n"
        )).unwrap();
        let stopped = std::cell::Cell::new(0);
        let result = run_with_deadline(
            &mut helper_command(&root),
            Some(Duration::from_millis(100)),
            || {
                stopped.set(stopped.get() + 1);
                fs::write(root.join("stop.txt"), b"stop")
                    .map_err(|_| "FIXTURE_STOP_WRITE_FAILED".to_owned())
            },
        );
        cleanup(&root);
        let (_, exit, deadline_reached) = result.unwrap();
        assert!(exit.success());
        assert!(deadline_reached);
        assert_eq!(stopped.get(), 1);
    }

    #[test]
    fn normal_child_exit_drains_descendants_and_preserves_exit_code() {
        let root = fixture_root();
        fs::write(root.join("helper.ps1"), concat!(
            "$ErrorActionPreference='Stop'\n",
            "if ([Console]::In.ReadToEnd() -cne \"CATDESK_DEV_FOREGROUND_OWNER_V1`n\") { exit 42 }\n",
            "$child = Start-Process powershell.exe -WindowStyle Hidden -PassThru ",
            "-ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 60'\n",
            "[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'pids.txt'), \"$($child.Id)\")\n",
            "while (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'release.txt'))) { Start-Sleep -Milliseconds 50 }\n",
            "exit 7\n"
        )).unwrap();
        let worker_root = root.clone();
        let worker = thread::spawn(move || run(&mut helper_command(&worker_root)));
        let deadline = Instant::now() + Duration::from_secs(15);
        let descendant = loop {
            if let Ok(text) = fs::read_to_string(root.join("pids.txt"))
                && let Ok(pid) = text.parse::<u32>()
            {
                break Process::open(pid);
            }
            if Instant::now() >= deadline || worker.is_finished() {
                break None;
            }
            thread::sleep(Duration::from_millis(50));
        };
        let was_running = descendant.as_ref().is_some_and(Process::is_running);
        fs::write(root.join("release.txt"), b"release").unwrap();
        let result = worker.join().unwrap();
        let descendant = descendant.expect("gated descendant started");
        let exited = descendant.wait();
        drop(descendant);
        cleanup(&root);
        let (_, exit) = result.unwrap();
        assert_eq!(exit.code(), Some(7));
        assert!(
            was_running,
            "descendant must be live before normal parent exit"
        );
        assert!(exited, "normal host exit left a descendant running");
    }

    // Invoked only by the parent-death regression. This process, rather than
    // the outer test, owns the production foreground launcher and its Job.
    #[test]
    fn owner_process_fixture() {
        let Some(root) = std::env::var_os("CATDESK_FOREGROUND_TEST_ROOT") else {
            return;
        };
        run(&mut helper_command(Path::new(&root))).unwrap();
    }

    struct Process(*mut std::ffi::c_void);
    impl Process {
        fn open(pid: u32) -> Option<Self> {
            let handle = unsafe { OpenProcess(0x0010_0001, 0, pid) };
            (!handle.is_null()).then_some(Self(handle))
        }
        fn wait(&self) -> bool {
            unsafe { WaitForSingleObject(self.0, 10_000) == 0 }
        }
        fn is_running(&self) -> bool {
            unsafe { WaitForSingleObject(self.0, 0) == 258 }
        }
    }
    impl Drop for Process {
        fn drop(&mut self) {
            unsafe {
                // Failure-path cleanup also uses the retained exact handle;
                // never find/kill a later process by its numeric PID.
                if WaitForSingleObject(self.0, 0) != 0 {
                    TerminateProcess(self.0, 1);
                    WaitForSingleObject(self.0, 10_000);
                }
                CloseHandle(self.0);
            }
        }
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
        fn TerminateProcess(handle: *mut std::ffi::c_void, code: u32) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }

    struct Owner(std::process::Child);
    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn caller_death_kills_owned_helper_and_descendant_without_runtime_start() {
        let root = fixture_root();
        fs::write(
            root.join("helper.ps1"),
            concat!(
                "$ErrorActionPreference='Stop'\n",
                "if ([Console]::In.ReadToEnd() -cne \"CATDESK_DEV_FOREGROUND_OWNER_V1`n\") { exit 42 }\n",
                "$child = Start-Process powershell.exe -WindowStyle Hidden -PassThru ",
                "-ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 60'\n",
                "[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'pids.txt'), \"$PID,$($child.Id)\")\n",
                "Start-Sleep -Seconds 60\n"
            ),
        )
        .unwrap();
        let mut owner = Owner(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "development_runtime::foreground::tests::owner_process_fixture",
                    "--nocapture",
                ])
                .env("CATDESK_FOREGROUND_TEST_ROOT", &root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x0800_0000)
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        let pids = loop {
            if let Ok(text) = fs::read_to_string(root.join("pids.txt")) {
                let pids: Vec<_> = text
                    .split(',')
                    .filter_map(|p| p.parse::<u32>().ok())
                    .collect();
                if pids.len() == 2 {
                    break Some(pids);
                }
            }
            if Instant::now() >= deadline || owner.0.try_wait().unwrap().is_some() {
                break None;
            }
            thread::sleep(Duration::from_millis(50));
        };
        let processes = pids.and_then(|pids| {
            pids.into_iter()
                .map(Process::open)
                .collect::<Option<Vec<_>>>()
        });
        let were_running = processes
            .as_ref()
            .is_some_and(|rows| rows.iter().all(Process::is_running));
        owner.0.kill().unwrap();
        owner.0.wait().unwrap();
        let processes = processes.expect("gated helper and descendant started");
        let exited = processes.iter().all(Process::wait);
        drop(processes);
        cleanup(&root);
        assert!(
            were_running,
            "helper and descendant must be live before caller death"
        );
        assert!(exited, "caller loss left an owned descendant running");
    }
}
