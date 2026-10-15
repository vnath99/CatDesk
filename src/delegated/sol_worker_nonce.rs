//! Dormant, host-side one-shot nonce journal for a future isolated Sol worker.
//!
//! A trusted host must pin a ledger directory that the worker cannot write.
//! The journal uses create-once, synced records, never a mutable status file:
//! an incomplete burn record after a process crash is still spent and cannot
//! be removed or retried on reopen. Parent-directory metadata durability
//! across power loss is unproven. This module does not prove the caller's
//! directory ACL, the worker's identity, process exit, or lost write
//! authority. The host must separately persist which issued nonce belongs to
//! its active turn; an abandoned, unburned issue record is not automatically
//! revoked. This module grants no Sol launch capability or staging authority.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::windows_protected_fs::{
    ProtectedDirectoryGuard, read_relative_regular_unique, write_new_regular_in,
};

use super::codex_model_policy::{CodexCliTurnProfileEvidenceV1, SOL_MEDIUM_MODEL};
use super::sol_worker_output::{
    ExpectedSolWorkerOutputV1, SolWorkerOutputManifestV1, VerifiedSolWorkerOutputV1,
    verify_exact_sol_worker_output,
};

const LABEL: &str = "Sol worker nonce journal";
const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_ALLOWED_PATHS: usize = 256;

