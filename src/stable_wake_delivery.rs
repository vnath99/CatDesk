//! W13-compatible durable claim/receipt state only. No browser owner exists here.

use crate::stable_wake_core::{
    EventClass, ReviewState, canonical_inbox_records, discover, is_actionable,
    read_protected_wake_target, verify_protected_wake_target, wake_message_sha256,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const STATE_SCHEMA: u32 = 4;
const RECEIPT_SCHEMA: u32 = 1;
const MAX_STATE_BYTES: u64 = 128 * 1024;
const MAX_HISTORY: usize = 128;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DeliveryClassification {
    Unclaimed,
    ClaimedRetryable,
    SubmittingAmbiguous,
    AlreadySent,
    SentOtherTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SubmissionBoundary {
    pub(crate) record_id: String,
    pub(crate) message_sha256: String,
    pub(crate) target_sha256: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DeliveryReceiptV1 {
    pub(crate) record_id: String,
    pub(crate) browser_sent_at_unix: f64,
    pub(crate) message_sha256: String,
    pub(crate) target_sha256: String,
    pub(crate) receipt_schema_version: u32,
}

/// Read-only, fully bound proof of one terminal stable-wake delivery.  This
/// is deliberately derived from both canonical inbox identity and schema-4
/// receipt state; a provider completion, source test, or an unbound `SENT`
/// string cannot construct it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExactWakeDeliveryEvidenceV1 {
    pub(crate) record_id: String,
    pub(crate) project_id: String,
    pub(crate) session_id: String,
    pub(crate) browser_sent_at_unix: f64,
    pub(crate) message_sha256: String,
    pub(crate) target_sha256: String,
    pub(crate) receipt_schema_version: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateV4 {
    schema_version: u32,
    deliveries: Vec<DeliveryV4>,
    #[serde(default)]
    operator_attention: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryV4 {
    record_id: String,
    status: String,
    claimed_at_unix: f64,
    #[serde(default)]
    browser_sent_at_unix: Option<f64>,
    #[serde(default)]
    message_sha256: Option<String>,
    #[serde(default)]
    target_sha256: Option<String>,
    #[serde(default)]
    receipt_schema_version: Option<u32>,
    #[serde(default)]
    attention: Option<String>,
}

pub(crate) struct StableWakeDelivery {
    workspace: PathBuf,
}

impl StableWakeDelivery {
    pub(crate) fn open(workspace: &Path) -> Result<Self, String> {
        let workspace = safe_directory(workspace, "stable wake delivery unavailable")?;
        let control = safe_child_dir(&workspace, ".catdesk")?;
        let _ = safe_child_dir(&control, "wake-bridge")?;
        Ok(Self { workspace })
    }

    pub(crate) fn classify(&self, record_id: &str) -> Result<DeliveryClassification, String> {
        valid_id(record_id)?;
        let target = read_protected_wake_target(&self.workspace)?;
        classify(&self.read_state()?, record_id, &target.target_sha256)
    }

    pub(crate) fn target_binding(&self) -> Result<String, String> {
        Ok(read_protected_wake_target(&self.workspace)?.target_sha256)
    }

    /// Reads one exact terminal delivery proof without claiming, retrying, or
    /// otherwise changing wake state.  The record must occur exactly once in
    /// the canonical CatDesk inbox and retain its completed-final-review,
    /// project, session, message, and current protected-target binding.
    pub(crate) fn read_exact_sent_evidence(
        &self,
        record_id: &str,
    ) -> Result<Option<ExactWakeDeliveryEvidenceV1>, String> {
        valid_id(record_id)?;
        let records = canonical_inbox_records(&self.workspace, "catdesk")?;
        let record = match records
            .iter()
            .filter(|record| record.record_id == record_id)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [] => return Ok(None),
            [record] => *record,
            _ => return Err("stable wake inbox duplicate record".into()),
        };
        if record.project_id != "catdesk"
            || record.state != ReviewState::CompletedVerified
            || record.next_action != "independent_final_review"
        {
            return Err("stable wake record is not terminal review evidence".into());
        }

        let target = read_protected_wake_target(&self.workspace)?;
        let state = self.read_state()?;
        validate_state(&state)?;
        require_no_operator_attention(&state)?;
        let delivery = match state
            .deliveries
            .iter()
            .filter(|delivery| delivery.record_id == record_id)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [] => return Ok(None),
            [delivery] => *delivery,
            _ => return Err("stable wake state duplicate record".into()),
        };
        validate_terminal_receipt(delivery)?;
        if delivery.target_sha256.as_deref() != Some(target.target_sha256.as_str()) {
            return Err("stable wake receipt target drifted".into());
        }
        Ok(Some(ExactWakeDeliveryEvidenceV1 {
            record_id: record.record_id.clone(),
            project_id: record.project_id.clone(),
            session_id: record.session_id.clone(),
            browser_sent_at_unix: delivery
                .browser_sent_at_unix
                .expect("terminal receipt was validated"),
            message_sha256: delivery
                .message_sha256
                .clone()
                .expect("terminal receipt was validated"),
            target_sha256: delivery
                .target_sha256
                .clone()
                .expect("terminal receipt was validated"),
            receipt_schema_version: delivery
                .receipt_schema_version
                .expect("terminal receipt was validated"),
        }))
    }

    /// Read-only cutover gate for the owner selector.  A selector must never
    /// make Rust eligible while legacy delivery evidence is malformed or has
    /// crossed an unresolved submit boundary.
    pub(crate) fn validate_cutover_safe(&self) -> Result<(), String> {
        let _lock = StateLock::acquire(&self.wake_root()?)?;
        let state = self.read_state()?;
        validate_state(&state)?;
        require_no_operator_attention(&state)?;
        if state.deliveries.iter().any(|delivery| {
            delivery.status == "SUBMITTING"
                || (delivery.attention.is_some()
                    && !is_provably_pre_submit_legacy_attention(delivery))
        }) {
            return Err("stable wake state contains unresolved submission authority".into());
        }
        // Retain the canonical parser as the event authority.  This is a
        // read-only conflict/schema gate, not an acknowledgement.
        let _ = canonical_inbox_records(&self.workspace, "catdesk")?;
        Ok(())
    }

    pub(crate) fn claim(
        &self,
        record_id: &str,
        claimed_at_unix: f64,
    ) -> Result<DeliveryClassification, String> {
        Ok(self.claim_with_transition(record_id, claimed_at_unix)?.0)
    }

    fn claim_with_transition(
        &self,
        record_id: &str,
        claimed_at_unix: f64,
    ) -> Result<(DeliveryClassification, bool), String> {
        valid_id(record_id)?;
        positive_time(claimed_at_unix)?;
        let _lock = StateLock::acquire(&self.wake_root()?)?;
        self.require_actionable(record_id)?;
        let mut state = self.read_state()?;
        require_no_operator_attention(&state)?;
        match self.classify_existing(&state, record_id)? {
            DeliveryClassification::Unclaimed => {
                self.compact_before_claim(&mut state)?;
                state.deliveries.push(DeliveryV4 {
                    record_id: record_id.into(),
                    status: "CLAIMED".into(),
                    claimed_at_unix,
                    browser_sent_at_unix: None,
                    message_sha256: None,
                    target_sha256: None,
                    receipt_schema_version: None,
                    attention: None,
                });
                self.write_state(&state)?;
                Ok((DeliveryClassification::ClaimedRetryable, true))
            }
            result => Ok((result, false)),
        }
    }

    /// This persists the one-way submit boundary. It intentionally invokes no browser.
    pub(crate) fn begin_submitting(
        &self,
        record_id: &str,
        expected_target_sha256: &str,
    ) -> Result<SubmissionBoundary, String> {
        valid_id(record_id)?;
        let _lock = StateLock::acquire(&self.wake_root()?)?;
        self.require_actionable(record_id)?;
        if !sha(expected_target_sha256) {
            return Err("stable wake target binding unsafe".into());
        }
        let target = verify_protected_wake_target(&self.workspace, expected_target_sha256)?;
        let mut state = self.read_state()?;
        require_no_operator_attention(&state)?;
        if self.classify_existing(&state, record_id)? != DeliveryClassification::ClaimedRetryable {
            return Err("stable wake delivery is not cleanly claimed".into());
        }
        let delivery = unique_mut(&mut state, record_id)?;
        delivery.status = "SUBMITTING".into();
        self.write_state(&state)?;
        Ok(SubmissionBoundary {
            record_id: record_id.into(),
            message_sha256: wake_message_sha256(record_id),
            target_sha256: target.target_sha256,
        })
    }

    pub(crate) fn record_receipt(
        &self,
        receipt: &DeliveryReceiptV1,
    ) -> Result<DeliveryClassification, String> {
        valid_receipt_shape(receipt)?;
        let _lock = StateLock::acquire(&self.wake_root()?)?;
        let target = read_protected_wake_target(&self.workspace)?;
        if receipt.target_sha256 != target.target_sha256
            || receipt.message_sha256 != wake_message_sha256(&receipt.record_id)
        {
            return Err("stable wake receipt does not bind exact target/message".into());
        }
        let mut state = self.read_state()?;
        require_no_operator_attention(&state)?;
        let delivery = unique_mut(&mut state, &receipt.record_id)?;
        if delivery.status == "SENT" {
            return classify(&state, &receipt.record_id, &target.target_sha256);
        }
        if delivery.status != "SUBMITTING" {
            return Err("stable wake receipt lacks submit boundary".into());
        }
        if receipt.browser_sent_at_unix < delivery.claimed_at_unix {
            return Err("stable wake receipt timestamp unsafe".into());
        }
        delivery.status = "SENT".into();
        delivery.browser_sent_at_unix = Some(receipt.browser_sent_at_unix);
        delivery.message_sha256 = Some(receipt.message_sha256.clone());
        delivery.target_sha256 = Some(receipt.target_sha256.clone());
        delivery.receipt_schema_version = Some(receipt.receipt_schema_version);
        delivery.attention = None;
        state.operator_attention = None;
        self.write_state(&state)?;
        Ok(DeliveryClassification::AlreadySent)
    }

    /// Only a trusted owner may restore a clean CLAIMED state after a bounded
    /// adapter result proves no submit boundary was crossed.
    pub(crate) fn return_to_clean_claim(&self, record_id: &str) -> Result<(), String> {
        valid_id(record_id)?;
        let _lock = StateLock::acquire(&self.wake_root()?)?;
        let mut state = self.read_state()?;
        require_no_operator_attention(&state)?;
        let delivery = unique_mut(&mut state, record_id)?;
        if delivery.status != "SUBMITTING" {
            return Err("stable wake delivery lacks submit boundary".into());
        }
        delivery.status = "CLAIMED".into();
        delivery.attention = None;
        self.write_state(&state)
    }

    /// Preserve post-submit uncertainty as durable non-retryable authority.
    pub(crate) fn note_operator_attention(
        &self,
        record_id: &str,
        attention: &str,
    ) -> Result<(), String> {
        valid_id(record_id)?;
        if !valid_attention(attention) {
            return Err("stable wake attention unsafe".into());
        }
        let _lock = StateLock::acquire(&self.wake_root()?)?;
        let mut state = self.read_state()?;
        let delivery = unique_mut(&mut state, record_id)?;
        if delivery.status != "SUBMITTING" {
            return Err("stable wake delivery lacks submit boundary".into());
        }
        delivery.attention = Some(attention.into());
        state.operator_attention = Some(attention.into());
        self.write_state(&state)
    }

    fn wake_root(&self) -> Result<PathBuf, String> {
        safe_child_dir(&safe_child_dir(&self.workspace, ".catdesk")?, "wake-bridge")
    }
    fn require_actionable(&self, record_id: &str) -> Result<(), String> {
        let events = discover(&self.workspace, "catdesk")?;
        if events
            .iter()
            .any(|e| matches!(e, EventClass::Pending(record) if record.record_id == record_id))
        {
            Ok(())
        } else {
            Err("stable wake record is not actionable".into())
        }
    }
    fn classify_existing(
        &self,
        state: &StateV4,
        record_id: &str,
    ) -> Result<DeliveryClassification, String> {
        let target = read_protected_wake_target(&self.workspace)?;
        classify(state, record_id, &target.target_sha256)
    }
    fn read_state(&self) -> Result<StateV4, String> {
        let root = self.wake_root()?;
        let path = root.join("state.json");
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(empty_state()),
            Err(_) => Err("stable wake state unavailable".into()),
            Ok(meta) => {
                if !meta.is_file() || unsafe_link(&meta) || meta.len() > MAX_STATE_BYTES {
                    return Err("stable wake state unsafe".into());
                }
                let canonical = path
                    .canonicalize()
                    .map_err(|_| "stable wake state unavailable".to_string())?;
                if canonical != path {
                    return Err("stable wake state unsafe".into());
                }
                let bytes =
                    fs::read(&path).map_err(|_| "stable wake state unavailable".to_string())?;
                parse_state(&bytes)
            }
        }
    }
    fn write_state(&self, state: &StateV4) -> Result<(), String> {
        validate_state(state)?;
        atomic_write(&self.wake_root()?, state)
    }

    fn compact_before_claim(&self, state: &mut StateV4) -> Result<(), String> {
        if state.deliveries.len() < MAX_HISTORY {
            return Ok(());
        }
        if let Some(removable) = state
            .deliveries
            .iter()
            .position(|delivery| delivery.status == "CLAIMED" && delivery.attention.is_none())
        {
            state.deliveries.remove(removable);
            return Ok(());
        }

        // The inbox parse is intentionally fail-closed. Its raw cardinality
        // is preserved so exact duplicate record entries cannot masquerade as
        // one acknowledged terminal authority.
        let canonical = canonical_inbox_records(&self.workspace, "catdesk")?;
        for delivery in &state.deliveries {
            if delivery.status == "SENT" {
                validate_terminal_receipt(delivery)?;
            }
        }
        let removable = state.deliveries.iter().position(|delivery| {
            if delivery.status != "SENT" {
                return false;
            }
            let matches = canonical
                .iter()
                .filter(|record| record.record_id == delivery.record_id)
                .collect::<Vec<_>>();
            matches.len() == 1 && (!matches[0].unread || !is_actionable(matches[0]))
        });
        let Some(removable) = removable else {
            return Err("stable wake history retains unresolved delivery".into());
        };
        state.deliveries.remove(removable);
        Ok(())
    }

    /// Test-only process probe support. This exercises the same kernel lock as
    /// production transitions, but performs no state or browser operation.
    #[doc(hidden)]
    pub(crate) fn acquire_lock_for_probe(workspace: &Path) -> Result<DeliveryLockProbe, String> {
        let delivery = Self::open(workspace)?;
        Ok(DeliveryLockProbe {
            _lock: StateLock::acquire(&delivery.wake_root()?)?,
        })
    }

    #[doc(hidden)]
    pub(crate) fn claim_for_probe(workspace: &Path, record_id: &str) -> Result<bool, String> {
        Self::open(workspace)?
            .claim_with_transition(record_id, 1.0)
            .map(|(_, created)| created)
    }
}

fn empty_state() -> StateV4 {
    StateV4 {
        schema_version: STATE_SCHEMA,
        deliveries: vec![],
        operator_attention: None,
    }
}
fn unique_mut<'a>(state: &'a mut StateV4, record: &str) -> Result<&'a mut DeliveryV4, String> {
    let index = state
        .deliveries
        .iter()
        .position(|d| d.record_id == record)
        .ok_or_else(|| "stable wake delivery is unclaimed".to_string())?;
    if state
        .deliveries
        .iter()
        .filter(|d| d.record_id == record)
        .count()
        != 1
    {
        return Err("stable wake state duplicate record".into());
    }
    Ok(&mut state.deliveries[index])
}
fn classify(state: &StateV4, record: &str, target: &str) -> Result<DeliveryClassification, String> {
    validate_state(state)?;
    require_no_operator_attention(state)?;
    let matches = state
        .deliveries
        .iter()
        .filter(|d| d.record_id == record)
        .collect::<Vec<_>>();
    let d = match matches.as_slice() {
        [] => return Ok(DeliveryClassification::Unclaimed),
        [d] => *d,
        _ => return Err("stable wake state duplicate record".into()),
    };
    match d.status.as_str() {
        "CLAIMED" => Ok(DeliveryClassification::ClaimedRetryable),
        "SUBMITTING" | "OPERATOR_ATTENTION" => Ok(DeliveryClassification::SubmittingAmbiguous),
        "SENT" if d.target_sha256.as_deref() == Some(target) => {
            Ok(DeliveryClassification::AlreadySent)
        }
        "SENT" => Ok(DeliveryClassification::SentOtherTarget),
        _ => Err("stable wake state unsafe".into()),
    }
}
fn require_no_operator_attention(state: &StateV4) -> Result<(), String> {
    if state.operator_attention.is_some() {
        Err("stable wake state requires operator attention".into())
    } else {
        Ok(())
    }
}

