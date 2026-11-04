//! Bounded, non-authoritative records for the manual test-only diagnostic runners.
use super::*;
use crate::windows_protected_fs::StrongFileId as SerializableStrongFileId;

const LABEL: &str = "diagnostic storage record";
const MAX_RECORD_BYTES: usize = 16 * 1024;
const START_FILE: &str = "diagnostic-storage-start.json";
const OUTCOME_FILE: &str = "diagnostic-storage-outcome.json";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Runner {
    CacheSeed,
    ActiveUnclassified,
    FixedLinker,
}
impl Runner {
    fn family(self) -> &'static str {
        match self {
            Self::ActiveUnclassified => "rl",
            Self::CacheSeed | Self::FixedLinker => "reviewed-build-host-linker-diagnostic",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Tool {
    sha256: String,
    identity: String,
    version_sha256: String,
}
impl Tool {
    fn capture(value: &TrustedBuildToolV1) -> Result<Self, String> {
        if !valid_sha256(&value.sha256)
            || !valid_sha256(&value.version_sha256)
            || value.identity.len() != 26
            || !value.identity.bytes().enumerate().all(|(i, c)| {
                if matches!(i, 8 | 17) {
                    c == b':'
                } else {
                    c.is_ascii_hexdigit()
                }
            })
        {
            return Err("DIAGNOSTIC_STORAGE_TOOL_BINDING_INVALID".into());
        }
        Ok(Self {
            sha256: value.sha256.clone(),
            identity: value.identity.clone(),
            version_sha256: value.version_sha256.clone(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Start {
    schema_version: u8,
    producer: String,
    record_kind: String,
    runner: Runner,
    run_id: String,
    root_family: String,
    relative_root: String,
    root_identity: SerializableStrongFileId,
    workspace_identity: SerializableStrongFileId,
    parent_attempt_id: String,
    parent_attempt_digest: String,
    snapshot_id: String,
    snapshot_authority_digest: String,
    snapshot_manifest_digest: String,
    cargo_lock_sha256: String,
    cargo_lock_bytes: u64,
    cargo: Tool,
    rustc: Tool,
    policy_sha256: String,
    argv_sha256: String,
    environment_policy_sha256: String,
    owner_pid: u32,
    owner_creation_time_100ns: u64,
    observed_at_unix: u64,
    completion: String,
    authority: bool,
    cleanup_authorized: bool,
    retention_disposition: String,
    output_allocated_bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Outcome {
    schema_version: u8,
    producer: String,
    record_kind: String,
    runner: Runner,
    run_id: String,
    root_family: String,
    relative_root: String,
    root_identity: SerializableStrongFileId,
    workspace_identity: SerializableStrongFileId,
    parent_attempt_id: String,
    parent_attempt_digest: String,
    start_sha256: String,
    observed_at_unix: u64,
    function_result: String,
    failure_category: Option<String>,
    cargo_started: bool,
    compiler_exit_code: Option<i32>,
    stderr_eof_verified: bool,
    process_tree_completion: String,
    authority: bool,
    cleanup_authorized: bool,
    retention_disposition: String,
    output_allocated_bytes: Option<u64>,
}

#[derive(Default)]
pub(super) struct Observation {
    pub(super) cargo_started: bool,
    pub(super) compiler_exit_code: Option<i32>,
    pub(super) stderr_eof_verified: bool,
    pub(super) owned_job_quiescent: bool,
}

fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| "DIAGNOSTIC_STORAGE_TIME_UNAVAILABLE".into())
}

fn owner_creation_time() -> Result<u64, String> {
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn GetProcessTimes(
            process: *mut c_void,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }
    let (mut created, mut exited, mut kernel, mut user) = (
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
    );
    if unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err("DIAGNOSTIC_STORAGE_OWNER_UNAVAILABLE".into());
    }
    let value = (u64::from(created.high) << 32) | u64::from(created.low);
    if value == 0 {
        return Err("DIAGNOSTIC_STORAGE_OWNER_UNAVAILABLE".into());
    }
    Ok(value)
}

fn encode<T: Serialize>(record: &T) -> Result<Vec<u8>, String> {
    let bytes =
        serde_json::to_vec(record).map_err(|_| "DIAGNOSTIC_STORAGE_SERIALIZATION_FAILED")?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err("DIAGNOSTIC_STORAGE_RECORD_TOO_LARGE".into());
    }
    Ok(bytes)
}

struct Lifecycle {
    workspace: ProtectedDirectoryGuard,
    root: ProtectedDirectoryGuard,
    start: Start,
    start_sha256: String,
}

fn root_notice(start: &Start) -> String {
    format!("DIAGNOSTIC_STORAGE_ROOT_RELATIVE={}\n", start.relative_root)
}

impl Lifecycle {
    fn register(
        workspace: &Path,
        root: &ProtectedDirectoryGuard,
        attempt: &ReviewedBuildAttemptV1,
        snapshot: &ValidatedReviewedSourceSnapshotV1,
        runner: Runner,
    ) -> Result<Self, String> {
        let workspace_guard = ProtectedDirectoryGuard::acquire(workspace, LABEL)?;
        let relative = root
            .path()
            .strip_prefix(workspace_guard.path())
            .map_err(|_| "DIAGNOSTIC_STORAGE_ROOT_SCOPE_INVALID")?;
        let parts = relative
            .components()
            .map(|c| c.as_os_str().to_str())
            .collect::<Option<Vec<_>>>()
            .ok_or("DIAGNOSTIC_STORAGE_ROOT_SCOPE_INVALID")?;
        if parts.len() != 3
            || parts[0] != "target-verify"
            || parts[1] != runner.family()
            || !valid_attempt_id(parts[2])
        {
            return Err("DIAGNOSTIC_STORAGE_ROOT_SCOPE_INVALID".into());
        }
        if !valid_attempt_id(&attempt.build_attempt_id)
            || digest_attempt(attempt)? != attempt.attempt_digest
            || attempt.snapshot_id != snapshot.snapshot_id
            || attempt.snapshot_authority_digest != snapshot.authority_digest
            || attempt.snapshot_manifest_digest != snapshot.manifest_digest
            || [
                &snapshot.snapshot_id,
                &snapshot.authority_digest,
                &snapshot.manifest_digest,
                &attempt.policy.policy_sha256,
                &attempt.policy.argv_sha256,
                &attempt.policy.environment_policy_sha256,
            ]
            .iter()
            .any(|v| !valid_sha256(v))
        {
            return Err("DIAGNOSTIC_STORAGE_SOURCE_BINDING_INVALID".into());
        }
        let locks = snapshot
            .entries
            .iter()
            .filter(|e| e.relative_path == "Cargo.lock")
            .collect::<Vec<_>>();
        if locks.len() != 1
            || !valid_sha256(&locks[0].sha256)
            || locks[0].byte_length == 0
            || locks[0].byte_length > MAX_CARGO_LOCK_BYTES
        {
            return Err("DIAGNOSTIC_STORAGE_LOCK_BINDING_INVALID".into());
        }
        let start = Start {
            schema_version: 1,
            producer: "CATDESK_MANUAL_DIAGNOSTIC_STORAGE_V1".into(),
            record_kind: "START".into(),
            runner,
            run_id: parts[2].into(),
            root_family: runner.family().into(),
            relative_root: parts.join("/"),
            root_identity: root.strong_physical_identity(LABEL)?,
            workspace_identity: workspace_guard.strong_physical_identity(LABEL)?,
            parent_attempt_id: attempt.build_attempt_id.clone(),
            parent_attempt_digest: attempt.attempt_digest.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            snapshot_authority_digest: snapshot.authority_digest.clone(),
            snapshot_manifest_digest: snapshot.manifest_digest.clone(),
            cargo_lock_sha256: locks[0].sha256.clone(),
            cargo_lock_bytes: locks[0].byte_length,
            cargo: Tool::capture(&attempt.cargo)?,
            rustc: Tool::capture(&attempt.rustc)?,
            policy_sha256: attempt.policy.policy_sha256.clone(),
            argv_sha256: attempt.policy.argv_sha256.clone(),
            environment_policy_sha256: attempt.policy.environment_policy_sha256.clone(),
            owner_pid: std::process::id(),
            owner_creation_time_100ns: owner_creation_time()?,
            observed_at_unix: now()?,
            completion: "UNKNOWN".into(),
            authority: false,
            cleanup_authorized: false,
            retention_disposition: "HOLD_OWNER_REVIEW".into(),
            output_allocated_bytes: None,
        };
        let bytes = encode(&start)?;
        root.assert_stable(LABEL)?;
        workspace_guard.assert_stable(LABEL)?;
        write_new_regular_in(root, START_FILE, &bytes, LABEL)?;
        // A bounded discovery hint, never authority; stderr failure cannot mask Result.
        let _ = std::io::Write::write_all(
            &mut std::io::stderr().lock(),
            root_notice(&start).as_bytes(),
        );
        Ok(Self {
            workspace: workspace_guard,
            root: root.try_clone(LABEL)?,
            start,
            start_sha256: sha256(&bytes),
        })
    }

    fn finish<T>(
        &self,
        result: &Result<T, String>,
        observation: &Observation,
    ) -> Result<(), String> {
        self.workspace.assert_stable(LABEL)?;
        self.root.assert_stable(LABEL)?;
        if self.workspace.strong_physical_identity(LABEL)? != self.start.workspace_identity
            || self.root.strong_physical_identity(LABEL)? != self.start.root_identity
        {
            return Err("DIAGNOSTIC_STORAGE_IDENTITY_CHANGED".into());
        }
        let bytes = read_relative_regular(&self.root, START_FILE, MAX_RECORD_BYTES as u64, LABEL)?;
        if sha256(&bytes) != self.start_sha256 {
            return Err("DIAGNOSTIC_STORAGE_START_CHANGED".into());
        }
        let outcome = Outcome {
            schema_version: 1,
            producer: self.start.producer.clone(),
            record_kind: "OUTCOME".into(),
            runner: self.start.runner,
            run_id: self.start.run_id.clone(),
            root_family: self.start.root_family.clone(),
            relative_root: self.start.relative_root.clone(),
            root_identity: self.start.root_identity,
            workspace_identity: self.start.workspace_identity,
            parent_attempt_id: self.start.parent_attempt_id.clone(),
            parent_attempt_digest: self.start.parent_attempt_digest.clone(),
            start_sha256: self.start_sha256.clone(),
            observed_at_unix: now()?,
            function_result: if result.is_ok() {
                "RETURNED_OK"
            } else {
                "RETURNED_ERR"
            }
            .into(),
            failure_category: result
                .as_ref()
                .err()
                .map(|_| "FUNCTION_ERROR_UNCLASSIFIED".into()),
            cargo_started: observation.cargo_started,
            compiler_exit_code: observation.compiler_exit_code,
            stderr_eof_verified: observation.stderr_eof_verified,
            process_tree_completion: if self.start.runner == Runner::ActiveUnclassified
                && observation.cargo_started
                && observation.owned_job_quiescent
                && observation.stderr_eof_verified
            {
                "OWNED_JOB_AND_STDERR_EOF_VERIFIED"
            } else if observation.cargo_started {
                "UNPROVEN"
            } else {
                "NOT_STARTED"
            }
            .into(),
            authority: false,
            cleanup_authorized: false,
            retention_disposition: "HOLD_OWNER_REVIEW".into(),
            output_allocated_bytes: None,
        };
        write_new_regular_in(&self.root, OUTCOME_FILE, &encode(&outcome)?, LABEL)
    }
}

pub(super) fn run<T>(
    workspace: &Path,
    root: &mut ProtectedDirectoryGuard,
    attempt: &ReviewedBuildAttemptV1,
    snapshot: &ValidatedReviewedSourceSnapshotV1,
    runner: Runner,
    body: impl FnOnce(&mut ProtectedDirectoryGuard, &mut Observation) -> Result<T, String>,
) -> Result<T, String> {
    // A failed start registration refuses before source/cache materialization.
    let lifecycle = Lifecycle::register(workspace, root, attempt, snapshot, runner)?;
    let mut observation = Observation::default();
    // A panic/crash deliberately leaves only START/UNKNOWN. No Drop completion.
    let result = body(root, &mut observation);
    // Missing/colliding/failed outcome remains UNKNOWN/HOLD; never mask the body.
    let _ = lifecycle.finish(&result, &observation);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reviewed_source_snapshot::ReviewedSourceEntryV1;

    fn fixture(
        runner: Runner,
    ) -> (
        PathBuf,
        ProtectedDirectoryGuard,
        ReviewedBuildAttemptV1,
        ValidatedReviewedSourceSnapshotV1,
    ) {
        let workspace =
            std::env::temp_dir().join(format!("catdesk-storage-record-{}", Uuid::new_v4()));
        fs::create_dir(&workspace).unwrap();
        let root = match runner {
            Runner::ActiveUnclassified => fixed_worker_shaped_cargo_diagnostic_root(&workspace),
            _ => fixed_v5_host_linker_diagnostic_root(&workspace),
        }
        .unwrap();
        let tool = TrustedBuildToolV1 {
            absolute_path: r"C:\private-path-not-for-records\cargo.exe".into(),
            sha256: "a".repeat(64),
            length: 1,
            identity: "00000001:00000002:00000003".into(),
            version_sha256: "b".repeat(64),
        };
        let snapshot = ValidatedReviewedSourceSnapshotV1 {
            snapshot_id: "1".repeat(64),
            authority_digest: "2".repeat(64),
            manifest_digest: "3".repeat(64),
            entries: vec![ReviewedSourceEntryV1 {
                relative_path: "Cargo.lock".into(),
                byte_length: 6,
                sha256: sha256(b"locked"),
                content_object: "private-content-object".into(),
            }],
            bytes_root: workspace.join("private-snapshot-bytes"),
        };
        let mut attempt = ReviewedBuildAttemptV1 {
            schema_version: ATTEMPT_SCHEMA_VERSION,
            build_attempt_id: "c".repeat(32),
            confirmation_token: "private-confirmation-token".into(),
            review_session_id: "private-review-session".into(),
            review_record_id: "private-review-record".into(),
            review_authority_sha256: "4".repeat(64),
            snapshot_expected: ReviewedSourceSnapshotExpectedV1 {
                session_id: "private-session".into(),
                project_id: "private-project".into(),
                approved_contract_hash: "private-contract".into(),
                logical_task_id: "private-task".into(),
                completion_artifact_ids: vec![],
                current_outputs: vec![],
                baseline_observations: vec![],
            },
            snapshot_id: snapshot.snapshot_id.clone(),
            snapshot_authority_digest: snapshot.authority_digest.clone(),
            snapshot_manifest_digest: snapshot.manifest_digest.clone(),
            cargo: tool.clone(),
            rustc: tool,
            policy: fixed_policy(),
            attempt_digest: String::new(),
        };
        attempt.attempt_digest = digest_attempt(&attempt).unwrap();
        (workspace, root, attempt, snapshot)
    }
    fn read<T: serde::de::DeserializeOwned>(root: &ProtectedDirectoryGuard, name: &str) -> T {
        serde_json::from_slice(
            &read_relative_regular(root, name, MAX_RECORD_BYTES as u64, LABEL).unwrap(),
        )
        .unwrap()
    }
    fn remove(workspace: PathBuf, root: ProtectedDirectoryGuard) {
        drop(root);
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn diagnostic_storage_record_bounds_are_exact() {
        assert_eq!(
            encode(&"x".repeat(MAX_RECORD_BYTES - 2)).unwrap().len(),
            MAX_RECORD_BYTES
        );
        assert!(encode(&"x".repeat(MAX_RECORD_BYTES - 1)).is_err());
    }

    #[test]
    fn diagnostic_storage_tool_identity_rejects_extra_component_colons() {
        let (workspace, root, mut attempt, _) = fixture(Runner::CacheSeed);
        assert!(Tool::capture(&attempt.cargo).is_ok());
        attempt.cargo.identity = ":0000001:00000002:00000003".into();
        assert!(Tool::capture(&attempt.cargo).is_err());
        remove(workspace, root);
    }

    #[test]
    fn diagnostic_storage_start_collision_refuses_before_body() {
        let (workspace, mut root, attempt, snapshot) = fixture(Runner::CacheSeed);
        write_new_regular_in(&root, START_FILE, b"original evidence", LABEL).unwrap();
        let heavy = std::cell::Cell::new(false);
        let result = run(
            &workspace,
            &mut root,
            &attempt,
            &snapshot,
            Runner::CacheSeed,
            |_, _| {
                heavy.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!heavy.get());
        assert_eq!(
            read_relative_regular(&root, START_FILE, 100, LABEL).unwrap(),
            b"original evidence"
        );
        remove(workspace, root);
    }

    #[test]
    fn diagnostic_storage_root_source_attempt_and_lock_bindings_fail_closed() {
        let (workspace, mut root, mut attempt, mut snapshot) = fixture(Runner::CacheSeed);
        assert!(
            run(
                &workspace,
                &mut root,
                &attempt,
                &snapshot,
                Runner::ActiveUnclassified,
                |_, _| Ok(())
            )
            .is_err()
        );
        snapshot.manifest_digest = "5".repeat(64);
        assert!(
            run(
                &workspace,
                &mut root,
                &attempt,
                &snapshot,
                Runner::CacheSeed,
                |_, _| Ok(())
            )
            .is_err()
        );
        snapshot.manifest_digest = attempt.snapshot_manifest_digest.clone();
        snapshot.entries.push(snapshot.entries[0].clone());
        assert!(
            run(
                &workspace,
                &mut root,
                &attempt,
                &snapshot,
                Runner::CacheSeed,
                |_, _| Ok(())
            )
            .is_err()
        );
        snapshot.entries.pop();
        attempt.attempt_digest = "6".repeat(64);
        assert!(
            run(
                &workspace,
                &mut root,
                &attempt,
                &snapshot,
                Runner::CacheSeed,
                |_, _| Ok(())
            )
            .is_err()
        );
        assert!(!root.path().join(START_FILE).exists());
        remove(workspace, root);
    }

    #[test]
    fn diagnostic_storage_records_whitelist_no_secrets_and_bind_raw_start() {
        let (workspace, mut root, attempt, snapshot) = fixture(Runner::CacheSeed);
        let result: Result<(), String> = run(
            &workspace,
            &mut root,
            &attempt,
            &snapshot,
            Runner::CacheSeed,
            |_, _| Err("private-body-error".into()),
        );
        assert_eq!(result, Err("private-body-error".into()));
        let bytes =
            read_relative_regular(&root, START_FILE, MAX_RECORD_BYTES as u64, LABEL).unwrap();
        let start: Start = serde_json::from_slice(&bytes).unwrap();
        let outcome: Outcome = read(&root, OUTCOME_FILE);
        let notice = root_notice(&start);
        assert_eq!(
            notice,
            format!(
                "DIAGNOSTIC_STORAGE_ROOT_RELATIVE=target-verify/{}/{}\n",
                start.root_family, start.run_id
            )
        );
        assert!(!notice.contains("private-") && !notice.contains(':') && !notice.contains('\\'));
        let serialized = String::from_utf8(bytes.clone()).unwrap()
            + &String::from_utf8(encode(&outcome).unwrap()).unwrap();
        // Small synthetic handoff retained in the private focused-test log only.
        println!(
            "DIAGNOSTIC_STORAGE_FIXTURE_START_B64={}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)
        );
        println!(
            "DIAGNOSTIC_STORAGE_FIXTURE_OUTCOME_B64={}",
            base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                read_relative_regular(&root, OUTCOME_FILE, MAX_RECORD_BYTES as u64, LABEL).unwrap()
            )
        );
        assert!(!serialized.contains("private-"));
        assert!(!serialized.contains("absolutePath"));
        assert!(!serialized.contains("confirmationToken"));
        assert_eq!(start.parent_attempt_digest, attempt.attempt_digest);
        assert_eq!(start.snapshot_manifest_digest, snapshot.manifest_digest);
        assert_eq!(start.cargo_lock_sha256, snapshot.entries[0].sha256);
        assert_eq!(
            start.root_identity,
            root.strong_physical_identity(LABEL).unwrap()
        );
        assert_eq!(outcome.root_identity, start.root_identity);
        assert_eq!(outcome.workspace_identity, start.workspace_identity);
        assert_eq!(outcome.start_sha256, sha256(&bytes));
        assert_eq!(outcome.function_result, "RETURNED_ERR");
        assert_eq!(
            outcome.failure_category.as_deref(),
            Some("FUNCTION_ERROR_UNCLASSIFIED")
        );
        assert_eq!(outcome.process_tree_completion, "NOT_STARTED");
        assert!(
            !start.authority
                && !outcome.authority
                && !start.cleanup_authorized
                && !outcome.cleanup_authorized
        );
        assert_eq!(outcome.retention_disposition, "HOLD_OWNER_REVIEW");
        assert_eq!(outcome.output_allocated_bytes, None);
        assert!(start.owner_pid > 0 && start.owner_creation_time_100ns > 0);
        remove(workspace, root);
    }

    #[test]
    fn diagnostic_storage_function_ok_is_not_compiler_success_or_fixed_job_proof() {
        for runner in [Runner::FixedLinker, Runner::ActiveUnclassified] {
            let (workspace, mut root, attempt, snapshot) = fixture(runner);
            assert_eq!(
                run(
                    &workspace,
                    &mut root,
                    &attempt,
                    &snapshot,
                    runner,
                    |_, observed| {
                        observed.cargo_started = true;
                        observed.compiler_exit_code = Some(101);
                        observed.stderr_eof_verified = true;
                        observed.owned_job_quiescent = true;
                        Ok(101)
                    }
                )
                .unwrap(),
                101
            );
            let outcome: Outcome = read(&root, OUTCOME_FILE);
            assert_eq!(outcome.function_result, "RETURNED_OK");
            assert_eq!(outcome.compiler_exit_code, Some(101));
            assert_eq!(
                outcome.process_tree_completion,
                if runner == Runner::FixedLinker {
                    "UNPROVEN"
                } else {
                    "OWNED_JOB_AND_STDERR_EOF_VERIFIED"
                }
            );
            assert_eq!(outcome.retention_disposition, "HOLD_OWNER_REVIEW");
            remove(workspace, root);
        }
    }

    #[test]
    fn diagnostic_storage_missing_outcome_and_failed_observation_preserve_result() {
        let (workspace, mut root, attempt, snapshot) = fixture(Runner::FixedLinker);
        let answer = run(
            &workspace,
            &mut root,
            &attempt,
            &snapshot,
            Runner::FixedLinker,
            |root, observed| {
                observed.cargo_started = true;
                fs::write(root.path().join(START_FILE), b"tampered temporary fixture").unwrap();
                Ok(17)
            },
        )
        .unwrap();
        assert_eq!(answer, 17);
        assert!(!root.path().join(OUTCOME_FILE).exists());
        remove(workspace, root);
    }

    #[test]
    fn diagnostic_storage_outcome_collision_never_overwrites_or_masks_error() {
        let (workspace, mut root, attempt, snapshot) = fixture(Runner::CacheSeed);
        let answer: Result<(), String> = run(
            &workspace,
            &mut root,
            &attempt,
            &snapshot,
            Runner::CacheSeed,
            |root, _| {
                write_new_regular_in(root, OUTCOME_FILE, b"unknown existing evidence", LABEL)
                    .unwrap();
                Err("original error".into())
            },
        );
        assert_eq!(answer, Err("original error".into()));
        assert_eq!(
            read_relative_regular(&root, OUTCOME_FILE, 100, LABEL).unwrap(),
            b"unknown existing evidence"
        );
        remove(workspace, root);
    }

    #[test]
    fn diagnostic_storage_active_error_without_eof_stays_unproven() {
        let (workspace, mut root, attempt, snapshot) = fixture(Runner::ActiveUnclassified);
        let result: Result<(), String> = run(
            &workspace,
            &mut root,
            &attempt,
            &snapshot,
            Runner::ActiveUnclassified,
            |_, observed| {
                observed.cargo_started = true;
                observed.owned_job_quiescent = true;
                Err("original observation failure".into())
            },
        );
        assert_eq!(result, Err("original observation failure".into()));
        let outcome: Outcome = read(&root, OUTCOME_FILE);
        assert_eq!(outcome.process_tree_completion, "UNPROVEN");
        assert_eq!(outcome.compiler_exit_code, None);
        assert!(!outcome.stderr_eof_verified);
        remove(workspace, root);
    }

    #[test]
    fn diagnostic_storage_changed_identity_prevents_completion() {
        let (workspace, root, attempt, snapshot) = fixture(Runner::CacheSeed);
        let mut lifecycle =
            Lifecycle::register(&workspace, &root, &attempt, &snapshot, Runner::CacheSeed).unwrap();
        lifecycle.start.workspace_identity = SerializableStrongFileId::default();
        assert!(lifecycle.finish(&Ok(()), &Observation::default()).is_err());
        assert!(!root.path().join(OUTCOME_FILE).exists());
        drop(lifecycle);
        remove(workspace, root);
    }

    #[test]
    fn diagnostic_storage_panic_leaves_start_unknown_not_terminal() {
        let (workspace, mut root, attempt, snapshot) = fixture(Runner::CacheSeed);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run::<()>(
                &workspace,
                &mut root,
                &attempt,
                &snapshot,
                Runner::CacheSeed,
                |_, _| panic!("synthetic interrupted diagnostic"),
            )
        }));
        assert!(panic.is_err());
        let start: Start = read(&root, START_FILE);
        assert_eq!(start.completion, "UNKNOWN");
        assert_eq!(start.retention_disposition, "HOLD_OWNER_REVIEW");
        assert!(!root.path().join(OUTCOME_FILE).exists());
        remove(workspace, root);
    }

    #[test]
    fn diagnostic_storage_hooks_only_real_runners_before_materialization() {
        let source = include_str!("reviewed_build.rs");
        for name in [
            "run_fixed_v5_cargo_cache_seed_diagnostic",
            "run_active_unclassified_cargo_diagnostic",
            "run_fixed_v5_host_linker_diagnostic",
        ] {
            let body = &source[source.find(&format!("fn {name}(")).unwrap()..];
            assert!(
                body.find("diagnostic_storage::run(").unwrap()
                    < body.find("materialize_snapshot_into(").unwrap()
            );
        }
        let root_constructor = source
            .split("fn fixed_v5_host_linker_diagnostic_root(")
            .nth(1)
            .unwrap()
            .split("fn run_fixed_v5_cargo_cache_seed_diagnostic(")
            .next()
            .unwrap();
        assert!(!root_constructor.contains("diagnostic_storage::run"));
    }
}
