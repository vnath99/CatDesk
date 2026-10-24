//! Durable FIFO admission for shared New Chat creation only. This is not
//! browser ownership, delivery authority, or permission to replay a journal.

use super::{existing_directory, plain};
use catdesk_wake::{
    protocol::valid_id,
    store::{atomic, now, read},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

const MAX_ENTRIES: usize = 512;
const MAX_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Identity {
    pub project_id: String,
    pub intent_id: String,
    pub source_generation: u64,
    pub source_digest: String,
    pub message_digest: String,
}

impl Identity {
    fn validate(&self) -> Result<(), String> {
        let digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        };
        if !valid_id(&self.project_id)
            || !valid_id(&self.intent_id)
            || self.source_generation == 0
            || !digest(&self.source_digest)
            || !digest(&self.message_digest)
        {
            return Err("WEB_ROLLOVER_ADMISSION_IDENTITY_INVALID".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Phase {
    Waiting,
    Active,
    CreationRecorded,
    Quarantined,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    identity: Identity,
    first_sequence: u64,
    sequence: u64,
    phase: Phase,
    first_enqueued_utc: u64,
    updated_utc: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    next_sequence: u64,
    entries: Vec<Entry>,
}

impl Default for Journal {
    fn default() -> Self {
        Self {
            schema_version: 1,
            next_sequence: 1,
            entries: vec![],
        }
    }
}

fn journal_path(root: &Path) -> PathBuf {
    root.join("rollover-admission.json")
}

fn load(root: &Path) -> Result<Journal, String> {
    let path = journal_path(root);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Journal::default()),
        Err(_) => return Err("WEB_ROLLOVER_ADMISSION_UNAVAILABLE".into()),
        Ok(metadata) if metadata.len() > MAX_BYTES => {
            return Err("WEB_ROLLOVER_ADMISSION_LIMIT".into());
        }
        Ok(_) => {}
    }
    plain(&path, false)?;
    let journal: Journal = read(&path)?;
    validate(&journal)?;
    Ok(journal)
}

fn validate(journal: &Journal) -> Result<(), String> {
    let mut identities = BTreeSet::new();
    let mut sequences = BTreeSet::new();
    if journal.schema_version != 1
        || journal.next_sequence == 0
        || journal.entries.len() > MAX_ENTRIES
        || journal
            .entries
            .iter()
            .filter(|entry| entry.phase == Phase::Active)
            .count()
            > 1
    {
        return Err("WEB_ROLLOVER_ADMISSION_INVALID".into());
    }
    for entry in &journal.entries {
        entry.identity.validate()?;
        if entry.first_enqueued_utc == 0
            || entry.updated_utc == 0
            || entry.first_sequence == 0
            || entry.first_sequence > entry.sequence
            || entry.sequence >= journal.next_sequence
            || !identities.insert((&entry.identity.project_id, &entry.identity.intent_id))
            || !sequences.insert(entry.sequence)
        {
            return Err("WEB_ROLLOVER_ADMISSION_INVALID".into());
        }
    }
    Ok(())
}

fn save(root: &Path, journal: &Journal) -> Result<(), String> {
    validate(journal)?;
    if serde_json::to_vec_pretty(journal)
        .map_err(|_| "WEB_ROLLOVER_ADMISSION_INVALID")?
        .len() as u64
        > MAX_BYTES
    {
        return Err("WEB_ROLLOVER_ADMISSION_LIMIT".into());
    }
    let path = journal_path(root);
    if path
        .try_exists()
        .map_err(|_| "WEB_ROLLOVER_ADMISSION_UNAVAILABLE")?
    {
        plain(&path, false)?;
    }
    atomic(&path, journal)
}

fn open_lock(root: &Path, name: &str) -> Result<File, String> {
    existing_directory(root)?;
    let path = root.join(name);
    if path
        .try_exists()
        .map_err(|_| "WEB_ROLLOVER_ADMISSION_UNAVAILABLE")?
    {
        plain(&path, false)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(&path)
        .map_err(|_| "WEB_ROLLOVER_ADMISSION_UNAVAILABLE")?;
    plain(&path, false)?;
    Ok(file)
}

fn locked_journal(root: &Path) -> Result<File, String> {
    let file = open_lock(root, "rollover-admission.lock")?;
    file.try_lock().map_err(|_| "WEB_ROLLOVER_ADMISSION_BUSY")?;
    Ok(file)
}

/// A retained OS lease is held only while creating one replacement conversation.
/// Dropping it never asserts success or resubmits. The next owner quarantines an
/// abandoned ACTIVE admission; only the project's existing journal can resume it.
pub(super) struct Lease {
    root: PathBuf,
    identity: Identity,
    _owner: File,
}

fn requeue_quarantine(journal: &mut Journal, identity: &Identity) -> Result<(), String> {
    let own = journal
        .entries
        .iter_mut()
        .find(|entry| entry.identity == *identity)
        .expect("identity was inserted");
    if own.phase == Phase::Quarantined {
        let sequence = journal.next_sequence;
        journal.next_sequence = sequence
            .checked_add(1)
            .ok_or("WEB_ROLLOVER_ADMISSION_LIMIT")?;
        own.sequence = sequence;
        own.phase = Phase::Waiting;
        own.updated_utc = now();
    }
    Ok(())
}

impl Lease {
    /// Call only after the project journal durably records the validated exact
    /// USER/issued-URL receipt. Completion/binding remain separate checkpoints.
    pub fn creation_recorded(self) -> Result<(), String> {
        let _lock = locked_journal(&self.root)?;
        let mut journal = load(&self.root)?;
        let entry = journal
            .entries
            .iter_mut()
            .find(|entry| entry.identity == self.identity)
            .ok_or("WEB_ROLLOVER_ADMISSION_CHANGED")?;
        if entry.phase != Phase::Active {
            return Err("WEB_ROLLOVER_ADMISSION_CHANGED".into());
        }
        entry.phase = Phase::CreationRecorded;
        entry.updated_utc = now();
        save(&self.root, &journal)
    }
}

/// Called after the current project's existing phase/target/policy checks.
/// `eligible` rechecks a different waiting head's durable project journal. It
/// must not start a browser, change that project, or grant submission authority.
pub(super) fn try_acquire(
    root: &Path,
    identity: Identity,
    mut eligible: impl FnMut(&Identity) -> bool,
) -> Result<Option<Lease>, String> {
    identity.validate()?;
    let _lock = locked_journal(root)?;
    let mut journal = load(root)?;
    if let Some(entry) = journal.entries.iter().find(|entry| {
        entry.identity.project_id == identity.project_id
            && entry.identity.intent_id == identity.intent_id
    }) {
        if entry.identity != identity {
            return Err("WEB_ROLLOVER_ADMISSION_SCOPE_CHANGED".into());
        }
        if entry.phase == Phase::CreationRecorded {
            return Err("WEB_ROLLOVER_CREATION_ALREADY_RECORDED".into());
        }
    } else {
        if journal.entries.len() == MAX_ENTRIES {
            return Err("WEB_ROLLOVER_ADMISSION_LIMIT".into());
        }
        let sequence = journal.next_sequence;
        journal.next_sequence = sequence
            .checked_add(1)
            .ok_or("WEB_ROLLOVER_ADMISSION_LIMIT")?;
        journal.entries.push(Entry {
            identity: identity.clone(),
            first_sequence: sequence,
            sequence,
            phase: Phase::Waiting,
            first_enqueued_utc: now(),
            updated_utc: now(),
        });
    }
    requeue_quarantine(&mut journal, &identity)?;
    let owner = open_lock(root, "rollover-admission-owner.lock")?;
    match owner.try_lock() {
        Err(std::fs::TryLockError::WouldBlock) => {
            save(root, &journal)?;
            return Ok(None);
        }
        Err(_) => return Err("WEB_ROLLOVER_ADMISSION_UNAVAILABLE".into()),
        Ok(()) => {}
    }
    // No admission owner exists. Retain an abandoned operation as quarantine,
    // never as creation/delivery proof. Existing browser ownership still applies.
    for entry in &mut journal.entries {
        if entry.phase == Phase::Active {
            entry.phase = Phase::Quarantined;
            entry.updated_utc = now();
        }
    }
    requeue_quarantine(&mut journal, &identity)?;
    loop {
        let Some(index) = journal
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.phase == Phase::Waiting)
            .min_by_key(|(_, entry)| entry.sequence)
            .map(|(index, _)| index)
        else {
            return Err("WEB_ROLLOVER_ADMISSION_INVALID".into());
        };
        let head = &mut journal.entries[index];
        if head.identity == identity {
            head.phase = Phase::Active;
            head.updated_utc = now();
            save(root, &journal)?;
            return Ok(Some(Lease {
                root: root.to_path_buf(),
                identity,
                _owner: owner,
            }));
        }
        if eligible(&head.identity) {
            save(root, &journal)?;
            return Ok(None);
        }
        // Missing/terminal/disabled source: preserve admission as quarantine,
        // leaving its original project journal, draft and binding untouched.
        head.phase = Phase::Quarantined;
        head.updated_utc = now();
    }
}

pub(super) fn status(root: &Path, identity: &Identity) -> Result<Value, String> {
    identity.validate()?;
    let journal = load(root)?;
    let entry = journal
        .entries
        .iter()
        .find(|entry| entry.identity == *identity);
    Ok(match entry {
        None => json!({"status":"UNOBSERVED","liveOwnerVerified":false}),
        Some(entry) => json!({"status":"DURABLE_SNAPSHOT","phase":entry.phase,
            "sequence":entry.sequence,"firstSequence":entry.first_sequence,
                "firstEnqueuedUtc":entry.first_enqueued_utc,"updatedUtc":entry.updated_utc,
            "waitingAhead":journal.entries.iter().filter(|other| matches!(other.phase, Phase::Waiting | Phase::Active) && other.sequence < entry.sequence).count(),
            "liveOwnerVerified":false,"creationProof":false}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("web-rollover-admission-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        root
    }
    fn key(project: &str) -> Identity {
        Identity {
            project_id: project.into(),
            intent_id: format!("intent-{project}"),
            source_generation: 1,
            source_digest: "a".repeat(64),
            message_digest: "b".repeat(64),
        }
    }
    #[test]
    fn rollover_admission_is_durable_fifo_and_not_a_global_tool_lock() {
        let root = fixture();
        let alpha = try_acquire(&root, key("alpha"), |_| true).unwrap().unwrap();
        assert!(try_acquire(&root, key("beta"), |_| true).unwrap().is_none());
        assert!(
            try_acquire(&root, key("gamma"), |_| true)
                .unwrap()
                .is_none()
        );
        assert_eq!(status(&root, &key("beta")).unwrap()["waitingAhead"], 1);
        std::fs::write(root.join("unrelated-project-work.txt"), "independent work").unwrap();
        alpha.creation_recorded().unwrap();
        assert!(
            try_acquire(&root, key("gamma"), |_| true)
                .unwrap()
                .is_none()
        );
        let beta = try_acquire(&root, key("beta"), |_| true).unwrap().unwrap();
        beta.creation_recorded().unwrap();
        try_acquire(&root, key("gamma"), |_| true)
            .unwrap()
            .unwrap()
            .creation_recorded()
            .unwrap();
        assert_eq!(
            load(&root)
                .unwrap()
                .entries
                .iter()
                .filter(|entry| entry.phase == Phase::CreationRecorded)
                .count(),
            3
        );
        assert_eq!(
            try_acquire(&root, key("alpha"), |_| true).err().unwrap(),
            "WEB_ROLLOVER_CREATION_ALREADY_RECORDED"
        );
    }
    #[test]
    fn rollover_admission_abandoned_owner_quarantines_and_does_not_deny_sibling() {
        let root = fixture();
        let alpha = try_acquire(&root, key("alpha"), |_| true).unwrap().unwrap();
        assert!(try_acquire(&root, key("beta"), |_| true).unwrap().is_none());
        drop(alpha); // Simulates owner exit, not browser completion or safe resend.
        let beta = try_acquire(&root, key("beta"), |_| true).unwrap().unwrap();
        assert_eq!(
            status(&root, &key("alpha")).unwrap()["phase"],
            "QUARANTINED"
        );
        beta.creation_recorded().unwrap();
        // Parent may re-enter only after its existing source journal authorizes
        // safe draft recovery; queue metadata itself cannot grant that authority.
        let recovered = try_acquire(&root, key("alpha"), |_| true).unwrap().unwrap();
        assert!(
            status(&root, &key("alpha")).unwrap()["sequence"]
                .as_u64()
                .unwrap()
                > 2
        );
        recovered.creation_recorded().unwrap();
    }
    #[test]
    fn rollover_admission_rejects_scope_drift_without_changing_the_journal() {
        let root = fixture();
        let alpha = try_acquire(&root, key("alpha"), |_| true).unwrap().unwrap();
        let before = std::fs::read(journal_path(&root)).unwrap();
        let mut changed = key("alpha");
        changed.message_digest = "c".repeat(64);
        assert_eq!(
            try_acquire(&root, changed, |_| true).err().unwrap(),
            "WEB_ROLLOVER_ADMISSION_SCOPE_CHANGED"
        );
        assert_eq!(std::fs::read(journal_path(&root)).unwrap(), before);
        drop(alpha);
    }
    #[test]
    fn rollover_admission_ineligible_waiter_is_retained_without_blocking_other_projects() {
        let root = fixture();
        let alpha = try_acquire(&root, key("alpha"), |_| true).unwrap().unwrap();
        assert!(try_acquire(&root, key("beta"), |_| true).unwrap().is_none());
        alpha.creation_recorded().unwrap();
        let gamma = try_acquire(&root, key("gamma"), |head| head.project_id != "beta")
            .unwrap()
            .unwrap();
        assert_eq!(status(&root, &key("beta")).unwrap()["phase"], "QUARANTINED");
        gamma.creation_recorded().unwrap();
    }
    #[test]
    fn rollover_admission_read_only_status_does_not_initialize_state_or_claim_liveness() {
        let root = fixture();
        assert_eq!(
            status(&root, &key("alpha")).unwrap()["status"],
            "UNOBSERVED"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::write(journal_path(&root), b"broken").unwrap();
        assert!(status(&root, &key("alpha")).is_err());
    }

    #[test]
    fn rollover_admission_concurrent_creation_waits_for_the_retained_owner() {
        use std::{sync::mpsc, time::Duration};
        let root = fixture();
        let worker_root = root.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let owner = try_acquire(&worker_root, key("alpha"), |_| true)
                .unwrap()
                .unwrap();
            ready_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(10));
            owner.creation_recorded()
        });
        let ready = ready_rx.recv_timeout(Duration::from_secs(2));
        let beta = try_acquire(&root, key("beta"), |_| true);
        let _ = release_tx.send(());
        let exited = worker.join();
        assert!(ready.is_ok());
        assert!(
            beta.unwrap().is_none(),
            "waiting caller must not enter New Chat"
        );
        assert!(exited.unwrap().is_ok());
        try_acquire(&root, key("beta"), |_| true)
            .unwrap()
            .unwrap()
            .creation_recorded()
            .unwrap();
    }
}
