//! Process ownership for the optional reviewed-candidate parent launcher.
//!
//! A suspended child is assigned to a private Windows kill-on-close job before
//! any candidate code runs. An ordinary candidate remains owned until exit.
//! A reviewed reconciliation may durably prepare a persistent handoff, then
//! disarm and release the job while retaining explicit failure cleanup until
//! the final handle close.

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
    const JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION: i32 = 1;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct BasicLimits {
        process_time: i64,
        job_time: i64,
        flags: u32,
        min_working_set: usize,
        max_working_set: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct IoCounters {
        read_operations: u64,
        write_operations: u64,
        other_operations: u64,
        read_bytes: u64,
        write_bytes: u64,
        other_bytes: u64,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io: IoCounters,
        process_memory: usize,
        job_memory: usize,
        peak_process_memory: usize,
        peak_job_memory: usize,
    }

    #[repr(C)]
    #[derive(Default)]
    struct BasicAccounting {
        total_user_time: i64,
        total_kernel_time: i64,
        period_user_time: i64,
        period_kernel_time: i64,
        total_page_fault_count: u32,
        total_processes: u32,
        active_processes: u32,
        total_terminated_processes: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> *mut c_void;
        fn OpenJobObjectW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
        fn GetLastError() -> u32;
        fn SetInformationJobObject(
            job: *mut c_void,
            class: i32,
            information: *mut c_void,
            length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
        fn QueryInformationJobObject(
            job: *mut c_void,
            class: i32,
            information: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
        fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
        fn GetCurrentProcess() -> *mut c_void;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn GetProcessTimes(
            process: *mut c_void,
            created: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
        fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
        fn GetHandleInformation(handle: *mut c_void, flags: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtResumeProcess(process: *mut c_void) -> i32;
    }

    struct KillOnCloseJob(*mut c_void);

    impl KillOnCloseJob {
        fn create() -> Result<Self, String> {
            Self::create_named(None)
        }

        fn create_named(name: Option<&str>) -> Result<Self, String> {
            let wide = name.map(|name| name.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
            let handle = unsafe {
                CreateJobObjectW(
                    std::ptr::null(),
                    wide.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
                )
            };
            if handle.is_null() {
                return Err("reviewed candidate owner job unavailable".into());
            }
            if name.is_some() && unsafe { GetLastError() } == 183 {
                unsafe { CloseHandle(handle) };
                return Err("reviewed stopped Job name is already owned".into());
            }
            let mut limits = ExtendedLimits::default();
            limits.basic.flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = unsafe {
                SetInformationJobObject(
                    handle,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    (&mut limits as *mut ExtendedLimits).cast(),
                    std::mem::size_of::<ExtendedLimits>() as u32,
                )
            };
            if configured == 0 {
                unsafe { CloseHandle(handle) };
                return Err("reviewed candidate owner job unavailable".into());
            }
            Ok(Self(handle))
        }

        fn assign(&self, child: &Child) -> Result<(), String> {
            if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle().cast()) } == 0 {
                return Err("reviewed candidate child job assignment failed".into());
            }
            Ok(())
        }

        fn terminate(&self) -> Result<(), String> {
            if self.0.is_null() || unsafe { TerminateJobObject(self.0, 1) } == 0 {
                return Err("reviewed candidate child job termination failed".into());
            }
            Ok(())
        }

        fn active_processes(&self) -> Result<u32, String> {
            let mut accounting = BasicAccounting::default();
            if self.0.is_null()
                || unsafe {
                    QueryInformationJobObject(
                        self.0,
                        JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION,
                        (&mut accounting as *mut BasicAccounting).cast(),
                        std::mem::size_of::<BasicAccounting>() as u32,
                        std::ptr::null_mut(),
                    )
                } == 0
            {
                return Err("reviewed candidate Job accounting unavailable".into());
            }
            Ok(accounting.active_processes)
        }

        fn contains(&self, child: &Child) -> Result<(), String> {
            let mut member = 0;
            if self.0.is_null()
                || unsafe { IsProcessInJob(child.as_raw_handle().cast(), self.0, &mut member) } == 0
                || member != 1
            {
                return Err("reviewed candidate child Job membership unavailable".into());
            }
            Ok(())
        }

        fn disarm(&self) -> Result<(), String> {
            let mut limits = ExtendedLimits::default();
            if self.0.is_null()
                || unsafe {
                    QueryInformationJobObject(
                        self.0,
                        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                        (&mut limits as *mut ExtendedLimits).cast(),
                        std::mem::size_of::<ExtendedLimits>() as u32,
                        std::ptr::null_mut(),
                    )
                } == 0
                || limits.basic.flags != JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            {
                return Err("reviewed candidate Job limits changed".into());
            }
            limits.basic.flags = 0;
            if unsafe {
                SetInformationJobObject(
                    self.0,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    (&mut limits as *mut ExtendedLimits).cast(),
                    std::mem::size_of::<ExtendedLimits>() as u32,
                )
            } == 0
            {
                return Err("reviewed candidate Job release failed".into());
            }
            let mut observed = ExtendedLimits::default();
            if unsafe {
                QueryInformationJobObject(
                    self.0,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    (&mut observed as *mut ExtendedLimits).cast(),
                    std::mem::size_of::<ExtendedLimits>() as u32,
                    std::ptr::null_mut(),
                )
            } == 0
                || observed.basic.flags != 0
            {
                return Err("reviewed candidate Job release is unproven".into());
            }
            Ok(())
        }

        fn require_armed(&self) -> Result<(), String> {
            let mut limits = ExtendedLimits::default();
            if self.0.is_null()
                || unsafe {
                    QueryInformationJobObject(
                        self.0,
                        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                        (&mut limits as *mut ExtendedLimits).cast(),
                        std::mem::size_of::<ExtendedLimits>() as u32,
                        std::ptr::null_mut(),
                    )
                } == 0
                || limits.basic.flags != JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            {
                return Err("reviewed stopped Job limits are unproven".into());
            }
            Ok(())
        }

        fn require_disarmed(&self) -> Result<(), String> {
            let mut observed = ExtendedLimits::default();
            if self.0.is_null()
                || unsafe {
                    QueryInformationJobObject(
                        self.0,
                        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                        (&mut observed as *mut ExtendedLimits).cast(),
                        std::mem::size_of::<ExtendedLimits>() as u32,
                        std::ptr::null_mut(),
                    )
                } == 0
                || observed.basic.flags != 0
            {
                return Err("reviewed candidate Job release is unproven".into());
            }
            Ok(())
        }

        fn close(&mut self) -> Result<(), String> {
            if self.0.is_null() {
                return Ok(());
            }
            if unsafe { CloseHandle(self.0) } == 0 {
                return Err("reviewed candidate child job close failed".into());
            }
            self.0 = std::ptr::null_mut();
            Ok(())
        }
    }

    impl Drop for KillOnCloseJob {
        fn drop(&mut self) {
            let _ = self.close();
        }
    }

    pub(crate) enum SpawnFailure {
        /// Command::spawn returned an error: no Child was handed to the owner.
        NoChildCreated,
        /// Ownership/setup failure is not authority to launch another image.
        Owner(String),
        /// A child may have run and complete Job exit was not proved. Losing
        /// the returned owner is never permission to restore mapped files.
        UnprovenOwner(String),
    }

    impl SpawnFailure {
        pub(crate) fn message(self) -> String {
            match self {
                Self::NoChildCreated => "reviewed candidate child spawn failed".into(),
                Self::Owner(message) | Self::UnprovenOwner(message) => message,
            }
        }

        pub(crate) fn quiescence_proven(&self) -> bool {
            !matches!(self, Self::UnprovenOwner(_))
        }
    }

    pub(crate) struct OwnedCandidateChild {
        child: Option<Child>,
        // Initially kill-on-close; after disarm, explicit termination remains
        // available until the final release closes this handle.
        job: KillOnCloseJob,
        disarmed: bool,
        released: bool,
    }

    #[derive(Clone)]
    pub(crate) struct StoppedControllerLifetime {
        pub(crate) transaction_id: String,
        pub(crate) job_name: String,
        pub(crate) session_id: u32,
        pub(crate) pid: u32,
        pub(crate) creation_time: u64,
        workspace: std::path::PathBuf,
        entry_args: Vec<String>,
        kind: u8,
    }

    static STOPPED_CONTROLLER: std::sync::Mutex<Option<StoppedControllerLifetime>> =
        std::sync::Mutex::new(None);

    fn dedicated_stopped_entry(args: &[String]) -> bool {
        if args.len() == 9
            && args[0] == "operator"
            && args[1] == "release-pair"
            && args[2] == "prepare-stopped"
            && args[3] == "--review-record-id"
            && !args[4].is_empty()
            && args[4].len() <= 128
            && args[5] == "--build-path"
            && !args[6].is_empty()
            && args[6].len() <= 4096
            && args[7] == "--expected-sha256"
            && args[8].len() == 64
            && args[8].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return true;
        }
        args.len() == 5
            && args[0] == "operator"
            && args[1] == "release-pair"
            && ((args[2] == "confirm-stopped" && args[3] == "--confirmation-token")
                || (matches!(args[2].as_str(), "resume-stopped" | "rollback-stopped")
                    && args[3] == "--transaction-id"))
            && args[4].len() == 32
            && args[4].bytes().all(|b| b.is_ascii_hexdigit())
    }

    fn dedicated_installed_backend_entry(args: &[String]) -> bool {
        let unscoped = args.len() == 5
            && args[0] == "operator"
            && args[1] == "installed-backend"
            && args[2] == "run"
            && args[3] == "--max-runtime-seconds"
            && args[4]
                .parse::<u64>()
                .is_ok_and(|value| (1..=180).contains(&value));
        let project = args.len() == 13
            && args[0] == "operator"
            && args[1] == "installed-backend"
            && args[2] == "run-project"
            && args[3] == "--control-workspace"
            && !args[4].is_empty()
            && args[4].len() <= 4096
            && std::path::Path::new(&args[4]).is_absolute()
            && args[5] == "--project-id"
            && catdesk_wake::protocol::valid_id(&args[6])
            && args[7] == "--project-target-generation"
            && args[8].parse::<u64>().is_ok_and(|value| value > 0)
            && args[9] == "--project-target-sha256"
            && args[10].len() == 64
            && args[10]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && args[11] == "--max-runtime-seconds"
            && args[12]
                .parse::<u64>()
                .is_ok_and(|value| (1..=180).contains(&value));
        let central = args.len() == 9
            && args[0] == "operator"
            && args[1] == "installed-backend"
            && args[2] == "run-central"
            && args[3] == "--control-workspace"
            && args[4].len() <= 4096
            && std::path::Path::new(&args[4]).is_absolute()
            && args[5] == "--mcp-path"
            && args[6].len() <= 128
            && crate::validate_headless_mcp_path(&args[6]).is_ok()
            && args[7] == "--max-runtime-seconds"
            && args[8]
                .parse::<u64>()
                .is_ok_and(|seconds| (1..=180).contains(&seconds));
        unscoped || project || central
    }

    fn dedicated_task_entry(args: &[String]) -> bool {
        crate::parse_headless_mcp_options(args.iter().cloned())
            .ok()
            .flatten()
            .is_some_and(|options| {
                options.project_task_contract.is_some()
                    && options.project_scope.is_some()
                    && options.no_web_auto
                    && options
                        .maximum_runtime_seconds
                        .is_some_and(|seconds| (1..=180).contains(&seconds))
            })
    }

    impl OwnedCandidateChild {
        /// A dedicated stopped CLI retains exactly one noninheritable armed
        /// Job handle until OS process teardown. Children inherit membership
        /// at creation, including the gap before their narrower assignment.
        pub(crate) fn enter_stopped_controller_lifetime(
            transaction: &str,
            workspace: &std::path::Path,
        ) -> Result<StoppedControllerLifetime, String> {
            Self::enter_controller_lifetime(transaction, workspace, 0)
        }

        pub(crate) fn enter_installed_backend_lifetime(
            transaction: &str,
            workspace: &std::path::Path,
        ) -> Result<StoppedControllerLifetime, String> {
            Self::enter_controller_lifetime(transaction, workspace, 1)
        }

        pub(crate) fn enter_project_task_lifetime(
            workspace: &std::path::Path,
        ) -> Result<StoppedControllerLifetime, String> {
            let transaction = uuid::Uuid::new_v4().simple().to_string();
            #[cfg(test)]
            let transaction = std::env::var("CATDESK_TASK_OWNER_FIXTURE").unwrap_or(transaction);
            Self::enter_controller_lifetime(&transaction, workspace, 2)
        }

        pub(crate) fn current_project_task_lifetime()
        -> Result<Option<StoppedControllerLifetime>, String> {
            let current = STOPPED_CONTROLLER
                .lock()
                .map_err(|_| "task owner state unavailable")?;
            let Some(proof) = current.as_ref().filter(|proof| proof.kind == 2) else {
                return Ok(None);
            };
            let args = std::env::args().skip(1).collect::<Vec<_>>();
            let dedicated = dedicated_task_entry(&args);
            #[cfg(test)]
            let dedicated = dedicated
                || (args.len() == 4
                    && args[0] == "--exact"
                    && args[1] == "project_mcp_task::recovery::tests::task_owner_fixture_child"
                    && args[2] == "--ignored"
                    && args[3] == "--nocapture"
                    && std::env::var("CATDESK_TASK_OWNER_FIXTURE").ok().as_deref()
                        == Some(&proof.transaction_id));
            if !dedicated || proof.entry_args != args || proof.pid != std::process::id() {
                return Err("task owner entry changed".into());
            }
            let wide = proof
                .job_name
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let handle = unsafe { OpenJobObjectW(4, 0, wide.as_ptr()) };
            if handle.is_null() {
                return Err("task owner Job unavailable".into());
            }
            let job = KillOnCloseJob(handle);
            job.require_armed()?;
            let mut inside = 0;
            if unsafe { IsProcessInJob(GetCurrentProcess(), job.0, &mut inside) } == 0
                || inside != 1
            {
                return Err("task owner Job membership changed".into());
            }
            Ok(Some(proof.clone()))
        }

        pub(crate) fn prove_project_task_owner_exited(
            id: &str,
            job_name: &str,
            session: u32,
            pid: u32,
            creation: u64,
        ) -> Result<(), String> {
            let mut current_session = 0;
            if id.len() != 32
                || !id.bytes().all(|b| b.is_ascii_hexdigit())
                || pid == 0
                || creation == 0
                || unsafe { ProcessIdToSessionId(std::process::id(), &mut current_session) } == 0
                || session != current_session
                || job_name
                    != format!(
                        "Local\\CatDesk.ProjectTask.{id}.controller.{session}.{pid}.{creation}"
                    )
            {
                return Err("task owner identity unproven".into());
            }
            let process = unsafe { OpenProcess(0x0010_1000, 0, pid) };
            if process.is_null() {
                if unsafe { GetLastError() } != 87 {
                    return Err("task owner process unavailable".into());
                }
            } else {
                let mut created = FileTime::default();
                let mut exited = FileTime::default();
                let mut kernel = FileTime::default();
                let mut user = FileTime::default();
                let queried = unsafe {
                    GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user)
                };
                let state = unsafe { WaitForSingleObject(process, 0) };
                unsafe { CloseHandle(process) };
                if queried == 0
                    || ((u64::from(created.high) << 32) | u64::from(created.low)) != creation
                    || state != 0
                {
                    return Err("task owner remains live or identity is ambiguous".into());
                }
            }
            let wide = job_name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let handle = unsafe { OpenJobObjectW(4, 0, wide.as_ptr()) };
            if handle.is_null() {
                return if unsafe { GetLastError() } == 2 {
                    Ok(())
                } else {
                    Err("task Job observation unavailable".into())
                };
            }
            let job = KillOnCloseJob(handle);
            job.require_armed()?;
            if job.active_processes()? != 0 {
                return Err("task Job remains active".into());
            }
            Ok(())
        }

        fn enter_controller_lifetime(
            transaction: &str,
            workspace: &std::path::Path,
            kind: u8,
        ) -> Result<StoppedControllerLifetime, String> {
            if transaction.len() != 32 || !transaction.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("stopped controller transaction is invalid".into());
            }
            let args = std::env::args().skip(1).collect::<Vec<_>>();
            let dedicated = if kind == 2 {
                dedicated_task_entry(&args)
            } else if kind == 1 {
                dedicated_installed_backend_entry(&args)
            } else {
                dedicated_stopped_entry(&args)
            };
            #[cfg(test)]
            let dedicated = dedicated
                || (args.len() == 4
                    && args[0] == "--exact"
                    && args[2] == "--ignored"
                    && args[3] == "--nocapture"
                    && args[1]
                        == if kind == 2 {
                            "project_mcp_task::recovery::tests::task_owner_fixture_child"
                        } else if kind == 1 {
                            "reviewed_candidate_owner::windows::tests::installed_controller_fixture_child"
                        } else {
                            "reviewed_candidate_owner::windows::tests::stopped_controller_fixture_child"
                        }
                    && std::env::var(if kind == 2 {
                        "CATDESK_TASK_OWNER_FIXTURE"
                    } else if kind == 1 {
                        "CATDESK_INSTALLED_CONTROLLER_FIXTURE"
                    } else {
                        "CATDESK_STOPPED_CONTROLLER_FIXTURE"
                    })
                    .ok()
                    .as_deref()
                        == Some(transaction));
            if !dedicated {
                return Err("stopped controller lifetime is limited to a dedicated CLI".into());
            }
            let workspace = workspace
                .canonicalize()
                .map_err(|_| "stopped controller workspace identity unavailable")?;
            let mut current = STOPPED_CONTROLLER
                .lock()
                .map_err(|_| "stopped controller entry unavailable")?;
            if let Some(proof) = current.as_ref() {
                let mut session = 0;
                if unsafe { ProcessIdToSessionId(std::process::id(), &mut session) } == 0 {
                    return Err("stopped controller session unavailable".into());
                }
                return if proof.transaction_id == transaction
                    && proof.workspace == workspace
                    && proof.entry_args == args
                    && proof.pid == std::process::id()
                    && proof.session_id == session
                    && proof.kind == kind
                {
                    Ok(proof.clone())
                } else {
                    Err("stopped controller cannot change transactions".into())
                };
            }
            let (mut created, mut exited, mut kernel, mut user) = (
                FileTime::default(),
                FileTime::default(),
                FileTime::default(),
                FileTime::default(),
            );
            let mut session = 0;
            if unsafe {
                GetProcessTimes(
                    GetCurrentProcess(),
                    &mut created,
                    &mut exited,
                    &mut kernel,
                    &mut user,
                )
            } == 0
                || unsafe { ProcessIdToSessionId(std::process::id(), &mut session) } == 0
            {
                return Err("stopped controller process identity unavailable".into());
            }
            let created = (u64::from(created.high) << 32) | u64::from(created.low);
            let prefix = if kind == 2 {
                "Local\\CatDesk.ProjectTask"
            } else if kind == 1 {
                "Local\\CatDesk.InstalledBackend"
            } else {
                "Local\\CatDesk.ReleasePairStopped"
            };
            let name = format!(
                "{prefix}.{transaction}.controller.{session}.{}.{created}",
                std::process::id()
            );
            let job = KillOnCloseJob::create_named(Some(&name))?;
            job.require_armed()?;
            let mut flags = 0;
            if unsafe { SetHandleInformation(job.0, 1, 0) } == 0
                || unsafe { GetHandleInformation(job.0, &mut flags) } == 0
                || flags & 1 != 0
            {
                return Err("stopped controller Job handle inheritance is unproven".into());
            }
            if unsafe { AssignProcessToJobObject(job.0, GetCurrentProcess()) } == 0 {
                return Err("stopped controller lifetime Job assignment failed".into());
            }
            std::mem::forget(job);
            let proof = StoppedControllerLifetime {
                transaction_id: transaction.into(),
                job_name: name,
                session_id: session,
                pid: std::process::id(),
                creation_time: created,
                workspace,
                entry_args: args,
                kind,
            };
            *current = Some(proof.clone());
            Ok(proof)
        }
        pub(crate) fn spawn(command: &mut Command) -> Result<Self, String> {
            Self::spawn_classified(command).map_err(SpawnFailure::message)
        }

        pub(crate) fn spawn_classified(command: &mut Command) -> Result<Self, SpawnFailure> {
            Self::spawn_with_resume(command, |child| {
                if unsafe { NtResumeProcess(child.as_raw_handle().cast()) } < 0 {
                    Err("reviewed candidate child resume failed".into())
                } else {
                    Ok(())
                }
            })
        }

        fn spawn_with_resume(
            command: &mut Command,
            resume: impl FnOnce(&mut Child) -> Result<(), String>,
        ) -> Result<Self, SpawnFailure> {
            Self::spawn_with_job(command, KillOnCloseJob::create(), resume)
        }

        fn spawn_with_job(
            command: &mut Command,
            job: Result<KillOnCloseJob, String>,
            resume: impl FnOnce(&mut Child) -> Result<(), String>,
        ) -> Result<Self, SpawnFailure> {
            let job = job.map_err(SpawnFailure::Owner)?;
            // Keep console Ctrl+C on the parent so its bounded stdin stop
            // controls the child's graceful journal/root retirement.
            command.creation_flags(CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
            let child = command.spawn().map_err(|_| SpawnFailure::NoChildCreated)?;
            let mut owner = Self {
                child: Some(child),
                job,
                disarmed: false,
                released: false,
            };
            if let Err(error) = owner.job.assign(owner.child.as_ref().unwrap()) {
                // Candidate user code is still suspended, so no descendant
                // can have been created. Prove the exact root and empty Job.
                let _ = owner.child_mut().kill();
                let root_exited = owner
                    .wait_until(Instant::now() + Duration::from_secs(10))
                    .unwrap_or(false);
                if root_exited {
                    drop(owner.child.take());
                }
                return Err(
                    if root_exited && owner.job.active_processes().ok() == Some(0) {
                        SpawnFailure::Owner(error)
                    } else {
                        SpawnFailure::UnprovenOwner(error)
                    },
                );
            }
            // Verify membership before any candidate code is resumed.
            if let Err(error) = owner.job.contains(owner.child.as_ref().unwrap()) {
                return Err(owner.failed_startup(error));
            }
            if let Err(error) = resume(owner.child_mut()) {
                // A resume failure may be partial. Retain the Job until the
                // existing whole-tree drain proves zero active processes.
                return Err(owner.failed_startup(error));
            }
            Ok(owner)
        }

        /// A bounded stopped transaction keeps its armed Job named so a later
        /// same-transaction recovery can independently observe whole-tree exit.
        pub(crate) fn spawn_stopped_named(
            command: &mut Command,
            name: &str,
        ) -> Result<Self, String> {
            validate_stopped_job_name(name)?;
            if name.contains(".controller.") {
                return Err("controller lifetime Job cannot be used for child spawn".into());
            }
            Self::spawn_with_job(command, KillOnCloseJob::create_named(Some(name)), |child| {
                if unsafe { NtResumeProcess(child.as_raw_handle().cast()) } < 0 {
                    Err("reviewed candidate child resume failed".into())
                } else {
                    Ok(())
                }
            })
            .map_err(SpawnFailure::message)
        }

        pub(crate) fn prove_stopped_named_job_quiescent(name: &str) -> Result<(), String> {
            validate_stopped_job_name(name)?;
            let wide = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let handle = unsafe { OpenJobObjectW(4, 0, wide.as_ptr()) };
            if handle.is_null() {
                // A Job object remains alive while associated processes remain.
                // Only ERROR_FILE_NOT_FOUND proves that this exact armed Job
                // has ceased to exist; access/API failures are not quiescence.
                return if unsafe { GetLastError() } == 2 {
                    Ok(())
                } else {
                    Err("reviewed stopped Job observation unavailable".into())
                };
            }
            let job = KillOnCloseJob(handle);
            job.require_armed()?;
            if job.active_processes()? != 0 {
                return Err("reviewed stopped Job still has active processes".into());
            }
            Ok(())
        }

        fn failed_startup(mut self, error: String) -> SpawnFailure {
            if self.terminate_tree_and_confirm_quiescent().is_ok() {
                SpawnFailure::Owner(error)
            } else {
                SpawnFailure::UnprovenOwner(error)
            }
        }

        /// A reconciliation worker must not inherit a second, unrelated Job:
        /// releasing this private Job cannot preserve a child owned elsewhere.
        pub(crate) fn require_uncontained_owner() -> Result<(), String> {
            let mut in_job = 0;
            if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) }
                == 0
                || in_job != 0
            {
                return Err("reviewed candidate owner has an outer Job".into());
            }
            Ok(())
        }

        pub(crate) fn pid(&self) -> u32 {
            self.child.as_ref().expect("owned child exists").id()
        }

        pub(crate) fn child_mut(&mut self) -> &mut Child {
            self.child.as_mut().expect("owned child exists")
        }

        /// Terminate the complete owned Job and prove its active process count
        /// reached zero before a caller restores any mapped release bytes.
        pub(crate) fn terminate_tree_and_confirm_quiescent(&mut self) -> Result<(), String> {
            if self.released || self.job.0.is_null() {
                return Err("reviewed candidate Job is no longer owned".into());
            }
            let termination = self.job.terminate();
            let deadline = Instant::now() + Duration::from_secs(10);
            if !self.wait_until(deadline)? {
                return Err("reviewed candidate child exit is unproven".into());
            }
            // Windows may retain a terminated process in Job accounting while
            // our Child still holds its process handle. Close that exact handle
            // before requiring zero active processes for the complete tree.
            drop(self.child.take());
            loop {
                if self.job.active_processes()? == 0 {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("reviewed candidate Job exit is unproven".into());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            // If TerminateJobObject itself failed but exact accounting proves
            // the tree has exited, there is no live process to restore under.
            let _ = termination;
            self.job.close()?;
            self.released = true;
            Ok(())
        }

        /// Clear kill-on-close only after the caller has durably recorded an
        /// exact pending serving handoff. Keep the Job handle for a final
        /// readback and explicit whole-tree termination on failure.
        pub(crate) fn disarm_live_retaining_job(&mut self) -> Result<(), String> {
            if self.released || self.disarmed || self.job.0.is_null() {
                return Err("reviewed candidate Job is no longer owned".into());
            }
            if self
                .child_mut()
                .try_wait()
                .map_err(|_| "reviewed candidate state unavailable")?
                .is_some()
            {
                return Err("reviewed candidate exited before Job release".into());
            }
            self.job
                .contains(self.child.as_ref().expect("owned child exists"))?;
            self.job.disarm()?;
            self.disarmed = true;
            Ok(())
        }

        /// Close the disarmed Job after a caller readback. The caller must
        /// independently prove the exact daemon is still serving after this
        /// close before recording final reconciliation success.
        pub(crate) fn release_live(&mut self) -> Result<(), String> {
            if self.released || !self.disarmed || self.job.0.is_null() {
                return Err("reviewed candidate Job release is unavailable".into());
            }
            if self
                .child_mut()
                .try_wait()
                .map_err(|_| "reviewed candidate state unavailable")?
                .is_some()
            {
                return Err("reviewed candidate exited before Job release".into());
            }
            self.job
                .contains(self.child.as_ref().expect("owned child exists"))?;
            self.job.require_disarmed()?;
            // Keep explicit termination authority until this close succeeds.
            self.job.close()?;
            self.released = true;
            Ok(())
        }

        fn wait_until(&mut self, deadline: Instant) -> Result<bool, String> {
            let Some(child) = self.child.as_mut() else {
                return Ok(true);
            };
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return Ok(true),
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Ok(None) => return Ok(false),
                    Err(_) => return Err("reviewed candidate child status unavailable".into()),
                }
            }
        }

        fn shutdown_exact(&mut self) -> Result<(), String> {
            // Terminate explicitly because a failed release may already have
            // cleared kill-on-close. Close remains the crash-safe fallback when
            // the Job is still armed.
            let termination = self.job.terminate();
            let job_closed = if self.job.close().is_ok() {
                true
            } else {
                // If CloseHandle failed while the job handle remains valid,
                // explicitly terminate its whole process tree and retry close.
                let _ = self.job.terminate();
                self.job.close().is_ok()
            };
            if !job_closed {
                if let Some(child) = self.child.as_mut() {
                    let _ = child.kill();
                }
            }
            if !self.wait_until(Instant::now() + Duration::from_secs(5))? {
                // Child::kill uses the retained process handle, never a PID
                // lookup. Reap it with a second bound after the fallback.
                if let Some(child) = self.child.as_mut() {
                    let _ = child.kill();
                }
                if !self.wait_until(Instant::now() + Duration::from_secs(2))? {
                    return Err("reviewed candidate child termination timed out".into());
                }
            }
            if !job_closed {
                return Err("reviewed candidate child job close failed".into());
            }
            if termination.is_err() {
                return Err("reviewed candidate child tree termination unproven".into());
            }
            self.released = true;
            Ok(())
        }

        pub(crate) fn terminate_and_wait(mut self) -> Result<(), String> {
            self.shutdown_exact()
        }
    }

    fn validate_stopped_job_name(name: &str) -> Result<(), String> {
        let value = name
            .strip_prefix("Local\\CatDesk.ReleasePairStopped.")
            .ok_or("reviewed stopped Job name is invalid")?;
        let (transaction, role) = value
            .split_once('.')
            .ok_or("reviewed stopped Job name is invalid")?;
        if transaction.len() != 32
            || !transaction.bytes().all(|b| b.is_ascii_hexdigit())
            || !(matches!(role, "candidate" | "helper") || {
                let fields = role.split('.').collect::<Vec<_>>();
                fields.len() == 4
                    && fields[0] == "controller"
                    && fields[1].parse::<u32>().is_ok()
                    && fields[2].parse::<u32>().is_ok_and(|pid| pid > 0)
                    && fields[3].parse::<u64>().is_ok_and(|time| time > 0)
            })
        {
            return Err("reviewed stopped Job name is invalid".into());
        }
        Ok(())
    }

    impl Drop for OwnedCandidateChild {
        fn drop(&mut self) {
            if self.released {
                return;
            }
            if let Err(error) = self.shutdown_exact() {
                eprintln!("reviewed candidate owner cleanup failed: {error}");
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::OwnedCandidateChild;
        use std::io::{BufRead, BufReader, Write};
        use std::os::windows::io::AsRawHandle;
        use std::process::{Command, Stdio};
        use std::time::Duration;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn WaitForSingleObject(handle: *mut std::ffi::c_void, timeout_ms: u32) -> u32;
            fn TerminateProcess(handle: *mut std::ffi::c_void, exit_code: u32) -> i32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }

        #[test]
        fn classified_spawn_rejection_preserves_legacy_error_without_child() {
            let missing =
                std::env::temp_dir().join(format!("catdesk-missing-{}.exe", uuid::Uuid::new_v4()));
            assert!(!missing.exists());
            assert!(matches!(
                OwnedCandidateChild::spawn_classified(&mut Command::new(&missing)),
                Err(super::SpawnFailure::NoChildCreated)
            ));
            assert_eq!(
                OwnedCandidateChild::spawn(&mut Command::new(&missing))
                    .err()
                    .unwrap(),
                "reviewed candidate child spawn failed"
            );
        }

        #[test]
        fn partial_resume_failure_drains_real_descendant_before_proving_quiescence() {
            use super::SpawnFailure;
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::child_with_descendant",
                    "--nocapture",
                ])
                .env("CATDESK_OWNER_JOB_TEST_DESCENDANT", "1")
                .stdout(Stdio::piped());
            let mut descendant_wait = None;
            let failure = OwnedCandidateChild::spawn_with_resume(&mut command, |child| {
                if unsafe { super::NtResumeProcess(child.as_raw_handle().cast()) } < 0 {
                    return Err("fixture resume failed".into());
                }
                let stdout = child.stdout.take().unwrap();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                        if let Some(pid) = line.split("DESCENDANT_PID=").nth(1) {
                            let _ = tx.send(pid.trim().parse::<u32>().unwrap());
                            break;
                        }
                    }
                });
                let pid = rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| "fixture descendant not observed")?;
                let handle = unsafe { OpenProcess(0x0010_0000, 0, pid) };
                if handle.is_null() {
                    return Err("fixture descendant handle unavailable".into());
                }
                let handle = handle as usize;
                descendant_wait = Some(std::thread::spawn(move || {
                    let handle = handle as *mut std::ffi::c_void;
                    let outcome = unsafe { WaitForSingleObject(handle, 10_000) };
                    unsafe { CloseHandle(handle) };
                    outcome
                }));
                // Exercise a failed native resume after code and a separately
                // named descendant have actually run inside the assigned Job.
                Err("fixture partial resume failure".into())
            })
            .err()
            .expect("injected startup failure");
            assert_eq!(descendant_wait.unwrap().join().unwrap(), 0);
            assert!(failure.quiescence_proven());
            assert!(matches!(failure, SpawnFailure::Owner(_)));
            assert!(!SpawnFailure::UnprovenOwner("unknown cleanup".into()).quiescence_proven());
        }

        #[test]
        fn suspended_child_is_owned_and_terminated_with_job() {
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::sleep_child",
                ])
                .env("CATDESK_OWNER_JOB_TEST_CHILD", "1")
                .stdout(Stdio::null());
            let mut owned = OwnedCandidateChild::spawn(&mut command).expect("owned child");
            assert!(owned.pid() > 0);
            assert!(owned.child_mut().try_wait().expect("child state").is_none());
            owned.terminate_and_wait().expect("terminate owned child");
        }

        #[test]
        fn sleep_child() {
            if std::env::var_os("CATDESK_OWNER_JOB_TEST_CHILD").is_some() {
                std::thread::sleep(Duration::from_secs(60));
            }
        }

        #[test]
        fn stopped_named_job_rejects_collision_and_proves_whole_tree_exit() {
            let name = format!(
                "Local\\CatDesk.ReleasePairStopped.{}.candidate",
                uuid::Uuid::new_v4().simple()
            );
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::child_with_descendant",
                    "--nocapture",
                ])
                .env("CATDESK_OWNER_JOB_TEST_DESCENDANT", "1")
                .stdout(Stdio::piped());
            let mut owner = OwnedCandidateChild::spawn_stopped_named(&mut command, &name).unwrap();
            let stdout = owner.child_mut().stdout.take().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(pid) = line
                        .strip_prefix("DESCENDANT_PID=")
                        .and_then(|s| s.parse::<u32>().ok())
                    {
                        let _ = tx.send(pid);
                        break;
                    }
                }
            });
            let descendant = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let handle = unsafe { OpenProcess(0x0010_0000, 0, descendant) };
            assert!(!handle.is_null());
            assert!(OwnedCandidateChild::prove_stopped_named_job_quiescent(&name).is_err());
            let mut other = Command::new(std::env::current_exe().unwrap());
            other
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::sleep_child",
                ])
                .env("CATDESK_OWNER_JOB_TEST_CHILD", "1")
                .stdout(Stdio::null());
            assert!(OwnedCandidateChild::spawn_stopped_named(&mut other, &name).is_err());
            owner.terminate_tree_and_confirm_quiescent().unwrap();
            assert_eq!(unsafe { WaitForSingleObject(handle, 5000) }, 0);
            unsafe { CloseHandle(handle) };
            reader.join().unwrap();
            OwnedCandidateChild::prove_stopped_named_job_quiescent(&name).unwrap();
        }

        #[test]
        fn stopped_named_job_names_have_no_arbitrary_namespace_authority() {
            for name in [
                "Global\\anything",
                "Local\\CatDesk.ReleasePairStopped.bad.candidate",
                "Local\\CatDesk.ReleasePairStopped.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.serving",
            ] {
                assert!(super::validate_stopped_job_name(name).is_err());
            }
        }

        #[test]
        fn stopped_controller_lifetime_rejects_shared_test_host() {
            assert!(
                OwnedCandidateChild::enter_stopped_controller_lifetime(
                    &uuid::Uuid::new_v4().simple().to_string(),
                    &std::env::current_dir().unwrap()
                )
                .is_err()
            );
        }

        #[test]
        fn installed_controller_lifetime_rejects_shared_host_and_open_arguments() {
            assert!(
                OwnedCandidateChild::enter_installed_backend_lifetime(
                    &uuid::Uuid::new_v4().simple().to_string(),
                    &std::env::current_dir().unwrap()
                )
                .is_err()
            );
            let good = [
                "operator",
                "installed-backend",
                "run",
                "--max-runtime-seconds",
                "180",
            ]
            .map(String::from);
            assert!(super::dedicated_installed_backend_entry(&good));
            for value in ["0", "181", "forever", "-1"] {
                let mut args = good.clone();
                args[4] = value.into();
                assert!(!super::dedicated_installed_backend_entry(&args));
            }
            let mut extra = good.to_vec();
            extra.push("--force".into());
            assert!(!super::dedicated_installed_backend_entry(&extra));
            let mut project = [
                "operator",
                "installed-backend",
                "run-project",
                "--control-workspace",
                "C:/control",
                "--project-id",
                "alpha",
                "--project-target-generation",
                "1",
                "--project-target-sha256",
                "",
                "--max-runtime-seconds",
                "60",
            ]
            .map(String::from);
            project[10] = "a".repeat(64);
            assert!(super::dedicated_installed_backend_entry(&project));
            for (index, value) in [
                (4, "relative"),
                (6, "../foreign"),
                (8, "0"),
                (10, "unknown"),
                (12, "181"),
            ] {
                let mut changed = project.clone();
                changed[index] = value.into();
                assert!(!super::dedicated_installed_backend_entry(&changed));
            }
            let mut extra = project.to_vec();
            extra.push("--project-task-contract".into());
            assert!(!super::dedicated_installed_backend_entry(&extra));
        }

        #[test]
        fn installed_central_lifetime_requires_closed_dedicated_entry() {
            let good = [
                "operator",
                "installed-backend",
                "run-central",
                "--control-workspace",
                "C:/approved-control",
                "--mcp-path",
                "/existing-route/mcp",
                "--max-runtime-seconds",
                "180",
            ]
            .map(String::from);
            assert!(super::dedicated_installed_backend_entry(&good));
            for (index, value) in [
                (0, "--headless-mcp"),
                (3, "--workspace"),
                (4, "relative"),
                (6, "https://other.invalid/mcp"),
                (6, "/one/mcp?other"),
                (7, "--no-auth"),
                (8, "0"),
                (8, "181"),
            ] {
                let mut changed = good.clone();
                changed[index] = value.into();
                assert!(!super::dedicated_installed_backend_entry(&changed));
            }
            let mut extra = good.to_vec();
            extra.push("--force".into());
            assert!(!super::dedicated_installed_backend_entry(&extra));
        }

        #[test]
        #[ignore = "Dedicated child-process fixture; invoked only by its bounded parent test"]
        fn stopped_controller_fixture_child() {
            controller_fixture_child(false);
        }

        #[test]
        #[ignore = "Dedicated child-process fixture; invoked only by its bounded parent test"]
        fn installed_controller_fixture_child() {
            controller_fixture_child(true);
        }

        fn controller_fixture_child(installed: bool) {
            use std::os::windows::process::CommandExt;
            let transaction = std::env::var(if installed {
                "CATDESK_INSTALLED_CONTROLLER_FIXTURE"
            } else {
                "CATDESK_STOPPED_CONTROLLER_FIXTURE"
            })
            .unwrap();
            let proof = if installed {
                OwnedCandidateChild::enter_installed_backend_lifetime(
                    &transaction,
                    &std::env::current_dir().unwrap(),
                )
            } else {
                OwnedCandidateChild::enter_stopped_controller_lifetime(
                    &transaction,
                    &std::env::current_dir().unwrap(),
                )
            }
            .unwrap();
            if std::env::var("CATDESK_STOPPED_CONTROLLER_FIXTURE_STAGE").as_deref() == Ok("normal")
            {
                println!(
                    "{}",
                    serde_json::json!({"stage":"NORMAL_EXIT","job":proof.job_name,"pid":proof.pid})
                );
                std::io::stdout().flush().unwrap();
                std::process::exit(0);
            }
            let mut runner = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::child_with_descendant",
                    "--nocapture",
                ])
                .env("CATDESK_OWNER_JOB_TEST_DESCENDANT", "1")
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let reader = BufReader::new(runner.stdout.take().unwrap());
            let mut descendant = None;
            for line in reader.lines().map_while(Result::ok) {
                if let Some(pid) = line
                    .strip_prefix("DESCENDANT_PID=")
                    .and_then(|s| s.parse::<u32>().ok())
                {
                    descendant = Some(pid);
                    break;
                }
            }
            let mut suspended = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::sleep_child",
                ])
                .env("CATDESK_OWNER_JOB_TEST_CHILD", "1")
                .creation_flags(super::CREATE_SUSPENDED | super::CREATE_NEW_PROCESS_GROUP)
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            // Deliberately stop at the real creation-before-private-assignment
            // boundary. Both children already inherit the controller Job.
            println!(
                "{}",
                serde_json::json!({"stage":"PREASSIGN","job":proof.job_name,
                "pid":proof.pid,"suspendedPid":suspended.id(),"runnerPid":runner.id(),"descendantPid":descendant.unwrap()})
            );
            std::io::stdout().flush().unwrap();
            std::thread::sleep(Duration::from_secs(30));
            let _ = suspended.kill();
            let _ = suspended.wait();
            let _ = runner.kill();
            let _ = runner.wait();
            std::process::exit(2);
        }

        #[test]
        fn stopped_controller_lifetime_covers_normal_exit_and_preassignment_crash() {
            for installed in [false, true] {
                for stage in ["normal", "preassign"] {
                    let transaction = uuid::Uuid::new_v4().simple().to_string();
                    let mut command = Command::new(std::env::current_exe().unwrap());
                    command.args(["--exact",if installed {"reviewed_candidate_owner::windows::tests::installed_controller_fixture_child"}else{"reviewed_candidate_owner::windows::tests::stopped_controller_fixture_child"},"--ignored","--nocapture"])
                    .env(if installed {"CATDESK_INSTALLED_CONTROLLER_FIXTURE"}else{"CATDESK_STOPPED_CONTROLLER_FIXTURE"},&transaction)
                    .env("CATDESK_STOPPED_CONTROLLER_FIXTURE_STAGE",stage).stdout(Stdio::piped()).stdin(Stdio::piped());
                    let mut owner = OwnedCandidateChild::spawn(&mut command).unwrap();
                    let stdout = owner.child_mut().stdout.take().unwrap();
                    let (tx, rx) = std::sync::mpsc::channel();
                    let reader = std::thread::spawn(move || {
                        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                                let _ = tx.send(value);
                                break;
                            }
                        }
                    });
                    let observation = rx.recv_timeout(Duration::from_secs(8)).unwrap();
                    let job = observation["job"].as_str().unwrap().to_owned();
                    if stage == "preassign" {
                        let mut handles = Vec::new();
                        for field in ["suspendedPid", "runnerPid", "descendantPid"] {
                            let handle = unsafe {
                                OpenProcess(
                                    0x0010_0000,
                                    0,
                                    observation[field].as_u64().unwrap() as u32,
                                )
                            };
                            assert!(!handle.is_null());
                            handles.push(handle);
                        }
                        owner.child_mut().kill().unwrap();
                        owner.child_mut().wait().unwrap();
                        // Outer test Job is still armed/open: these exits are
                        // caused by closing the killed controller's lifetime Job.
                        for handle in handles {
                            assert_eq!(unsafe { WaitForSingleObject(handle, 5000) }, 0);
                            unsafe { CloseHandle(handle) };
                        }
                    } else {
                        let deadline = std::time::Instant::now() + Duration::from_secs(5);
                        loop {
                            if let Some(status) = owner.child_mut().try_wait().unwrap() {
                                assert!(status.success());
                                break;
                            }
                            assert!(std::time::Instant::now() < deadline);
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    }
                    drop(owner.child.take());
                    if installed {
                        let name = job.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
                        let handle = unsafe { super::OpenJobObjectW(4, 0, name.as_ptr()) };
                        assert!(
                            handle.is_null(),
                            "exited installed controller Job retained a handle"
                        );
                        assert_eq!(unsafe { super::GetLastError() }, 2);
                    } else {
                        OwnedCandidateChild::prove_stopped_named_job_quiescent(&job).unwrap();
                    }
                    owner.terminate_tree_and_confirm_quiescent().unwrap();
                    reader.join().unwrap();
                }
            }
        }

        #[test]
        fn child_with_descendant() {
            if std::env::var_os("CATDESK_OWNER_JOB_TEST_DESCENDANT").is_none() {
                return;
            }
            let mut descendant = Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::sleep_child",
                ])
                .env("CATDESK_OWNER_JOB_TEST_CHILD", "1")
                .stdout(Stdio::null())
                .spawn()
                .expect("descendant process");
            println!("DESCENDANT_PID={}", descendant.id());
            std::io::stdout().flush().expect("flush descendant PID");
            std::thread::sleep(Duration::from_secs(60));
            let _ = descendant.kill();
            let _ = descendant.wait();
        }

        fn spawn_with_descendant() -> (OwnedCandidateChild, u32) {
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::child_with_descendant",
                    "--nocapture",
                ])
                .env("CATDESK_OWNER_JOB_TEST_DESCENDANT", "1")
                .stdout(Stdio::piped());
            let mut owned = OwnedCandidateChild::spawn(&mut command).expect("owned child");
            let stdout = owned.child_mut().stdout.take().expect("child stdout");
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).expect("child output") == 0 {
                        return;
                    }
                    if let Some(value) = line.split("DESCENDANT_PID=").nth(1) {
                        ready_tx
                            .send(value.trim().parse::<u32>().expect("descendant PID"))
                            .expect("send descendant PID");
                        return;
                    }
                }
            });
            let descendant_pid = ready_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("bounded descendant readiness");
            reader.join().expect("ready reader");
            (owned, descendant_pid)
        }

        #[test]
        fn release_after_durable_commit_preserves_child_and_descendant() {
            const PROCESS_TERMINATE: u32 = 0x0001;
            const SYNCHRONIZE: u32 = 0x0010_0000;
            const WAIT_OBJECT_0: u32 = 0;
            const WAIT_TIMEOUT: u32 = 0x0000_0102;
            let (mut owned, descendant_pid) = spawn_with_descendant();
            let child = unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, owned.pid()) };
            let descendant =
                unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, descendant_pid) };
            assert!(!child.is_null() && !descendant.is_null());
            let disarm = owned.disarm_live_retaining_job();
            let release = if disarm.is_ok() {
                owned.release_live()
            } else {
                Err("reviewed candidate disarm failed".into())
            };
            drop(owned);
            let child_outcome = unsafe { WaitForSingleObject(child, 250) };
            let descendant_outcome = unsafe { WaitForSingleObject(descendant, 250) };
            // These exact fixture handles are cleanup authority, including if
            // a release assertion below fails.
            unsafe { TerminateProcess(child, 1) };
            unsafe { TerminateProcess(descendant, 1) };
            let child_ended = unsafe { WaitForSingleObject(child, 5_000) };
            let descendant_ended = unsafe { WaitForSingleObject(descendant, 5_000) };
            unsafe { CloseHandle(child) };
            unsafe { CloseHandle(descendant) };
            disarm.expect("durable handoff disarm");
            release.expect("durable handoff release");
            assert_eq!(child_outcome, WAIT_TIMEOUT, "released child remains alive");
            assert_eq!(
                descendant_outcome, WAIT_TIMEOUT,
                "released descendant remains alive"
            );
            assert_eq!(child_ended, WAIT_OBJECT_0);
            assert_eq!(descendant_ended, WAIT_OBJECT_0);
        }

        #[test]
        fn failed_handoff_proves_entire_owned_tree_exited() {
            let (mut owned, _) = spawn_with_descendant();
            owned
                .terminate_tree_and_confirm_quiescent()
                .expect("whole Job quiescence");
            drop(owned);
        }

        #[test]
        fn failed_post_disarm_handoff_still_terminates_entire_tree() {
            let (mut owned, _) = spawn_with_descendant();
            owned
                .disarm_live_retaining_job()
                .expect("disarm while retaining cleanup authority");
            owned
                .terminate_tree_and_confirm_quiescent()
                .expect("post-disarm whole Job quiescence");
            drop(owned);
        }

        #[test]
        fn early_owner_return_reaps_child_and_descendant() {
            const PROCESS_TERMINATE: u32 = 0x0001;
            const SYNCHRONIZE: u32 = 0x0010_0000;
            const WAIT_OBJECT_0: u32 = 0;
            const WAIT_TIMEOUT: u32 = 0x0000_0102;
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::child_with_descendant",
                    "--nocapture",
                ])
                .env("CATDESK_OWNER_JOB_TEST_DESCENDANT", "1")
                .stdout(Stdio::piped());
            let mut owned = OwnedCandidateChild::spawn(&mut command).expect("owned child");
            let child_handle =
                unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, owned.pid()) };
            assert!(!child_handle.is_null(), "exact child process handle");
            let stdout = owned.child_mut().stdout.take().expect("child stdout");
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let ready_reader = std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).expect("child output") == 0 {
                        return;
                    }
                    if let Some(value) = line.split("DESCENDANT_PID=").nth(1) {
                        ready_tx
                            .send(value.trim().parse::<u32>().expect("descendant PID"))
                            .expect("send descendant PID");
                        return;
                    }
                }
            });
            let descendant_pid = ready_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("bounded descendant readiness");
            let descendant_handle =
                unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, descendant_pid) };
            assert!(
                !descendant_handle.is_null(),
                "exact descendant process handle"
            );
            assert_eq!(
                unsafe { WaitForSingleObject(child_handle, 0) },
                WAIT_TIMEOUT
            );
            assert_eq!(
                unsafe { WaitForSingleObject(descendant_handle, 0) },
                WAIT_TIMEOUT
            );
            // This matches a readiness or listener error exit in the owner:
            // no explicit terminate_and_wait call is needed by the caller.
            drop(owned);
            ready_reader.join().expect("ready reader");
            let child_outcome = unsafe { WaitForSingleObject(child_handle, 5_000) };
            let descendant_outcome = unsafe { WaitForSingleObject(descendant_handle, 5_000) };
            if child_outcome != WAIT_OBJECT_0 {
                unsafe { TerminateProcess(child_handle, 1) };
            }
            if descendant_outcome != WAIT_OBJECT_0 {
                unsafe { TerminateProcess(descendant_handle, 1) };
            }
            unsafe { CloseHandle(child_handle) };
            unsafe { CloseHandle(descendant_handle) };
            assert_eq!(child_outcome, WAIT_OBJECT_0, "owner must reap exact child");
            assert_eq!(
                descendant_outcome, WAIT_OBJECT_0,
                "job close must terminate descendant"
            );
        }

        #[test]
        fn owner_process_helper() {
            if std::env::var_os("CATDESK_OWNER_JOB_TEST_PARENT").is_none() {
                return;
            }
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::sleep_child",
                ])
                .env("CATDESK_OWNER_JOB_TEST_CHILD", "1")
                .stdout(Stdio::null());
            let owned = OwnedCandidateChild::spawn(&mut command).expect("owned child");
            println!("CHILD_PID={}", owned.pid());
            std::io::stdout().flush().expect("flush child PID");
            std::thread::sleep(Duration::from_secs(60));
            drop(owned);
        }

        #[test]
        fn hard_owner_crash_closes_job_and_kills_exact_child() {
            const PROCESS_TERMINATE: u32 = 0x0001;
            const SYNCHRONIZE: u32 = 0x0010_0000;
            const WAIT_OBJECT_0: u32 = 0;
            const WAIT_TIMEOUT: u32 = 0x0000_0102;
            let mut owner = Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::owner_process_helper",
                    "--nocapture",
                ])
                .env("CATDESK_OWNER_JOB_TEST_PARENT", "1")
                .stdout(Stdio::piped())
                .spawn()
                .expect("owner helper");
            let mut reader = BufReader::new(owner.stdout.take().expect("helper stdout"));
            let mut line = String::new();
            let child_pid = loop {
                line.clear();
                if reader.read_line(&mut line).expect("helper output") == 0 {
                    let _ = owner.wait();
                    panic!("owner helper exited before child ownership was announced");
                }
                if let Some(value) = line.split("CHILD_PID=").nth(1) {
                    break value.trim().parse::<u32>().expect("child PID");
                }
            };
            let child_handle =
                unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, child_pid) };
            assert!(!child_handle.is_null(), "exact child process handle");
            let alive_before_crash = unsafe { WaitForSingleObject(child_handle, 0) };
            owner.kill().expect("hard kill owner");
            owner.wait().expect("reap owner");
            let outcome = unsafe { WaitForSingleObject(child_handle, 5_000) };
            if outcome != WAIT_OBJECT_0 {
                unsafe { TerminateProcess(child_handle, 1) };
            }
            unsafe { CloseHandle(child_handle) };
            assert_eq!(
                alive_before_crash, WAIT_TIMEOUT,
                "candidate must still be alive before owner crash"
            );
            assert_eq!(outcome, WAIT_OBJECT_0, "job close must kill child");
        }
    }
}

#[cfg(windows)]
pub(crate) use windows::{OwnedCandidateChild, SpawnFailure, StoppedControllerLifetime};