/// This exception is intentionally narrower than ordinary delivery
/// classification. It is a read-only activation compatibility predicate for
/// historical legacy entries that stopped before `before_submit()`. Such a
/// record remains `SubmittingAmbiguous` for its own record ID; it is never
/// reclaimed, retried, compacted, or otherwise normalized here.
fn is_provably_pre_submit_legacy_attention(delivery: &DeliveryV4) -> bool {
    delivery.status == "OPERATOR_ATTENTION"
        && matches!(
            delivery.attention.as_deref(),
            Some("CHATGPT_NOT_IDLE" | "LOGIN_OR_PROFILE_REQUIRED")
        )
        && delivery.browser_sent_at_unix.is_none()
        && delivery.message_sha256.is_none()
        && delivery.target_sha256.is_none()
        && delivery.receipt_schema_version.is_none()
}

fn validate_terminal_receipt(delivery: &DeliveryV4) -> Result<(), String> {
    validate_delivery(delivery)?;
    if delivery.status != "SENT"
        || delivery.message_sha256.as_deref() != Some(&wake_message_sha256(&delivery.record_id))
    {
        return Err("stable wake terminal receipt unsafe".into());
    }
    Ok(())
}
fn validate_state(state: &StateV4) -> Result<(), String> {
    if state.schema_version != STATE_SCHEMA
        || state.deliveries.len() > MAX_HISTORY
        || !valid_attention_opt(&state.operator_attention)
    {
        return Err("stable wake state unsafe".into());
    }
    let mut ids = BTreeSet::new();
    for d in &state.deliveries {
        validate_delivery(d)?;
        if !ids.insert(&d.record_id) {
            return Err("stable wake state duplicate record".into());
        }
    }
    Ok(())
}
fn validate_delivery(d: &DeliveryV4) -> Result<(), String> {
    valid_id(&d.record_id)?;
    positive_time(d.claimed_at_unix)?;
    let empty = d.browser_sent_at_unix.is_none()
        && d.message_sha256.is_none()
        && d.target_sha256.is_none()
        && d.receipt_schema_version.is_none();
    match d.status.as_str() {
        "CLAIMED" if empty && d.attention.is_none() => Ok(()),
        "SUBMITTING" if empty && valid_attention_opt(&d.attention) => Ok(()),
        "OPERATOR_ATTENTION" if empty && d.attention.as_deref().is_some_and(valid_attention) => {
            Ok(())
        }
        "SENT"
            if d.attention.is_none()
                && d.browser_sent_at_unix
                    .is_some_and(|v| v.is_finite() && v > 0.0 && v >= d.claimed_at_unix)
                && d.receipt_schema_version == Some(RECEIPT_SCHEMA)
                && d.message_sha256.as_deref().is_some_and(sha)
                && d.target_sha256.as_deref().is_some_and(sha) =>
        {
            Ok(())
        }
        _ => Err("stable wake state unsafe".into()),
    }
}
fn valid_receipt_shape(r: &DeliveryReceiptV1) -> Result<(), String> {
    valid_id(&r.record_id)?;
    positive_time(r.browser_sent_at_unix)?;
    if r.receipt_schema_version != RECEIPT_SCHEMA
        || !sha(&r.message_sha256)
        || !sha(&r.target_sha256)
    {
        return Err("stable wake receipt unsafe".into());
    }
    Ok(())
}
fn parse_state(bytes: &[u8]) -> Result<StateV4, String> {
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err("stable wake state unsafe".into());
    }
    let state: StateV4 =
        serde_json::from_slice(bytes).map_err(|_| "stable wake state malformed".to_string())?;
    validate_state(&state)?;
    Ok(state)
}

