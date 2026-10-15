//! Private, create-once pre-spawn reservation for the dormant Sol path.
//!
//! A record is always a spent, attention-only state after reopen. There is no
//! terminal handoff record or recovery path that permits a second launch.
//! File and pinned-parent flushes are required before returning the opaque
//! reservation. This does not retroactively prove the older nonce journal's
//! parent-entry durability, hardware power-loss behavior, worker isolation,
//! or any repository authority.

use serde::{Deserialize, Serialize};

#[cfg(test)]
use std::cell::Cell;

use crate::windows_protected_fs::{
    ProtectedDirectoryGuard, flush_pinned_directory_metadata, read_optional_relative_regular,
    read_relative_regular_unique, write_new_regular_in,
};

use super::sol_worker_nonce::{SolWorkerNonceAuthorityV1, read_active_sol_worker_nonce};

const LABEL: &str = "Sol worker launch reservation";
const MAX_RECORD_BYTES: usize = 64 * 1024;

#[cfg(test)]
thread_local! {
    static FAIL_AFTER_CREATE: Cell<bool> = const { Cell::new(false) };
    static FAIL_AFTER_PARENT_FLUSH: Cell<bool> = const { Cell::new(false) };
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct SolLaunchBindingV1 {
    pub authority: SolWorkerNonceAuthorityV1,
    pub nonce: String,
    pub ledger_identity: (u32, u32, u32),
    pub output_identity: (u32, u32, u32),
    pub executable_parent_identity: (u32, u32, u32),
    pub executable_component: String,
    pub executable_object_identity: String,
    pub executable_sha256: String,
    pub launch_input_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LaunchStartedRecordV1 {
    schema_version: u32,
    purpose: String,
    binding: SolLaunchBindingV1,
}

/// Non-cloneable receipt. Possession only permits one private attempt through
/// the native prepared launch function; it never authorizes output handoff.
pub(super) struct SolLaunchReservationV1 {
    bytes: Vec<u8>,
    binding: SolLaunchBindingV1,
}

fn record_name(nonce: &str) -> Result<String, String> {
    // The nonce comes from the verified active record, yet validate it again
    // before using it as a relative component.
    let parsed = uuid::Uuid::parse_str(nonce).map_err(|_| format!("{LABEL} nonce is unsafe"))?;
    if parsed.get_version_num() != 4 || parsed.hyphenated().to_string() != nonce {
        return Err(format!("{LABEL} nonce is unsafe"));
    }
    Ok(format!("launch-started-{nonce}.json"))
}

fn read_exact(ledger: &ProtectedDirectoryGuard, nonce: &str) -> Result<Vec<u8>, String> {
    read_relative_regular_unique(ledger, &record_name(nonce)?, MAX_RECORD_BYTES as u64, LABEL)
}

fn require_active(
    ledger: &ProtectedDirectoryGuard,
    binding: &SolLaunchBindingV1,
) -> Result<(), String> {
    if read_active_sol_worker_nonce(ledger, &binding.authority)? != binding.nonce
        || ledger.physical_identity(LABEL)? != binding.ledger_identity
    {
        return Err(format!("{LABEL} selected turn changed"));
    }
    Ok(())
}

impl SolLaunchReservationV1 {
    pub(super) fn reserve(
        ledger: &ProtectedDirectoryGuard,
        binding: SolLaunchBindingV1,
    ) -> Result<Self, String> {
        require_active(ledger, &binding)?;
        let record = LaunchStartedRecordV1 {
            schema_version: 1,
            purpose: "sol-worker-launch-started".into(),
            binding: binding.clone(),
        };
        let bytes = serde_json::to_vec(&record)
            .map_err(|_| format!("{LABEL} record cannot be serialized"))?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(format!("{LABEL} record is too large"));
        }
        let name = record_name(&binding.nonce)?;
        // FILE_CREATE refuses even a torn or corrupt prior reservation. Once
        // this succeeds, no error path removes the record or retries launch.
        write_new_regular_in(ledger, &name, &bytes, LABEL)?;
        #[cfg(test)]
        if FAIL_AFTER_CREATE.with(|slot| slot.replace(false)) {
            return Err(format!("{LABEL} injected stop after record create"));
        }
        flush_pinned_directory_metadata(ledger, LABEL)?;
        #[cfg(test)]
        if FAIL_AFTER_PARENT_FLUSH.with(|slot| slot.replace(false)) {
            return Err(format!("{LABEL} injected stop after parent flush"));
        }
        if read_exact(ledger, &binding.nonce)? != bytes {
            return Err(format!("{LABEL} readback changed"));
        }
        require_active(ledger, &binding)?;
        Ok(Self { bytes, binding })
    }

    pub(super) fn require_exact(
        &self,
        ledger: &ProtectedDirectoryGuard,
        binding: &SolLaunchBindingV1,
    ) -> Result<(), String> {
        if self.binding != *binding || read_exact(ledger, &binding.nonce)? != self.bytes {
            return Err(format!("{LABEL} binding changed"));
        }
        require_active(ledger, binding)
    }

    pub(super) fn require_unchanged(&self, ledger: &ProtectedDirectoryGuard) -> Result<(), String> {
        self.require_exact(ledger, &self.binding)
    }

    /// Read-only post-run comparison with the objects retained across launch.
    /// Worker output may exist now, so the pre-spawn empty-root check cannot
    /// be repeated. This receipt does not authorize reading or staging output.
    pub(super) fn diagnose_retained_objects(
        &self,
        ledger: &ProtectedDirectoryGuard,
        output: &ProtectedDirectoryGuard,
        executable_parent: &ProtectedDirectoryGuard,
        executable_component: &str,
        executable_object_identity: &str,
        executable_sha256: &str,
    ) -> Result<(), String> {
        self.require_unchanged(ledger)?;
        if output.physical_identity(LABEL)? != self.binding.output_identity
            || executable_parent.physical_identity(LABEL)?
                != self.binding.executable_parent_identity
            || executable_component != self.binding.executable_component
            || executable_object_identity != self.binding.executable_object_identity
            || executable_sha256 != self.binding.executable_sha256
        {
            return Err(format!("{LABEL} retained object changed"));
        }
        self.require_unchanged(ledger)?;
        if output.physical_identity(LABEL)? != self.binding.output_identity
            || executable_parent.physical_identity(LABEL)?
                != self.binding.executable_parent_identity
        {
            return Err(format!("{LABEL} retained directory changed"));
        }
        Ok(())
    }
}

/// Reading after process restart is diagnostic only. Every selected turn is
/// sticky attention, including a missing, torn, or corrupt reservation, until
/// a separate terminal handoff protocol is implemented and proved.
fn reopen_detail(
    ledger: &ProtectedDirectoryGuard,
    authority: &SolWorkerNonceAuthorityV1,
) -> Result<String, String> {
    let nonce = read_active_sol_worker_nonce(ledger, authority)?;
    let name = record_name(&nonce)?;
    let Some(bytes) =
        read_optional_relative_regular(ledger, &name, MAX_RECORD_BYTES as u64, LABEL)?
    else {
        return Ok(format!(
            "{LABEL} selected turn has no launch record; ATTENTION"
        ));
    };
    let record: LaunchStartedRecordV1 =
        serde_json::from_slice(&bytes).map_err(|_| format!("{LABEL} corrupt record; ATTENTION"))?;
    if record.schema_version != 1
        || record.purpose != "sol-worker-launch-started"
        || record.binding.authority != *authority
        || record.binding.nonce != nonce
        || record.binding.ledger_identity != ledger.physical_identity(LABEL)?
        || serde_json::to_vec(&record).map_err(|_| format!("{LABEL} invalid record; ATTENTION"))?
            != bytes
    {
        return Err(format!("{LABEL} mismatched record; ATTENTION"));
    }
    Ok(format!(
        "{LABEL} launch-started without terminal handoff; ATTENTION"
    ))
}

pub(super) fn reopen_attention(
    ledger: &ProtectedDirectoryGuard,
    authority: &SolWorkerNonceAuthorityV1,
) -> String {
    // Even malformed or inaccessible state is attention. No caller can turn
    // an I/O or parsing error into a new launch permission.
    reopen_detail(ledger, authority).unwrap_or_else(|reason| format!("{reason}; ATTENTION"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct Fixture {
        root: PathBuf,
        ledger_path: PathBuf,
        output_path: PathBuf,
        authority: SolWorkerNonceAuthorityV1,
        nonce: String,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("catdesk-sol-launch-{}", Uuid::new_v4()));
            let ledger_path = root.join("ledger");
            let output_path = root.join("output");
            fs::create_dir_all(&ledger_path).expect("create pinned ledger fixture");
            fs::create_dir(&output_path).expect("create output fixture");
            let authority = SolWorkerNonceAuthorityV1 {
                session_id: "session-1".into(),
                task_id: "T-0483".into(),
                host_turn_id: "turn-1".into(),
                contract_hash: "b".repeat(64),
                baseline_commit: "a".repeat(40),
                allowed_paths: BTreeSet::from(["src/lib.rs".into()]),
            };
            let ledger = ProtectedDirectoryGuard::acquire(&ledger_path, LABEL).expect("pin ledger");
            let nonce = super::super::sol_worker_nonce::issue_sol_worker_nonce(&ledger, &authority)
                .expect("select nonce");
            drop(ledger);
            Self {
                root,
                ledger_path,
                output_path,
                authority,
                nonce,
            }
        }

        fn ledger(&self) -> ProtectedDirectoryGuard {
            ProtectedDirectoryGuard::acquire(&self.ledger_path, LABEL).expect("reopen ledger")
        }

        fn binding(&self, ledger: &ProtectedDirectoryGuard) -> SolLaunchBindingV1 {
            let output =
                ProtectedDirectoryGuard::acquire(&self.output_path, LABEL).expect("pin output");
            SolLaunchBindingV1 {
                authority: self.authority.clone(),
                nonce: self.nonce.clone(),
                ledger_identity: ledger.physical_identity(LABEL).expect("ledger identity"),
                output_identity: output.physical_identity(LABEL).expect("output identity"),
                executable_parent_identity: output
                    .physical_identity(LABEL)
                    .expect("fixture image parent"),
                executable_component: "fixture.exe".into(),
                executable_object_identity: "fixture-object".into(),
                executable_sha256: "a".repeat(64),
                launch_input_sha256: "b".repeat(64),
            }
        }

        fn cleanup(self) {
            let target = fs::canonicalize(&self.root).expect("resolve exact fixture root");
            let temp = fs::canonicalize(std::env::temp_dir()).expect("resolve temp root");
            assert_eq!(
                target.parent(),
                Some(temp.as_path()),
                "fixture escaped temp root"
            );
            assert!(
                target
                    .file_name()
                    .expect("fixture component")
                    .to_string_lossy()
                    .starts_with("catdesk-sol-launch-"),
                "fixture name changed"
            );
            fs::remove_dir_all(target).expect("remove checked fixture");
        }
    }

    #[test]
    fn pinned_parent_reopen_flushes_exact_windows_directory_object() {
        let fixture = Fixture::new();
        let ledger = fixture.ledger();
        let expected = ledger
            .physical_identity(LABEL)
            .expect("original directory identity");
        flush_pinned_directory_metadata(&ledger, LABEL).expect("native directory reopen and flush");
        assert_eq!(
            ledger
                .physical_identity(LABEL)
                .expect("identity after flush"),
            expected
        );
        drop(ledger);
        fixture.cleanup();
    }

    #[test]
    fn selected_nonce_without_launch_record_reopens_as_attention() {
        let fixture = Fixture::new();
        let ledger = fixture.ledger();
        assert!(reopen_attention(&ledger, &fixture.authority).contains("ATTENTION"));
        drop(ledger);
        fixture.cleanup();
    }

    #[test]
    fn create_once_reservation_reopens_as_attention_and_never_relaunches() {
        let fixture = Fixture::new();
        let ledger = fixture.ledger();
        let binding = fixture.binding(&ledger);
        let reservation =
            SolLaunchReservationV1::reserve(&ledger, binding.clone()).expect("synced reservation");
        reservation
            .require_exact(&ledger, &binding)
            .expect("readback exact binding");
        drop(reservation);
        drop(ledger);
        let reopened = fixture.ledger();
        assert!(reopen_attention(&reopened, &fixture.authority).contains("ATTENTION"));
        assert!(
            SolLaunchReservationV1::reserve(&reopened, binding).is_err(),
            "create-once collision"
        );
        drop(reopened);
        fixture.cleanup();
    }

    #[test]
    fn failed_create_or_flush_never_returns_spawn_receipt_and_remains_attention() {
        for after_parent_flush in [false, true] {
            let fixture = Fixture::new();
            let ledger = fixture.ledger();
            let binding = fixture.binding(&ledger);
            if after_parent_flush {
                FAIL_AFTER_PARENT_FLUSH.with(|slot| slot.set(true));
            } else {
                FAIL_AFTER_CREATE.with(|slot| slot.set(true));
            }
            assert!(SolLaunchReservationV1::reserve(&ledger, binding.clone()).is_err());
            assert!(reopen_attention(&ledger, &fixture.authority).contains("ATTENTION"));
            assert!(SolLaunchReservationV1::reserve(&ledger, binding).is_err());
            drop(ledger);
            fixture.cleanup();
        }
    }

    #[test]
    fn corrupt_reservation_is_sticky_attention_and_cannot_be_replaced() {
        let fixture = Fixture::new();
        let ledger = fixture.ledger();
        let binding = fixture.binding(&ledger);
        let reservation =
            SolLaunchReservationV1::reserve(&ledger, binding.clone()).expect("reserve");
        drop(reservation);
        fs::write(
            fixture
                .ledger_path
                .join(record_name(&fixture.nonce).expect("name")),
            b"{torn",
        )
        .expect("tamper fixture record");
        let diagnostic = reopen_attention(&ledger, &fixture.authority);
        assert!(diagnostic.contains("corrupt") && diagnostic.contains("ATTENTION"));
        assert!(SolLaunchReservationV1::reserve(&ledger, binding).is_err());
        drop(ledger);
        fixture.cleanup();
    }

    #[test]
    fn concurrent_reservation_has_one_winner() {
        let fixture = Fixture::new();
        let ledger = fixture.ledger();
        let binding = fixture.binding(&ledger);
        drop(ledger);
        let outcomes = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                let ledger = fixture.ledger();
                SolLaunchReservationV1::reserve(&ledger, binding.clone()).is_ok()
            });
            let second = scope.spawn(|| {
                let ledger = fixture.ledger();
                SolLaunchReservationV1::reserve(&ledger, binding.clone()).is_ok()
            });
            [first.join().expect("first"), second.join().expect("second")]
        });
        assert_eq!(outcomes.into_iter().filter(|won| *won).count(), 1);
        let ledger = fixture.ledger();
        assert!(reopen_attention(&ledger, &fixture.authority).contains("ATTENTION"));
        drop(ledger);
        fixture.cleanup();
    }
}