/// Host-held authority for one logical task. It is captured before the
/// worker receives its nonce and must be reconstructed from trusted session
/// state, not from the worker's output manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SolWorkerNonceAuthorityV1 {
    pub session_id: String,
    pub task_id: String,
    pub host_turn_id: String,
    pub contract_hash: String,
    pub baseline_commit: String,
    pub allowed_paths: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct IssuedNonceRecordV1 {
    schema_version: u32,
    purpose: String,
    nonce: String,
    authority: SolWorkerNonceAuthorityV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct BoundNativeTurnRecordV1 {
    schema_version: u32,
    purpose: String,
    nonce: String,
    authority: SolWorkerNonceAuthorityV1,
    expected_codex_thread_id: String,
    profile: CodexCliTurnProfileEvidenceV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct BurnedNonceRecordV1 {
    schema_version: u32,
    purpose: String,
    nonce: String,
    session_id: String,
    task_id: String,
    host_turn_id: String,
    contract_hash: String,
    baseline_commit: String,
    native_turn_id: String,
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_authority(authority: &SolWorkerNonceAuthorityV1) -> bool {
    valid_slug(&authority.session_id)
        && valid_slug(&authority.task_id)
        && valid_slug(&authority.host_turn_id)
        && authority.contract_hash.len() == 64
        && authority
            .contract_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        && authority.baseline_commit.len() == 40
        && authority
            .baseline_commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        && authority.allowed_paths.len() <= MAX_ALLOWED_PATHS
        && authority
            .allowed_paths
            .iter()
            .all(|path| !path.is_empty() && path.len() <= 512)
}

fn valid_nonce(nonce: &str) -> bool {
    Uuid::parse_str(nonce)
        .is_ok_and(|uuid| uuid.get_version_num() == 4 && uuid.hyphenated().to_string() == nonce)
}

fn issue_name(nonce: &str) -> String {
    format!("issued-{nonce}.json")
}

fn burn_name(nonce: &str) -> String {
    format!("burned-{nonce}.json")
}

fn bound_name(nonce: &str) -> String {
    format!("native-turn-{nonce}.json")
}

fn serialize_bounded<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(value).map_err(|_| format!("{LABEL} is invalid"))?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(format!("{LABEL} is oversized"));
    }
    Ok(bytes)
}

fn require_matching_issue(
    ledger: &ProtectedDirectoryGuard,
    authority: &SolWorkerNonceAuthorityV1,
    nonce: &str,
) -> Result<(), String> {
    if !valid_authority(authority) || !valid_nonce(nonce) {
        return Err(format!("{LABEL} handoff binding is invalid"));
    }
    ledger.assert_stable(LABEL)?;
    let issue_bytes =
        read_relative_regular_unique(ledger, &issue_name(nonce), MAX_RECORD_BYTES as u64, LABEL)?;
    let issue: IssuedNonceRecordV1 = serde_json::from_slice(&issue_bytes)
        .map_err(|_| format!("{LABEL} issue record is invalid"))?;
    if issue.schema_version != 1
        || issue.purpose != "sol-worker-handoff-issued"
        || issue.nonce != nonce
        || issue.authority != *authority
        || serialize_bounded(&issue)? != issue_bytes
    {
        return Err(format!("{LABEL} issue record does not match"));
    }
    Ok(())
}

/// Write and sync the authority before giving this host-generated nonce to a
/// worker.
/// The caller keeps the same pinned, worker-inaccessible ledger object for the
/// later handoff. A collision or any incomplete write is a terminal error.
pub(crate) fn issue_sol_worker_nonce(
    ledger: &ProtectedDirectoryGuard,
    authority: &SolWorkerNonceAuthorityV1,
) -> Result<String, String> {
    if !valid_authority(authority) {
        return Err(format!("{LABEL} authority is invalid"));
    }
    ledger.assert_stable(LABEL)?;
    let nonce = Uuid::new_v4().hyphenated().to_string();
    let record = IssuedNonceRecordV1 {
        schema_version: 1,
        purpose: "sol-worker-handoff-issued".into(),
        nonce: nonce.clone(),
        authority: authority.clone(),
    };
    let bytes = serialize_bounded(&record)?;
    write_new_regular_in(ledger, &issue_name(&nonce), &bytes, LABEL)?;
    // A successful create and file flush is followed by a same-object,
    // bounded no-follow read. An incomplete issue record left by a process
    // crash cannot be interpreted as a valid nonce by the consumer.
    let observed =
        read_relative_regular_unique(ledger, &issue_name(&nonce), MAX_RECORD_BYTES as u64, LABEL)?;
    if observed != bytes {
        return Err(format!("{LABEL} issue record changed"));
    }
    Ok(nonce)
}

/// Bind the nonce to the exact completed native Codex turn after trusted
/// host-side telemetry has validated that turn. The evidence and expected
/// thread must come from CatDesk's native profile reader and persisted host
/// state, not from the worker manifest. A second bind, even with identical
/// values, is rejected by the create-once record.
pub(crate) fn bind_sol_worker_native_turn(
    ledger: &ProtectedDirectoryGuard,
    authority: &SolWorkerNonceAuthorityV1,
    nonce: &str,
    expected_codex_thread_id: &str,
    profile: &CodexCliTurnProfileEvidenceV1,
) -> Result<(), String> {
    if !valid_slug(expected_codex_thread_id)
        || !profile.matches(expected_codex_thread_id, SOL_MEDIUM_MODEL)
    {
        return Err(format!("{LABEL} native turn profile is invalid"));
    }
    require_matching_issue(ledger, authority, nonce)?;
    let record = BoundNativeTurnRecordV1 {
        schema_version: 1,
        purpose: "sol-worker-native-turn-bound".into(),
        nonce: nonce.into(),
        authority: authority.clone(),
        expected_codex_thread_id: expected_codex_thread_id.into(),
        profile: profile.clone(),
    };
    let bytes = serialize_bounded(&record)?;
    let name = bound_name(nonce);
    write_new_regular_in(ledger, &name, &bytes, LABEL)?;
    let observed = read_relative_regular_unique(ledger, &name, MAX_RECORD_BYTES as u64, LABEL)?;
    if observed != bytes {
        return Err(format!("{LABEL} native turn binding changed"));
    }
    Ok(())
}

/// Verify output and write a synced burn marker before returning captured
/// bytes. A process restart sees an existing marker as spent.
/// The caller must first prove the producer and all descendants have exited
/// and cannot mutate either pinned directory. This function deliberately
/// does not stage files or turn a nonce into a launch permit. A failed burn,
/// including a pre-existing or torn marker, returns no output and never
/// deletes the marker.
pub(crate) fn verify_and_burn_sol_worker_output(
    ledger: &ProtectedDirectoryGuard,
    output_root: &ProtectedDirectoryGuard,
    authority: &SolWorkerNonceAuthorityV1,
    expected: &ExpectedSolWorkerOutputV1,
    manifest: &SolWorkerOutputManifestV1,
) -> Result<VerifiedSolWorkerOutputV1, String> {
    if !valid_authority(authority)
        || !valid_nonce(&expected.handoff_nonce)
        || authority.task_id != expected.task_id
        || authority.baseline_commit != expected.baseline_commit
        || authority.allowed_paths != expected.allowed_paths
    {
        return Err(format!("{LABEL} handoff binding is invalid"));
    }
    require_matching_issue(ledger, authority, &expected.handoff_nonce)?;
    let bound_bytes = read_relative_regular_unique(
        ledger,
        &bound_name(&expected.handoff_nonce),
        MAX_RECORD_BYTES as u64,
        LABEL,
    )?;
    let bound: BoundNativeTurnRecordV1 = serde_json::from_slice(&bound_bytes)
        .map_err(|_| format!("{LABEL} native turn binding is invalid"))?;
    if bound.schema_version != 1
        || bound.purpose != "sol-worker-native-turn-bound"
        || bound.nonce != expected.handoff_nonce
        || bound.authority != *authority
        || !valid_slug(&bound.expected_codex_thread_id)
        || !bound
            .profile
            .matches(&bound.expected_codex_thread_id, SOL_MEDIUM_MODEL)
        || bound.profile.codex_turn_id != expected.native_turn_id
        || serialize_bounded(&bound)? != bound_bytes
    {
        return Err(format!("{LABEL} native turn binding does not match"));
    }
    let output = verify_exact_sol_worker_output(output_root, expected, manifest)?;
    let burn = BurnedNonceRecordV1 {
        schema_version: 1,
        purpose: "sol-worker-handoff-burned".into(),
        nonce: expected.handoff_nonce.clone(),
        session_id: authority.session_id.clone(),
        task_id: authority.task_id.clone(),
        host_turn_id: authority.host_turn_id.clone(),
        contract_hash: authority.contract_hash.clone(),
        baseline_commit: authority.baseline_commit.clone(),
        native_turn_id: expected.native_turn_id.clone(),
    };
    let bytes = serialize_bounded(&burn)?;
    let name = burn_name(&expected.handoff_nonce);
    // FILE_CREATE under the pinned ledger is the replay serialization point.
    // Existing names, including torn crash residue, fail closed.
    write_new_regular_in(ledger, &name, &bytes, LABEL)?;
    let observed = read_relative_regular_unique(ledger, &name, MAX_RECORD_BYTES as u64, LABEL)?;
    if observed != bytes {
        return Err(format!("{LABEL} burn record changed"));
    }
    Ok(output)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::PathBuf;

    struct Fixture {
        base: PathBuf,
        ledger_path: PathBuf,
        output_path: PathBuf,
        authority: SolWorkerNonceAuthorityV1,
        expected: ExpectedSolWorkerOutputV1,
        manifest: SolWorkerOutputManifestV1,
    }

    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!("catdesk-sol-nonce-{}", Uuid::new_v4()));
            let ledger_path = base.join("ledger");
            let output_path = base.join("output");
            fs::create_dir(&base).expect("fixture base");
            fs::create_dir(&ledger_path).expect("fixture ledger");
            fs::create_dir(&output_path).expect("fixture output");
            fs::create_dir(output_path.join("src")).expect("output src");
            fs::write(output_path.join("src/lib.rs"), b"changed\n").expect("output file");
            let authority = SolWorkerNonceAuthorityV1 {
                session_id: "session-1".into(),
                task_id: "T-0483".into(),
                host_turn_id: "turn-123".into(),
                contract_hash: "b".repeat(64),
                baseline_commit: "a".repeat(40),
                allowed_paths: BTreeSet::from(["src/lib.rs".into()]),
            };
            let ledger = ProtectedDirectoryGuard::acquire(&ledger_path, LABEL).expect("ledger");
            let nonce = issue_sol_worker_nonce(&ledger, &authority).expect("issue nonce");
            let expected = ExpectedSolWorkerOutputV1 {
                task_id: authority.task_id.clone(),
                baseline_commit: authority.baseline_commit.clone(),
                native_turn_id: "turn-123".into(),
                handoff_nonce: nonce,
                allowed_paths: authority.allowed_paths.clone(),
            };
            let profile = CodexCliTurnProfileEvidenceV1 {
                codex_thread_id: "thread-abc".into(),
                codex_turn_id: expected.native_turn_id.clone(),
                model_id: SOL_MEDIUM_MODEL.into(),
                reasoning_effort: "medium".into(),
                evidence_sha256: "c".repeat(64),
            };
            bind_sol_worker_native_turn(
                &ledger,
                &authority,
                &expected.handoff_nonce,
                "thread-abc",
                &profile,
            )
            .expect("bind native turn");
            drop(ledger);
            let manifest = SolWorkerOutputManifestV1 {
                schema_version: 1,
                task_id: expected.task_id.clone(),
                baseline_commit: expected.baseline_commit.clone(),
                native_turn_id: expected.native_turn_id.clone(),
                handoff_nonce: expected.handoff_nonce.clone(),
                files: vec![super::super::sol_worker_output::SolWorkerOutputFileV1 {
                    path: "src/lib.rs".into(),
                    size: 8,
                    sha256: format!("{:x}", Sha256::digest(b"changed\n")),
                }],
            };
            Self {
                base,
                ledger_path,
                output_path,
                authority,
                expected,
                manifest,
            }
        }

        fn verify(&self) -> Result<VerifiedSolWorkerOutputV1, String> {
            let ledger = ProtectedDirectoryGuard::acquire(&self.ledger_path, LABEL)?;
            let output = ProtectedDirectoryGuard::acquire(&self.output_path, LABEL)?;
            verify_and_burn_sol_worker_output(
                &ledger,
                &output,
                &self.authority,
                &self.expected,
                &self.manifest,
            )
        }

        fn cleanup(self) {
            let issued = self
                .ledger_path
                .join(issue_name(&self.expected.handoff_nonce));
            let burned = self
                .ledger_path
                .join(burn_name(&self.expected.handoff_nonce));
            let bound = self
                .ledger_path
                .join(bound_name(&self.expected.handoff_nonce));
            if issued.exists() {
                fs::remove_file(issued).expect("remove issued fixture record");
            }
            if burned.exists() {
                fs::remove_file(burned).expect("remove burned fixture record");
            }
            if bound.exists() {
                fs::remove_file(bound).expect("remove bound fixture record");
            }
            fs::remove_file(self.output_path.join("src/lib.rs"))
                .expect("remove output fixture file");
            fs::remove_dir(self.output_path.join("src")).expect("remove output src");
            fs::remove_dir(self.output_path).expect("remove output root");
            fs::remove_dir(self.ledger_path).expect("remove ledger root");
            fs::remove_dir(self.base).expect("remove fixture base");
        }
    }

    #[test]
    fn first_handoff_burns_nonce_before_return_and_replay_fails_after_reopen() {
        let fixture = Fixture::new();
        assert!(valid_nonce(&fixture.expected.handoff_nonce));
        assert_eq!(
            fixture.verify().expect("first handoff").into_files()["src/lib.rs"],
            b"changed\n"
        );
        assert!(
            fixture.verify().is_err(),
            "reopened ledger must reject replay"
        );
        fixture.cleanup();
    }

    #[test]
    fn competing_handoffs_can_return_bytes_at_most_once() {
        let fixture = Fixture::new();
        let successful = std::thread::scope(|scope| {
            let first = scope.spawn(|| fixture.verify().is_ok());
            let second = scope.spawn(|| fixture.verify().is_ok());
            u8::from(first.join().expect("first verifier"))
                + u8::from(second.join().expect("second verifier"))
        });
        assert_eq!(successful, 1);
        fixture.cleanup();
    }

    #[test]
    fn changed_host_authority_or_worker_binding_returns_no_output() {
        let mut fixture = Fixture::new();
        fixture.authority.session_id = "session-2".into();
        assert!(fixture.verify().is_err());
        fixture.authority.session_id = "session-1".into();
        fixture.authority.host_turn_id = "turn-other".into();
        assert!(fixture.verify().is_err());
        fixture.authority.host_turn_id = "turn-123".into();
        fixture.authority.contract_hash = "c".repeat(64);
        assert!(fixture.verify().is_err());
        fixture.authority.contract_hash = "b".repeat(64);
        fixture.expected.native_turn_id = "turn-other".into();
        fixture.manifest.native_turn_id = "turn-other".into();
        assert!(
            fixture.verify().is_err(),
            "coordinated host expectation and worker manifest change must fail"
        );
        fixture.manifest.native_turn_id = "turn-123".into();
        assert!(fixture.verify().is_err());
        fixture.expected.native_turn_id = "turn-123".into();
        fixture.authority.allowed_paths.insert("other.rs".into());
        assert!(fixture.verify().is_err());
        fixture.authority.allowed_paths.remove("other.rs");
        assert!(
            fixture.verify().is_ok(),
            "failed attempts did not burn nonce"
        );
        fixture.cleanup();
    }

    #[test]
    fn torn_or_existing_burn_marker_is_spent_without_deletion() {
        let fixture = Fixture::new();
        let marker = fixture
            .ledger_path
            .join(burn_name(&fixture.expected.handoff_nonce));
        fs::write(&marker, b"{").expect("simulate crash-residue marker");
        assert!(fixture.verify().is_err());
        assert_eq!(fs::read(&marker).expect("retained marker"), b"{");
        fixture.cleanup();
    }

    #[test]
    fn missing_or_torn_issue_record_never_authorizes_output() {
        let fixture = Fixture::new();
        let issued = fixture
            .ledger_path
            .join(issue_name(&fixture.expected.handoff_nonce));
        fs::write(&issued, b"{").expect("simulate torn issue record");
        assert!(fixture.verify().is_err());
        fs::remove_file(&issued).expect("remove fixture issue record");
        assert!(fixture.verify().is_err());
        fixture.cleanup();
    }

    #[test]
    fn native_turn_binding_is_create_once_and_torn_or_missing_binding_fails_closed() {
        let fixture = Fixture::new();
        let ledger = ProtectedDirectoryGuard::acquire(&fixture.ledger_path, LABEL).expect("ledger");
        let profile = CodexCliTurnProfileEvidenceV1 {
            codex_thread_id: "thread-abc".into(),
            codex_turn_id: "turn-123".into(),
            model_id: SOL_MEDIUM_MODEL.into(),
            reasoning_effort: "medium".into(),
            evidence_sha256: "c".repeat(64),
        };
        assert!(
            bind_sol_worker_native_turn(
                &ledger,
                &fixture.authority,
                &fixture.expected.handoff_nonce,
                "thread-abc",
                &profile,
            )
            .is_err(),
            "identical replay cannot rebind an existing native turn"
        );
        drop(ledger);
        let bound = fixture
            .ledger_path
            .join(bound_name(&fixture.expected.handoff_nonce));
        fs::write(&bound, b"{").expect("simulate torn native turn record");
        assert!(fixture.verify().is_err());
        fs::remove_file(&bound).expect("remove fixture binding");
        assert!(fixture.verify().is_err());
        fixture.cleanup();
    }

    #[test]
    fn native_turn_binding_rejects_wrong_model_or_effort_before_creating_record() {
        let fixture = Fixture::new();
        let bound = fixture
            .ledger_path
            .join(bound_name(&fixture.expected.handoff_nonce));
        fs::remove_file(&bound).expect("remove fixture binding to test fresh bind");
        let ledger = ProtectedDirectoryGuard::acquire(&fixture.ledger_path, LABEL).expect("ledger");
        let mut profile = CodexCliTurnProfileEvidenceV1 {
            codex_thread_id: "thread-abc".into(),
            codex_turn_id: "turn-123".into(),
            model_id: SOL_MEDIUM_MODEL.into(),
            reasoning_effort: "high".into(),
            evidence_sha256: "c".repeat(64),
        };
        assert!(
            bind_sol_worker_native_turn(
                &ledger,
                &fixture.authority,
                &fixture.expected.handoff_nonce,
                "thread-abc",
                &profile,
            )
            .is_err()
        );
        assert!(!bound.exists());
        profile.reasoning_effort = "medium".into();
        profile.model_id = "gpt-5.6-terra".into();
        assert!(
            bind_sol_worker_native_turn(
                &ledger,
                &fixture.authority,
                &fixture.expected.handoff_nonce,
                "thread-abc",
                &profile,
            )
            .is_err()
        );
        assert!(!bound.exists());
        profile.model_id = SOL_MEDIUM_MODEL.into();
        bind_sol_worker_native_turn(
            &ledger,
            &fixture.authority,
            &fixture.expected.handoff_nonce,
            "thread-abc",
            &profile,
        )
        .expect("valid native profile binds once");
        drop(ledger);
        assert!(fixture.verify().is_ok());
        fixture.cleanup();
    }
}