fn atomic_write(root: &Path, state: &StateV4) -> Result<(), String> {
    let root = safe_directory(root, "stable wake state unavailable")?;
    let path = root.join("state.json");
    let before = identity(&path)?;
    let temp = root.join(format!(
        ".state.{}.{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let bytes =
        serde_json::to_vec_pretty(state).map_err(|_| "stable wake state unsafe".to_string())?;
    let result = (|| {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|_| "stable wake state unavailable".to_string())?;
        f.write_all(&bytes)
            .and_then(|_| f.write_all(b"\n"))
            .and_then(|_| f.sync_all())
            .map_err(|_| "stable wake state unavailable".to_string())?;
        drop(f);
        if identity(&path)? != before {
            return Err("stable wake state changed concurrently".into());
        }
        fs::rename(&temp, &path).map_err(|_| "stable wake state unavailable".to_string())
    })();
    if temp.exists() {
        let _ = fs::remove_file(&temp);
    }
    result
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    len: u64,
    modified: Option<std::time::SystemTime>,
}
fn identity(path: &Path) -> Result<Option<Identity>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("stable wake state unavailable".into()),
        Ok(m) if !m.is_file() || unsafe_link(&m) => Err("stable wake state unsafe".into()),
        Ok(m) => Ok(Some(Identity {
            len: m.len(),
            modified: m.modified().ok(),
        })),
    }
}
#[cfg(windows)]
struct StateLock {
    handle: *mut core::ffi::c_void,
}
#[cfg(not(windows))]
struct StateLock {
    _process_local: std::sync::MutexGuard<'static, ()>,
}
#[doc(hidden)]
pub(crate) struct DeliveryLockProbe {
    _lock: StateLock,
}
impl StateLock {
    fn acquire(root: &Path) -> Result<Self, String> {
        let root = safe_directory(root, "stable wake state unavailable")?;
        let path = root.join("state.lock");
        // This entry is not lock authority. It is only rejected when present
        // and unsafe so a hostile link/special object cannot become accepted
        // state. The kernel object below is released on process death.
        if let Some(identity) = identity(&path)?
            && identity.len > MAX_STATE_BYTES
        {
            return Err("stable wake state unsafe".into());
        }
        #[cfg(windows)]
        {
            let name = kernel_mutex_name(&root);
            let wide = name
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 0, wide.as_ptr()) };
            if handle.is_null() {
                return Err("stable wake state unavailable".into());
            }
            // WAIT_ABANDONED is safe to take only because all writable state
            // is atomic old-or-new and every transition reparses it fail-closed.
            let wait = unsafe { WaitForSingleObject(handle, 0) };
            if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
                unsafe { CloseHandle(handle) };
                return Err(if wait == WAIT_TIMEOUT {
                    "stable wake state busy".into()
                } else {
                    "stable wake state unavailable".into()
                });
            }
            Ok(Self { handle })
        }
        #[cfg(not(windows))]
        {
            static PROCESS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let guard = PROCESS_LOCK
                .try_lock()
                .map_err(|_| "stable wake state busy".to_string())?;
            Ok(Self {
                _process_local: guard,
            })
        }
    }
}
#[cfg(windows)]
impl Drop for StateLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.handle);
            CloseHandle(self.handle);
        }
    }
}
#[cfg(windows)]
const WAIT_OBJECT_0: u32 = 0;
#[cfg(windows)]
const WAIT_ABANDONED: u32 = 0x80;
#[cfg(windows)]
const WAIT_TIMEOUT: u32 = 0x102;
#[cfg(windows)]
unsafe extern "system" {
    fn CreateMutexW(
        attributes: *mut core::ffi::c_void,
        initial_owner: i32,
        name: *const u16,
    ) -> *mut core::ffi::c_void;
    fn WaitForSingleObject(handle: *mut core::ffi::c_void, milliseconds: u32) -> u32;
    fn ReleaseMutex(handle: *mut core::ffi::c_void) -> i32;
    fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
}
#[cfg(windows)]
fn kernel_mutex_name(root: &Path) -> String {
    let digest = Sha256::digest(root.as_os_str().to_string_lossy().as_bytes());
    format!("Local\\CatDeskStableWakeState-{digest:x}")
}
fn safe_directory(path: &Path, unavailable: &str) -> Result<PathBuf, String> {
    let m = fs::symlink_metadata(path).map_err(|_| unavailable.to_string())?;
    if !m.is_dir() || unsafe_link(&m) {
        return Err("stable wake state unsafe".into());
    }
    path.canonicalize().map_err(|_| unavailable.to_string())
}
fn safe_child_dir(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let path = parent.join(name);
    let m = fs::symlink_metadata(&path).map_err(|_| "stable wake state unavailable".to_string())?;
    if !m.is_dir() || unsafe_link(&m) {
        return Err("stable wake state unsafe".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "stable wake state unavailable".to_string())?;
    if !canonical.starts_with(parent) {
        return Err("stable wake state unsafe".into());
    }
    Ok(canonical)
}
fn unsafe_link(m: &fs::Metadata) -> bool {
    if m.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}
fn valid_id(v: &str) -> Result<(), String> {
    if v.is_empty()
        || v.len() > 128
        || !v
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        Err("stable wake identity unsafe".into())
    } else {
        Ok(())
    }
}
fn positive_time(v: f64) -> Result<(), String> {
    if v.is_finite() && v > 0.0 {
        Ok(())
    } else {
        Err("stable wake timestamp unsafe".into())
    }
}
fn sha(v: &str) -> bool {
    v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit())
}
fn valid_attention(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 128
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn valid_attention_opt(v: &Option<String>) -> bool {
    v.as_deref().is_none_or(valid_attention)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stable_wake_core::{ReviewInboxRecord, ReviewState};
    use std::{
        sync::Arc,
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };
    fn root(name: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let r = std::env::temp_dir().join(format!("catdesk-delivery-{name}-{n}"));
        fs::create_dir_all(r.join(".catdesk/autonomy")).unwrap();
        fs::create_dir_all(r.join(".catdesk/wake-bridge")).unwrap();
        fs::write(r.join(".catdesk/wake-bridge/config.json"), r#"{"conversation_url":"https://chatgpt.com/c/exact-thread","profile_dir":".catdesk/wake-bridge/browser-profile"}"#).unwrap();
        r
    }
    fn inbox(root: &Path, unread: bool) {
        inbox_records(root, &["review_1"], unread);
    }
    fn inbox_records(root: &Path, record_ids: &[&str], unread: bool) {
        let records = record_ids
            .iter()
            .map(|record_id| ReviewInboxRecord {
                schema_version: 1,
                record_id: (*record_id).into(),
                project_id: "catdesk".into(),
                session_id: "session_1".into(),
                state: ReviewState::CompletedVerified,
                next_action: "independent_final_review".into(),
                reference: "artifacts/completion.json".into(),
                created_at_unix: 1,
                unread,
            })
            .collect::<Vec<_>>();
        write_inbox_records(root, &records);
    }
    fn write_inbox_records(root: &Path, records: &[ReviewInboxRecord]) {
        fs::write(
            root.join(".catdesk/autonomy/review-inbox.json"),
            serde_json::to_vec(&records).unwrap(),
        )
        .unwrap();
    }
    fn record(record_id: &str, unread: bool) -> ReviewInboxRecord {
        ReviewInboxRecord {
            schema_version: 1,
            record_id: record_id.into(),
            project_id: "catdesk".into(),
            session_id: "session_1".into(),
            state: ReviewState::CompletedVerified,
            next_action: "independent_final_review".into(),
            reference: "artifacts/completion.json".into(),
            created_at_unix: 1,
            unread,
        }
    }
    fn sent(record_id: &str) -> DeliveryV4 {
        DeliveryV4 {
            record_id: record_id.into(),
            status: "SENT".into(),
            claimed_at_unix: 1.0,
            browser_sent_at_unix: Some(2.0),
            message_sha256: Some(wake_message_sha256(record_id)),
            target_sha256: Some("0".repeat(64)),
            receipt_schema_version: Some(RECEIPT_SCHEMA),
            attention: None,
        }
    }

    fn legacy_pre_submit_attention(record_id: &str, attention: &str) -> DeliveryV4 {
        DeliveryV4 {
            record_id: record_id.into(),
            status: "OPERATOR_ATTENTION".into(),
            claimed_at_unix: 1.0,
            browser_sent_at_unix: None,
            message_sha256: None,
            target_sha256: None,
            receipt_schema_version: None,
            attention: Some(attention.into()),
        }
    }

    #[test]
    fn cutover_ignores_only_exact_legacy_pre_submit_attention_without_reclassifying_it() {
        let r = root("legacy-pre-submit-cutover");
        let ids = [
            "legacy_idle_1",
            "legacy_login_1",
            "legacy_idle_2",
            "legacy_login_2",
            "legacy_idle_3",
            "legacy_login_3",
            "legacy_idle_4",
        ];
        inbox_records(&r, &ids, true);
        let delivery = StableWakeDelivery::open(&r).unwrap();
        let state = StateV4 {
            schema_version: STATE_SCHEMA,
            deliveries: ids
                .iter()
                .enumerate()
                .map(|(index, record_id)| {
                    legacy_pre_submit_attention(
                        record_id,
                        if index % 2 == 0 {
                            "CHATGPT_NOT_IDLE"
                        } else {
                            "LOGIN_OR_PROFILE_REQUIRED"
                        },
                    )
                })
                .collect(),
            operator_attention: None,
        };
        let state_path = r.join(".catdesk/wake-bridge/state.json");
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
        let before_state = fs::read(&state_path).unwrap();
        let before_inbox = fs::read(r.join(".catdesk/autonomy/review-inbox.json")).unwrap();
        let before_config = fs::read(r.join(".catdesk/wake-bridge/config.json")).unwrap();

        assert!(delivery.validate_cutover_safe().is_ok());
        assert_eq!(fs::read(&state_path).unwrap(), before_state);
        assert_eq!(
            fs::read(r.join(".catdesk/autonomy/review-inbox.json")).unwrap(),
            before_inbox
        );
        assert_eq!(
            fs::read(r.join(".catdesk/wake-bridge/config.json")).unwrap(),
            before_config
        );
        for record_id in ids {
            assert_eq!(
                delivery.classify(record_id).unwrap(),
                DeliveryClassification::SubmittingAmbiguous
            );
            assert_eq!(
                delivery.claim(record_id, 2.0).unwrap(),
                DeliveryClassification::SubmittingAmbiguous
            );
        }

        for attention in ["CHATGPT_NOT_IDLE", "LOGIN_OR_PROFILE_REQUIRED"] {
            for evidence in 0..4 {
                let mut entry = legacy_pre_submit_attention("negative_attention", attention);
                match evidence {
                    0 => entry.browser_sent_at_unix = Some(2.0),
                    1 => entry.message_sha256 = Some("0".repeat(64)),
                    2 => entry.target_sha256 = Some("0".repeat(64)),
                    3 => entry.receipt_schema_version = Some(RECEIPT_SCHEMA),
                    _ => unreachable!(),
                }
                fs::write(
                    &state_path,
                    serde_json::to_vec(&StateV4 {
                        schema_version: STATE_SCHEMA,
                        deliveries: vec![entry],
                        operator_attention: None,
                    })
                    .unwrap(),
                )
                .unwrap();
                assert!(delivery.validate_cutover_safe().is_err());
            }
        }

        for entry in [
            legacy_pre_submit_attention("unknown_attention", "POST_SUBMIT_UNKNOWN"),
            DeliveryV4 {
                record_id: "malformed_attention".into(),
                status: "OPERATOR_ATTENTION".into(),
                claimed_at_unix: 1.0,
                browser_sent_at_unix: None,
                message_sha256: None,
                target_sha256: None,
                receipt_schema_version: None,
                attention: None,
            },
            DeliveryV4 {
                record_id: "submitting".into(),
                status: "SUBMITTING".into(),
                claimed_at_unix: 1.0,
                browser_sent_at_unix: None,
                message_sha256: None,
                target_sha256: None,
                receipt_schema_version: None,
                attention: Some("SUBMIT_RECEIPT_UNPROVEN".into()),
            },
        ] {
            fs::write(
                &state_path,
                serde_json::to_vec(&StateV4 {
                    schema_version: STATE_SCHEMA,
                    deliveries: vec![entry],
                    operator_attention: None,
                })
                .unwrap(),
            )
            .unwrap();
            assert!(delivery.validate_cutover_safe().is_err());
        }
        fs::write(
            &state_path,
            br#"{"schema_version":4,"deliveries":[{"record_id":"conflict","status":"CLAIMED","claimed_at_unix":1},{"record_id":"conflict","status":"CLAIMED","claimed_at_unix":1}],"operator_attention":null}"#,
        )
        .unwrap();
        assert!(delivery.validate_cutover_safe().is_err());
        fs::write(&state_path, b"{bad").unwrap();
        assert!(delivery.validate_cutover_safe().is_err());
        fs::remove_dir_all(r).unwrap();
    }
    #[test]
    fn claim_submit_receipt_restart_and_immutable_inputs() {
        let r = root("flow");
        inbox(&r, true);
        let before_inbox = fs::read(r.join(".catdesk/autonomy/review-inbox.json")).unwrap();
        let before_config = fs::read(r.join(".catdesk/wake-bridge/config.json")).unwrap();
        let d = StableWakeDelivery::open(&r).unwrap();
        assert_eq!(
            d.claim("review_1", 1.0).unwrap(),
            DeliveryClassification::ClaimedRetryable
        );
        assert_eq!(
            StableWakeDelivery::open(&r)
                .unwrap()
                .classify("review_1")
                .unwrap(),
            DeliveryClassification::ClaimedRetryable
        );
        let binding = d.target_binding().unwrap();
        let b = d.begin_submitting("review_1", &binding).unwrap();
        assert_eq!(
            d.classify("review_1").unwrap(),
            DeliveryClassification::SubmittingAmbiguous
        );
        assert_eq!(
            d.record_receipt(&DeliveryReceiptV1 {
                record_id: "review_1".into(),
                browser_sent_at_unix: 2.0,
                message_sha256: b.message_sha256,
                target_sha256: b.target_sha256,
                receipt_schema_version: 1
            })
            .unwrap(),
            DeliveryClassification::AlreadySent
        );
        assert_eq!(
            StableWakeDelivery::open(&r)
                .unwrap()
                .classify("review_1")
                .unwrap(),
            DeliveryClassification::AlreadySent
        );
        let before_state = fs::read(r.join(".catdesk/wake-bridge/state.json")).unwrap();
        let evidence = StableWakeDelivery::open(&r)
            .unwrap()
            .read_exact_sent_evidence("review_1")
            .expect("read-only evidence")
            .expect("terminal receipt proof");
        assert_eq!(evidence.project_id, "catdesk");
        assert_eq!(evidence.session_id, "session_1");
        assert_eq!(evidence.receipt_schema_version, 1);
        assert!(evidence.browser_sent_at_unix > 0.0);
        assert_eq!(
            fs::read(r.join(".catdesk/wake-bridge/state.json")).unwrap(),
            before_state,
            "evidence read must not mutate schema-4 state"
        );
        assert_eq!(
            fs::read(r.join(".catdesk/autonomy/review-inbox.json")).unwrap(),
            before_inbox
        );
        assert_eq!(
            fs::read(r.join(".catdesk/wake-bridge/config.json")).unwrap(),
            before_config
        );
        fs::remove_dir_all(r).unwrap();
    }
    #[test]
    fn stale_drift_conflict_and_unsafe_state_fail_closed() {
        let r = root("negative");
        inbox(&r, true);
        let d = StableWakeDelivery::open(&r).unwrap();
        let binding = d.target_binding().unwrap();
        d.claim("review_1", 1.0).unwrap();
        inbox(&r, false);
        assert!(d.begin_submitting("review_1", &binding).is_err());
        inbox(&r, true);
        fs::write(r.join(".catdesk/wake-bridge/config.json"), r#"{"conversation_url":"https://chatgpt.com/c/other","profile_dir":".catdesk/wake-bridge/browser-profile"}"#).unwrap();
        assert!(d.begin_submitting("review_1", &binding).is_err());
        let duplicate = r#"{"schema_version":4,"deliveries":[{"record_id":"review_1","status":"CLAIMED","claimed_at_unix":1},{"record_id":"review_1","status":"CLAIMED","claimed_at_unix":1}],"operator_attention":null}"#;
        fs::write(r.join(".catdesk/wake-bridge/state.json"), duplicate).unwrap();
        assert!(d.classify("review_1").is_err());
        fs::remove_dir_all(r).unwrap();
    }
    #[test]
    fn concurrent_claims_have_one_durable_record() {
        let r = Arc::new(root("concurrent"));
        inbox(&r, true);
        let a = r.clone();
        let b = r.clone();
        let one =
            thread::spawn(move || StableWakeDelivery::open(&a).unwrap().claim("review_1", 1.0));
        let two =
            thread::spawn(move || StableWakeDelivery::open(&b).unwrap().claim("review_1", 1.0));
        let _ = one.join().unwrap();
        let _ = two.join().unwrap();
        let state = StableWakeDelivery::open(&r).unwrap().read_state().unwrap();
        assert_eq!(
            state
                .deliveries
                .iter()
                .filter(|d| d.record_id == "review_1")
                .count(),
            1
        );
        fs::remove_dir_all(&*r).unwrap();
    }
    #[test]
    fn source_has_no_second_submit_owner() {
        let source = include_str!("stable_wake_delivery.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        for forbidden in [
            "selenium",
            "CDP",
            "Command::new",
            "openai_tunnel",
            "daemon_reload",
            "reviewed-release",
        ] {
            assert!(
                !production
                    .to_ascii_lowercase()
                    .contains(&forbidden.to_ascii_lowercase())
            );
        }
    }

    #[test]
    fn receipt_and_history_compatibility_fail_closed_or_classify_other_target() {
        let r = root("receipt-history");
        inbox(&r, true);
        let d = StableWakeDelivery::open(&r).unwrap();
        let other_target = "0".repeat(64);
        let sent = StateV4 {
            schema_version: STATE_SCHEMA,
            deliveries: vec![DeliveryV4 {
                record_id: "review_1".into(),
                status: "SENT".into(),
                claimed_at_unix: 1.0,
                browser_sent_at_unix: Some(2.0),
                message_sha256: Some(wake_message_sha256("review_1")),
                target_sha256: Some(other_target),
                receipt_schema_version: Some(1),
                attention: None,
            }],
            operator_attention: None,
        };
        fs::write(
            r.join(".catdesk/wake-bridge/state.json"),
            serde_json::to_vec(&sent).unwrap(),
        )
        .unwrap();
        assert_eq!(
            d.classify("review_1").unwrap(),
            DeliveryClassification::SentOtherTarget
        );
        let invalid = DeliveryReceiptV1 {
            record_id: "review_1".into(),
            browser_sent_at_unix: 0.0,
            message_sha256: "0".repeat(64),
            target_sha256: "0".repeat(64),
            receipt_schema_version: 1,
        };
        assert!(d.record_receipt(&invalid).is_err());
        let mut too_many = empty_state();
        for i in 0..=MAX_HISTORY {
            too_many.deliveries.push(DeliveryV4 {
                record_id: format!("review_{i}"),
                status: "CLAIMED".into(),
                claimed_at_unix: 1.0,
                browser_sent_at_unix: None,
                message_sha256: None,
                target_sha256: None,
                receipt_schema_version: None,
                attention: None,
            });
        }
        fs::write(
            r.join(".catdesk/wake-bridge/state.json"),
            serde_json::to_vec(&too_many).unwrap(),
        )
        .unwrap();
        assert!(d.classify("review_1").is_err());
        fs::write(r.join(".catdesk/wake-bridge/state.json"), b"{bad").unwrap();
        assert!(d.classify("review_1").is_err());
        fs::remove_dir_all(r).unwrap();
    }

    #[test]
    fn unsafe_final_state_path_and_submitting_restart_refuse() {
        let r = root("path-restart");
        inbox(&r, true);
        let d = StableWakeDelivery::open(&r).unwrap();
        let binding = d.target_binding().unwrap();
        d.claim("review_1", 1.0).unwrap();
        d.begin_submitting("review_1", &binding).unwrap();
        assert_eq!(
            StableWakeDelivery::open(&r)
                .unwrap()
                .classify("review_1")
                .unwrap(),
            DeliveryClassification::SubmittingAmbiguous
        );
        assert!(d.begin_submitting("review_1", &binding).is_err());
        fs::remove_file(r.join(".catdesk/wake-bridge/state.json")).unwrap();
        fs::create_dir(r.join(".catdesk/wake-bridge/state.json")).unwrap();
        assert!(d.classify("review_1").is_err());
        fs::remove_dir_all(r).unwrap();
    }

    #[test]
    fn submitting_and_sent_authority_survive_129_later_claims() {
        let r = root("bounded-ambiguity");
        let ids = (0..=131)
            .map(|index| format!("review_{index}"))
            .collect::<Vec<_>>();
        let id_refs = ids.iter().map(String::as_str).collect::<Vec<_>>();
        inbox_records(&r, &id_refs, true);
        let delivery = StableWakeDelivery::open(&r).unwrap();
        delivery.claim("review_0", 1.0).unwrap();
        let binding = delivery.target_binding().unwrap();
        delivery.begin_submitting("review_0", &binding).unwrap();

        delivery.claim("review_1", 2.0).unwrap();
        let sent_boundary = delivery.begin_submitting("review_1", &binding).unwrap();
        assert_eq!(
            delivery
                .record_receipt(&DeliveryReceiptV1 {
                    record_id: "review_1".into(),
                    browser_sent_at_unix: 3.0,
                    message_sha256: sent_boundary.message_sha256,
                    target_sha256: sent_boundary.target_sha256,
                    receipt_schema_version: RECEIPT_SCHEMA,
                })
                .unwrap(),
            DeliveryClassification::AlreadySent
        );

        for (index, id) in ids.iter().enumerate().skip(2).take(129) {
            delivery.claim(id, (index + 1) as f64).unwrap();
        }
        assert_eq!(
            delivery.classify("review_0").unwrap(),
            DeliveryClassification::SubmittingAmbiguous
        );
        assert_eq!(
            delivery.claim("review_0", 200.0).unwrap(),
            DeliveryClassification::SubmittingAmbiguous
        );
        assert_eq!(
            delivery.classify("review_1").unwrap(),
            DeliveryClassification::AlreadySent
        );
        assert!(delivery.begin_submitting("review_0", &binding).is_err());
        assert!(delivery.read_state().unwrap().deliveries.len() <= MAX_HISTORY);

        let mut all_ambiguous = empty_state();
        for index in 0..MAX_HISTORY {
            all_ambiguous.deliveries.push(DeliveryV4 {
                record_id: format!("blocked_{index}"),
                status: "SUBMITTING".into(),
                claimed_at_unix: 1.0,
                browser_sent_at_unix: None,
                message_sha256: None,
                target_sha256: None,
                receipt_schema_version: None,
                attention: None,
            });
        }
        assert!(delivery.compact_before_claim(&mut all_ambiguous).is_err());
        fs::remove_dir_all(r).unwrap();
    }

    #[test]
    fn operator_attention_and_non_monotonic_receipts_fail_closed() {
        let r = root("attention");
        inbox(&r, true);
        let delivery = StableWakeDelivery::open(&r).unwrap();
        delivery.claim("review_1", 10.0).unwrap();
        let mut state = delivery.read_state().unwrap();
        state.operator_attention = Some("POST_SUBMIT_UNKNOWN".into());
        delivery.write_state(&state).unwrap();
        let binding = delivery.target_binding().unwrap();
        assert!(delivery.classify("review_1").is_err());
        assert!(delivery.claim("review_1", 11.0).is_err());
        assert!(delivery.begin_submitting("review_1", &binding).is_err());

        state.operator_attention = None;
        delivery.write_state(&state).unwrap();
        delivery.begin_submitting("review_1", &binding).unwrap();
        assert!(
            delivery
                .record_receipt(&DeliveryReceiptV1 {
                    record_id: "review_1".into(),
                    browser_sent_at_unix: 9.0,
                    message_sha256: wake_message_sha256("review_1"),
                    target_sha256: binding,
                    receipt_schema_version: RECEIPT_SCHEMA,
                })
                .is_err()
        );
        fs::remove_dir_all(r).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn unsafe_lock_link_is_refused_when_symlink_creation_is_available() {
        use std::os::windows::fs::symlink_file;

        let r = root("unsafe-lock");
        inbox(&r, true);
        let target = r.join("lock-target");
        let lock = r.join(".catdesk/wake-bridge/state.lock");
        fs::write(&target, b"not a lock").unwrap();
        if symlink_file(&target, &lock).is_ok() {
            assert!(
                StableWakeDelivery::open(&r)
                    .unwrap()
                    .claim("review_1", 1.0)
                    .is_err()
            );
        }
        let _ = fs::remove_file(&lock);
        fs::remove_dir_all(r).unwrap();
    }

    #[test]
    fn acknowledged_terminal_sent_retires_at_128_but_unread_sent_does_not() {
        let r = root("terminal-retirement");
        let mut records = (0..127)
            .map(|index| record(&format!("sent_{index}"), true))
            .collect::<Vec<_>>();
        records.push(record("sent_acknowledged", false));
        records.push(record("review_next", true));
        write_inbox_records(&r, &records);
        let delivery = StableWakeDelivery::open(&r).unwrap();
        let mut state = empty_state();
        state.deliveries = (0..127)
            .map(|index| sent(&format!("sent_{index}")))
            .collect();
        state.deliveries.push(sent("sent_acknowledged"));
        delivery.write_state(&state).unwrap();
        let inbox_before = fs::read(r.join(".catdesk/autonomy/review-inbox.json")).unwrap();
        let config_before = fs::read(r.join(".catdesk/wake-bridge/config.json")).unwrap();

        assert_eq!(
            delivery.claim("review_next", 3.0).unwrap(),
            DeliveryClassification::ClaimedRetryable
        );
        let reopened = StableWakeDelivery::open(&r).unwrap();
        assert_eq!(
            reopened.classify("review_next").unwrap(),
            DeliveryClassification::ClaimedRetryable
        );
        let state = reopened.read_state().unwrap();
        assert_eq!(state.deliveries.len(), MAX_HISTORY);
        assert!(
            !state
                .deliveries
                .iter()
                .any(|delivery| delivery.record_id == "sent_acknowledged")
        );
        assert_eq!(
            fs::read(r.join(".catdesk/autonomy/review-inbox.json")).unwrap(),
            inbox_before
        );
        assert_eq!(
            fs::read(r.join(".catdesk/wake-bridge/config.json")).unwrap(),
            config_before
        );

        let mut all_unread = empty_state();
        all_unread.deliveries = (0..MAX_HISTORY)
            .map(|index| sent(&format!("sent_{index}")))
            .collect();
        delivery.write_state(&all_unread).unwrap();
        assert!(delivery.claim("review_next", 4.0).is_err());
        assert_eq!(delivery.read_state().unwrap().deliveries.len(), MAX_HISTORY);
        fs::remove_dir_all(r).unwrap();
    }

    #[test]
    fn terminal_retirement_requires_exact_safe_canonical_evidence_and_receipt() {
        let r = root("terminal-retirement-refusal");
        let delivery = StableWakeDelivery::open(&r).unwrap();
        let mut state = empty_state();
        state.deliveries = (0..MAX_HISTORY)
            .map(|index| sent(&format!("sent_{index}")))
            .collect();
        write_inbox_records(&r, &[record("review_next", true)]);
        delivery.write_state(&state).unwrap();
        assert!(delivery.claim("review_next", 3.0).is_err());

        let mut records = (0..MAX_HISTORY)
            .map(|index| record(&format!("sent_{index}"), false))
            .collect::<Vec<_>>();
        records.push(record("review_next", true));
        write_inbox_records(&r, &records);
        state.deliveries[0].message_sha256 = Some("f".repeat(64));
        delivery.write_state(&state).unwrap();
        assert!(delivery.claim("review_next", 4.0).is_err());
        state.deliveries[0].message_sha256 = Some(wake_message_sha256("sent_0"));
        delivery.write_state(&state).unwrap();

        fs::write(r.join(".catdesk/autonomy/review-inbox.json"), b"{bad").unwrap();
        assert!(delivery.claim("review_next", 5.0).is_err());

        fs::write(
            r.join(".catdesk/autonomy/review-inbox.json"),
            vec![b'x'; 2 * 1024 * 1024 + 1],
        )
        .unwrap();
        assert!(delivery.claim("review_next", 6.0).is_err());

        let mut wrong_project = vec![record("review_next", true)];
        let mut wrong = record("sent_0", false);
        wrong.project_id = "other-project".into();
        wrong_project.push(wrong);
        write_inbox_records(&r, &wrong_project);
        assert!(delivery.claim("review_next", 7.0).is_err());

        let mut conflicting = vec![record("review_next", true), record("sent_0", false)];
        let mut conflict = record("sent_0", false);
        conflict.reference = "artifacts/other.json".into();
        conflicting.push(conflict);
        write_inbox_records(&r, &conflicting);
        assert!(delivery.claim("review_next", 8.0).is_err());

        let mut unsafe_reference = record("sent_0", false);
        unsafe_reference.reference = "../escape.json".into();
        write_inbox_records(&r, &[record("review_next", true), unsafe_reference]);
        assert!(delivery.claim("review_next", 9.0).is_err());
        fs::remove_dir_all(r).unwrap();
    }

    #[test]
    fn repeated_acknowledged_terminal_retirement_stays_bounded() {
        let r = root("terminal-retirement-repeat");
        let delivery = StableWakeDelivery::open(&r).unwrap();
        for round in 0..3 {
            let retired = format!("retired_{round}");
            let next = format!("next_{round}");
            let mut records = (0..127)
                .map(|index| record(&format!("retained_{index}"), true))
                .collect::<Vec<_>>();
            records.push(record(&retired, false));
            records.push(record(&next, true));
            write_inbox_records(&r, &records);
            let mut state = empty_state();
            state.deliveries = (0..127)
                .map(|index| sent(&format!("retained_{index}")))
                .collect();
            state.deliveries.push(sent(&retired));
            delivery.write_state(&state).unwrap();
            assert_eq!(
                delivery.claim(&next, 10.0 + round as f64).unwrap(),
                DeliveryClassification::ClaimedRetryable
            );
            assert_eq!(delivery.read_state().unwrap().deliveries.len(), MAX_HISTORY);
        }
        fs::remove_dir_all(r).unwrap();
    }
}
