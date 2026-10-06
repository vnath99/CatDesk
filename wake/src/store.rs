use crate::protocol::{Event, Target, digest, valid_id};
use crate::{PROTOCOL_VERSION, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP: AtomicU64 = AtomicU64::new(0);
const MAX_FILE: u64 = 1024 * 1024;
pub const MANUAL_TURN_PREFIX: &str = "manual-turn-";
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("UTC clock before epoch")
        .as_secs()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetChange {
    pub project_id: String,
    pub target: Target,
    pub changed_utc: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub targets: BTreeMap<String, Target>,
    pub history: Vec<TargetChange>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: PROTOCOL_VERSION,
            targets: BTreeMap::new(),
            history: vec![],
        }
    }
}

impl Config {
    fn validate(&self) -> Result<()> {
        if self.schema_version != PROTOCOL_VERSION
            || self.targets.len() > 64
            || self.history.len() > 128
        {
            return Err("CONFIG_INVALID".into());
        }
        for (id, target) in &self.targets {
            if !valid_id(id) {
                return Err("CONFIG_PROJECT_INVALID".into());
            }
            target.validate()?;
        }
        for change in &self.history {
            if !valid_id(&change.project_id) || change.changed_utc == 0 {
                return Err("CONFIG_HISTORY_INVALID".into());
            }
            change.target.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Phase {
    Claimed,
    Submitting,
    Sent,
    Attention,
    Stale,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Delivery {
    pub schema_version: u32,
    pub event: Event,
    pub target: Target,
    pub phase: Phase,
    pub owner: String,
    pub updated_utc: u64,
    pub message_digest: String,
    pub receipt: Option<Receipt>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub schema_version: u32,
    pub event_id: String,
    pub target_generation: u64,
    pub target_digest: String,
    pub message_digest: String,
    pub sent_utc: u64,
    pub evidence: String,
}

/// Passive assistant-response timing, anchored only to an exact USER receipt.
/// It has no control input and cannot publish or replay a wake event.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResponseState {
    Observing,
    Retrying,
    Attention,
    Complete,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnTimer {
    pub schema_version: u32,
    pub event_id: String,
    pub project_id: String,
    pub target_generation: u64,
    pub target_digest: String,
    pub started_utc: u64,
    pub completed_utc: Option<u64>,
    pub response_state: ResponseState,
}

impl TurnTimer {
    fn validate(&self) -> Result<()> {
        if self.schema_version != PROTOCOL_VERSION
            || !valid_id(&self.event_id)
            || !valid_id(&self.project_id)
            || self.target_generation == 0
            || self.target_digest.len() != 64
            || self.started_utc == 0
            || matches!(self.response_state, ResponseState::Complete)
                != self.completed_utc.is_some()
            || self
                .completed_utc
                .is_some_and(|complete| complete < self.started_utc)
        {
            return Err("TURN_TIMER_INVALID".into());
        }
        Ok(())
    }
}

impl Delivery {
    fn validate(&self) -> Result<()> {
        self.event.validate()?;
        self.target.validate()?;
        if self.schema_version != PROTOCOL_VERSION
            || !valid_id(&self.owner)
            || self.updated_utc == 0
            || self.event.target_generation != self.target.generation
            || self.message_digest != digest(&self.event.message)
            || self.reason.as_ref().is_some_and(|r| !valid_id(r))
        {
            return Err("RECEIPT_JOURNAL_INVALID".into());
        }
        match (&self.phase, &self.receipt) {
            (Phase::Sent, Some(r))
                if r.schema_version == PROTOCOL_VERSION
                    && r.event_id == self.event.event_id
                    && r.target_generation == self.target.generation
                    && r.target_digest == self.target.digest
                    && r.message_digest == self.message_digest
                    && r.sent_utc > 0
                    && r.evidence == "EXACT_USER_MESSAGE_APPENDED" =>
            {
                Ok(())
            }
            (Phase::Sent, _) | (_, Some(_)) => Err("RECEIPT_INVALID".into()),
            _ => Ok(()),
        }
    }
}

/// All producers and controls serialize through the same OS file lock. Kernel
/// releases it on process death; atomic JSON replacement preserves prior bytes.
pub struct Store {
    root: PathBuf,
}
pub struct HostLease {
    _lock: File,
    root: PathBuf,
    owner: String,
}
impl HostLease {
    pub fn owner(&self) -> &str {
        &self.owner
    }
}
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        safe_directories(root)?;
        for child in ["queue", "archive", "deliveries", "turns", "logs"] {
            safe_directories(&root.join(child))?;
        }
        Ok(Self {
            root: fs::canonicalize(root).map_err(|_| "CONFIG_ROOT_UNAVAILABLE")?,
        })
    }

    /// Test-build-only constructor for sandboxed integration tests whose worker
    /// may not be permitted to inspect every ancestor above the repository.
    /// Production `open` remains unchanged and still validates from the volume
    /// root. This helper accepts one already-existing canonical trusted parent,
    /// then applies the same non-link/non-reparse directory checks to every
    /// descendant it creates beneath that parent.
    #[cfg(feature = "test-support")]
    pub fn open_scoped_for_test(root: &Path, trusted_parent: &Path) -> Result<Self> {
        let trusted = fs::canonicalize(trusted_parent).map_err(|_| "CONFIG_ROOT_UNAVAILABLE")?;
        if !trusted.is_absolute() || !root.is_absolute() {
            return Err("CONFIG_ROOT_NOT_ABSOLUTE".into());
        }
        let relative = root
            .strip_prefix(trusted_parent)
            .map_err(|_| "CONFIG_ROOT_OUTSIDE_TEST_PARENT")?;
        let scoped_root = trusted.join(relative);
        safe_directories_beneath(&trusted, &scoped_root)?;
        for child in ["queue", "archive", "deliveries", "turns", "logs"] {
            safe_directories_beneath(&trusted, &scoped_root.join(child))?;
        }
        let canonical = fs::canonicalize(&scoped_root).map_err(|_| "CONFIG_ROOT_UNAVAILABLE")?;
        if !canonical.starts_with(&trusted) {
            return Err("CONFIG_ROOT_OUTSIDE_TEST_PARENT".into());
        }
        Ok(Self { root: canonical })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn lock(&self) -> Result<File> {
        let path = self.root.join("transaction.lock");
        reject_link(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| "CONFIG_LOCK_UNAVAILABLE")?;
        file.lock().map_err(|_| "CONFIG_LOCK_UNAVAILABLE")?;
        Ok(file)
    }
    fn read_config(&self) -> Result<Config> {
        let config: Config = read(&self.root.join("config.json"))?;
        config.validate()?;
        Ok(config)
    }
    pub fn initialize(&self) -> Result<()> {
        let _lock = self.lock()?;
        if !self
            .root
            .join("config.json")
            .try_exists()
            .map_err(|_| "CONFIG_UNAVAILABLE")?
        {
            atomic(&self.root.join("config.json"), &Config::default())?;
        }
        self.read_config().map(|_| ())
    }
    pub fn config(&self) -> Result<Config> {
        let _lock = self.lock()?;
        self.read_config()
    }
    pub fn set_target(&self, project: &str, expected: u64, url: &str) -> Result<Target> {
        self.set_target_with_submission_policy(project, expected, url, false)
    }

    /// Explicit operator target rollover may quarantine an already-crossed
    /// SUBMITTING delivery on its original immutable target instead of making
    /// that historical ambiguity permanently block future target generations.
    ///
    /// This is intentionally separate from ordinary set_target(): callers must
    /// opt into rollover semantics. Every quarantined delivery must still bind
    /// to an exact target retained by the current config/history. The delivery
    /// and queue event are not changed, retried, downgraded, or retired.
    pub fn set_target_quarantining_submitting(
        &self,
        project: &str,
        expected: u64,
        url: &str,
    ) -> Result<Target> {
        self.set_target_with_submission_policy(project, expected, url, true)
    }

    fn set_target_with_submission_policy(
        &self,
        project: &str,
        expected: u64,
        url: &str,
        quarantine_submitting: bool,
    ) -> Result<Target> {
        if !valid_id(project) {
            return Err("CONFIG_PROJECT_INVALID".into());
        }
        let _lock = self.lock()?;
        let mut config = self.read_config()?;
        let previous = config.targets.get(project);
        if previous.map_or(0, |t| t.generation) != expected {
            return Err("CONFIG_CAS_CONFLICT".into());
        }
        if let Some(target) = previous
            && target.url == url
        {
            return Ok(target.clone());
        }
        for entry in
            fs::read_dir(self.root.join("deliveries")).map_err(|_| "RECEIPT_UNAVAILABLE")?
        {
            let path = entry.map_err(|_| "RECEIPT_UNAVAILABLE")?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let delivery: Delivery = read(&path)?;
            delivery.validate()?;
            if delivery.event.project_id != project || delivery.phase != Phase::Submitting {
                continue;
            }
            if !quarantine_submitting {
                return Err("CONFIG_SUBMISSION_RECONCILIATION_REQUIRED".into());
            }
            let target_is_retained = config.targets.get(project) == Some(&delivery.target)
                || config
                    .history
                    .iter()
                    .any(|change| change.project_id == project && change.target == delivery.target);
            if !target_is_retained {
                return Err("CONFIG_SUBMISSION_TARGET_UNAVAILABLE".into());
            }
        }
        let target = Target::new(
            expected
                .checked_add(1)
                .ok_or("CONFIG_GENERATION_OVERFLOW")?,
            url.into(),
        )?;
        config.targets.insert(project.into(), target.clone());
        config.history.push(TargetChange {
            project_id: project.into(),
            target: target.clone(),
            changed_utc: now(),
        });
        if config.history.len() > 128 {
            config.history.remove(0);
        }
        config.validate()?;
        atomic(&self.root.join("config.json"), &config)?;
        if self.read_config()?.targets.get(project) != Some(&target) {
            return Err("CONFIG_READBACK_MISMATCH".into());
        }
        Ok(target)
    }
    /// Caller captures generation when creating the event, never during replay.
    /// Identical publication is idempotent; changed bytes under one ID conflict.
    pub fn publish(&self, event: &Event) -> Result<()> {
        event.validate()?;
        let _lock = self.lock()?;
        let path = self.event_path(&event.event_id);
        let archived = self.archive_path(&event.event_id);
        if archived.try_exists().map_err(|_| "QUEUE_UNAVAILABLE")? {
            let prior: Event = read(&archived)?;
            return if prior == *event {
                Ok(())
            } else {
                Err("QUEUE_EVENT_ID_CONFLICT".into())
            };
        }
        if path.try_exists().map_err(|_| "QUEUE_UNAVAILABLE")? {
            let prior: Event = read(&path)?;
            return if prior == *event {
                Ok(())
            } else {
                Err("QUEUE_EVENT_ID_CONFLICT".into())
            };
        }
        let config = self.read_config()?;
        let target = config
            .targets
            .get(&event.project_id)
            .ok_or("TARGET_NOT_CONFIGURED")?;
        if target.generation != event.target_generation {
            return Err("TARGET_GENERATION_STALE".into());
        }
        self.check_capacity()?;
        atomic(&path, event)
    }

    /// Producer convenience: assign a target generation and commit the event in
    /// one transaction. Replays retain the original generation and timestamp.
    pub fn produce(
        &self,
        id: &str,
        project: &str,
        event_type: &str,
        message: &str,
    ) -> Result<Event> {
        self.produce_bound(id, project, event_type, message, None)
    }

    /// Manual diagnostics must bind the target observed by their caller. The
    /// comparison and queue publication share one lock, including idempotent
    /// retries, so a root mismatch or concurrent rollover cannot retarget work.
    pub fn produce_expected(
        &self,
        id: &str,
        project: &str,
        event_type: &str,
        message: &str,
        expected: &Target,
    ) -> Result<Event> {
        self.produce_bound(id, project, event_type, message, Some(expected))
    }

    fn produce_bound(
        &self,
        id: &str,
        project: &str,
        event_type: &str,
        message: &str,
        expected: Option<&Target>,
    ) -> Result<Event> {
        if !valid_id(id) || !valid_id(project) {
            return Err("QUEUE_EVENT_INVALID".into());
        }
        let _lock = self.lock()?;
        if let Some(expected) = expected
            && self.read_config()?.targets.get(project) != Some(expected)
        {
            return Err("TARGET_EXPECTATION_MISMATCH".into());
        }
        let path = self.event_path(id);
        let archived = self.archive_path(id);
        if archived.try_exists().map_err(|_| "QUEUE_UNAVAILABLE")? {
            let prior: Event = read(&archived)?;
            prior.validate()?;
            if expected.is_some_and(|target| target.generation != prior.target_generation) {
                return Err("TARGET_EXPECTATION_MISMATCH".into());
            }
            if prior.event_id == id
                && prior.project_id == project
                && prior.event_type == event_type
                && prior.message == message
            {
                return Ok(prior);
            }
            return Err("QUEUE_EVENT_ID_CONFLICT".into());
        }
        if path.try_exists().map_err(|_| "QUEUE_UNAVAILABLE")? {
            let prior: Event = read(&path)?;
            prior.validate()?;
            if expected.is_some_and(|target| target.generation != prior.target_generation) {
                return Err("TARGET_EXPECTATION_MISMATCH".into());
            }
            if prior.event_id != id
                || prior.project_id != project
                || prior.event_type != event_type
                || prior.message != message
            {
                return Err("QUEUE_EVENT_ID_CONFLICT".into());
            }
            return Ok(prior);
        }
        let config = self.read_config()?;
        let target = config.targets.get(project).ok_or("TARGET_NOT_CONFIGURED")?;
        let event = Event {
            schema_version: 1,
            event_id: id.into(),
            project_id: project.into(),
            event_type: event_type.into(),
            created_utc: now(),
            message: message.into(),
            target_generation: target.generation,
            audit_references: vec![],
        };
        event.validate()?;
        self.check_capacity()?;
        atomic(&path, &event)?;
        Ok(event)
    }
    fn event_path(&self, id: &str) -> PathBuf {
        self.root.join("queue").join(format!("{id}.json"))
    }
    fn archive_path(&self, id: &str) -> PathBuf {
        self.root.join("archive").join(format!("{id}.json"))
    }
    fn check_capacity(&self) -> Result<()> {
        let mut count = 0;
        for entry in fs::read_dir(self.root.join("queue")).map_err(|_| "QUEUE_UNAVAILABLE")? {
            if entry
                .map_err(|_| "QUEUE_UNAVAILABLE")?
                .path()
                .extension()
                .and_then(|x| x.to_str())
                == Some("json")
            {
                count += 1;
            }
            if count >= 10000 {
                return Err("QUEUE_CAPACITY_EXCEEDED".into());
            }
        }
        Ok(())
    }
    fn delivery_path(&self, id: &str) -> PathBuf {
        self.root.join("deliveries").join(format!("{id}.json"))
    }
    fn timer_path(&self, id: &str) -> PathBuf {
        self.root.join("turns").join(format!("{id}.json"))
    }

    fn read_timer(&self, id: &str) -> Result<Option<TurnTimer>> {
        if !valid_id(id) {
            return Err("TURN_EVENT_INVALID".into());
        }
        let path = self.timer_path(id);
        if !path.try_exists().map_err(|_| "TURN_TIMER_UNAVAILABLE")? {
            return Ok(None);
        }
        let timer: TurnTimer = read(&path)?;
        timer.validate()?;
        if timer.event_id != id {
            return Err("TURN_EVENT_MISMATCH".into());
        }
        Ok(Some(timer))
    }
    pub fn timer(&self, id: &str) -> Result<Option<TurnTimer>> {
        let _lock = self.lock()?;
        self.read_timer(id)
    }
    pub fn timers(&self) -> Result<Vec<TurnTimer>> {
        let _lock = self.lock()?;
        let mut timers = Vec::new();
        for entry in fs::read_dir(self.root.join("turns")).map_err(|_| "TURN_TIMER_UNAVAILABLE")? {
            let path = entry.map_err(|_| "TURN_TIMER_UNAVAILABLE")?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let timer: TurnTimer = read(&path)?;
            timer.validate()?;
            if path.file_stem().and_then(|value| value.to_str()) != Some(&timer.event_id) {
                return Err("TURN_EVENT_MISMATCH".into());
            }
            timers.push(timer);
        }
        timers.sort_by(|left, right| left.event_id.cmp(&right.event_id));
        Ok(timers)
    }

    /// Start or recover an ordinary ChatGPT work timer using the same durable
    /// TurnTimer schema as Wake response timers. Manual timers are identified
    /// only by the reserved ID namespace so older Wake hosts can continue to
    /// deserialize the record without a schema change.
    pub fn start_manual_turn_timer(
        &self,
        id: &str,
        project: &str,
        expected_target: &Target,
    ) -> Result<TurnTimer> {
        if !id.starts_with(MANUAL_TURN_PREFIX) || !valid_id(id) || !valid_id(project) {
            return Err("MANUAL_TURN_INVALID".into());
        }
        expected_target.validate()?;
        let _lock = self.lock()?;
        let config = self.read_config()?;
        if config.targets.get(project) != Some(expected_target) {
            return Err("MANUAL_TURN_TARGET_MISMATCH".into());
        }
        for path in [
            self.event_path(id),
            self.archive_path(id),
            self.delivery_path(id),
        ] {
            if path.try_exists().map_err(|_| "TURN_TIMER_UNAVAILABLE")? {
                return Err("MANUAL_TURN_AUTHORITY_COLLISION".into());
            }
        }
        if let Some(timer) = self.read_timer(id)? {
            if timer.project_id != project
                || timer.target_generation != expected_target.generation
                || timer.target_digest != expected_target.digest
            {
                return Err("MANUAL_TURN_BINDING_MISMATCH".into());
            }
            if timer.response_state == ResponseState::Complete {
                return Err("MANUAL_TURN_TERMINAL".into());
            }
            return Ok(timer);
        }
        let timer = TurnTimer {
            schema_version: PROTOCOL_VERSION,
            event_id: id.into(),
            project_id: project.into(),
            target_generation: expected_target.generation,
            target_digest: expected_target.digest.clone(),
            started_utc: now(),
            completed_utc: None,
            response_state: ResponseState::Observing,
        };
        timer.validate()?;
        atomic(&self.timer_path(id), &timer)?;
        Ok(timer)
    }

    /// Complete only a reserved ordinary-work timer. Wake-event timers remain
    /// receipt-bound to complete_timer() and cannot cross this manual path.
    pub fn complete_manual_turn_timer(
        &self,
        id: &str,
        project: &str,
        expected_target: &Target,
    ) -> Result<TurnTimer> {
        if !id.starts_with(MANUAL_TURN_PREFIX) || !valid_id(id) || !valid_id(project) {
            return Err("MANUAL_TURN_INVALID".into());
        }
        expected_target.validate()?;
        let _lock = self.lock()?;
        let config = self.read_config()?;
        if config.targets.get(project) != Some(expected_target) {
            return Err("MANUAL_TURN_TARGET_MISMATCH".into());
        }
        for path in [
            self.event_path(id),
            self.archive_path(id),
            self.delivery_path(id),
        ] {
            if path.try_exists().map_err(|_| "TURN_TIMER_UNAVAILABLE")? {
                return Err("MANUAL_TURN_AUTHORITY_COLLISION".into());
            }
        }
        let mut timer = self.read_timer(id)?.ok_or("MANUAL_TURN_MISSING")?;
        if timer.project_id != project
            || timer.target_generation != expected_target.generation
            || timer.target_digest != expected_target.digest
        {
            return Err("MANUAL_TURN_BINDING_MISMATCH".into());
        }
        if timer.response_state == ResponseState::Complete {
            return Ok(timer);
        }
        timer.response_state = ResponseState::Complete;
        timer.completed_utc = Some(now().max(timer.started_utc));
        timer.validate()?;
        atomic(&self.timer_path(id), &timer)?;
        Ok(timer)
    }

    pub fn events(&self) -> Result<Vec<Event>> {
        let _lock = self.lock()?;
        let mut events = Vec::new();
        for entry in fs::read_dir(self.root.join("queue")).map_err(|_| "QUEUE_UNAVAILABLE")? {
            let path = entry.map_err(|_| "QUEUE_UNAVAILABLE")?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let event: Event = read(&path)?;
            event.validate()?;
            if path.file_stem().and_then(|s| s.to_str()) != Some(&event.event_id) {
                return Err("QUEUE_ID_MISMATCH".into());
            }
            events.push(event);
            if events.len() > 10000 {
                return Err("QUEUE_CAPACITY_EXCEEDED".into());
            }
        }
        events.sort_by(|a, b| (a.created_utc, &a.event_id).cmp(&(b.created_utc, &b.event_id)));
        Ok(events)
    }
    fn read_delivery(&self, id: &str) -> Result<Option<Delivery>> {
        if !valid_id(id) {
            return Err("QUEUE_EVENT_INVALID".into());
        }
        let path = self.delivery_path(id);
        if !path.try_exists().map_err(|_| "RECEIPT_UNAVAILABLE")? {
            return Ok(None);
        }
        let delivery: Delivery = read(&path)?;
        delivery.validate()?;
        if delivery.event.event_id != id {
            return Err("RECEIPT_ID_MISMATCH".into());
        }
        Ok(Some(delivery))
    }
    pub fn delivery(&self, id: &str) -> Result<Option<Delivery>> {
        let _lock = self.lock()?;
        self.read_delivery(id)
    }
    /// A host holds a separate lifetime lock. Only after that lock is acquired
    /// can it reclaim pre-submit work. Submitting/Sent/Attention are never replayed.
    pub fn claim(&self, event: &Event, lease: &HostLease) -> Result<Delivery> {
        event.validate()?;
        if lease.root != self.root {
            return Err("HOST_LEASE_ROOT_MISMATCH".into());
        }
        let owner = lease.owner();
        if !valid_id(owner) {
            return Err("HOST_OWNER_INVALID".into());
        }
        let _lock = self.lock()?;
        let persisted: Event = read(&self.event_path(&event.event_id))?;
        if persisted != *event {
            return Err("QUEUE_EVENT_ID_CONFLICT".into());
        }
        let config = self.read_config()?;
        let target = config
            .targets
            .get(&event.project_id)
            .ok_or("TARGET_NOT_CONFIGURED")?;
        if target.generation != event.target_generation {
            return Err("TARGET_GENERATION_STALE".into());
        }
        if let Some(previous) = self.read_delivery(&event.event_id)? {
            if previous.phase != Phase::Claimed {
                return Err("SUBMISSION_RECONCILIATION_REQUIRED".into());
            }
            if previous.event != *event || previous.target != *target {
                return Err("RECEIPT_BINDING_MISMATCH".into());
            }
        }
        let delivery = Delivery {
            schema_version: PROTOCOL_VERSION,
            event: event.clone(),
            target: target.clone(),
            phase: Phase::Claimed,
            owner: owner.into(),
            updated_utc: now(),
            message_digest: digest(&event.message),
            receipt: None,
            reason: None,
        };
        atomic(&self.delivery_path(&event.event_id), &delivery)?;
        Ok(delivery)
    }
    pub fn transition(
        &self,
        id: &str,
        owner: &str,
        next: Phase,
        receipt: Option<Receipt>,
        reason: Option<String>,
    ) -> Result<Delivery> {
        let _lock = self.lock()?;
        let mut delivery = self.read_delivery(id)?.ok_or("SUBMISSION_CLAIM_MISSING")?;
        if delivery.owner != owner {
            return Err("SUBMISSION_OWNER_CONFLICT".into());
        }
        if !matches!(
            (&delivery.phase, &next),
            (Phase::Claimed, Phase::Submitting | Phase::Attention)
                | (Phase::Submitting, Phase::Sent)
        ) {
            return Err("SUBMISSION_TRANSITION_INVALID".into());
        }
        if next == Phase::Submitting {
            let config = self.read_config()?;
            if config.targets.get(&delivery.event.project_id) != Some(&delivery.target) {
                return Err("TARGET_GENERATION_STALE".into());
            }
        }
        delivery.phase = next;
        delivery.receipt = receipt;
        delivery.reason = reason;
        delivery.updated_utc = now();
        delivery.validate()?;
        atomic(&self.delivery_path(id), &delivery)?;
        if delivery.phase == Phase::Sent {
            // Receipt commit precedes archival. A crash retains either the
            // queue event plus receipt or the archive plus receipt; never a replay.
            atomic(&self.archive_path(id), &delivery.event)?;
            fs::remove_file(self.event_path(id)).map_err(|_| "QUEUE_ARCHIVE_FAILED")?;
        }
        Ok(delivery)
    }

    /// Reconcile one already-crossed submission boundary after a later
    /// browser-only proof confirms that the exact expected user message is
    /// durably present on the exact stored target. This path can only advance
    /// SUBMITTING -> SENT; it never permits replay, downgrade, or retargeting.
    pub(crate) fn reconcile_submitting_sent(&self, id: &str, receipt: Receipt) -> Result<Delivery> {
        let _lock = self.lock()?;
        let mut delivery = self.read_delivery(id)?.ok_or("SUBMISSION_CLAIM_MISSING")?;
        if delivery.phase != Phase::Submitting || delivery.receipt.is_some() {
            return Err("SUBMISSION_RECONCILIATION_REFUSED".into());
        }
        if self.read_config()?.targets.get(&delivery.event.project_id) != Some(&delivery.target) {
            return Err("TARGET_GENERATION_STALE".into());
        }
        if receipt.schema_version != PROTOCOL_VERSION
            || receipt.event_id != delivery.event.event_id
            || receipt.target_generation != delivery.target.generation
            || receipt.target_digest != delivery.target.digest
            || receipt.message_digest != delivery.message_digest
            || receipt.sent_utc == 0
            || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
        {
            return Err("RECEIPT_BINDING_MISMATCH".into());
        }
        delivery.phase = Phase::Sent;
        delivery.receipt = Some(receipt);
        delivery.reason = None;
        delivery.updated_utc = now();
        delivery.validate()?;
        atomic(&self.delivery_path(id), &delivery)?;
        atomic(&self.archive_path(id), &delivery.event)?;
        fs::remove_file(self.event_path(id)).map_err(|_| "QUEUE_ARCHIVE_FAILED")?;
        Ok(delivery)
    }

    /// Start/recover the passive response timer once the same-tab browser
    /// proves that the submitted USER turn was accepted by ChatGPT. This is
    /// deliberately weaker than a durable receipt: it creates no receipt,
    /// archives no event, and cannot transition the delivery to SENT.
    /// A later fresh-document EXACT_USER_MESSAGE_APPENDED receipt remains
    /// mandatory for final delivery success.
    pub fn start_submission_timer(&self, id: &str) -> Result<()> {
        let _lock = self.lock()?;
        let delivery = self.read_delivery(id)?.ok_or("TIMER_EVENT_MISSING")?;
        if delivery.phase != Phase::Submitting {
            return Err("TIMER_SUBMISSION_NOT_ACTIVE".into());
        }
        let config = self.read_config()?;
        if config.targets.get(&delivery.event.project_id) != Some(&delivery.target) {
            return Err("TARGET_GENERATION_STALE".into());
        }
        let path = self.timer_path(&delivery.event.event_id);
        if let Some(mut prior) = self.read_timer(&delivery.event.event_id)? {
            if prior.project_id != delivery.event.project_id
                || prior.target_generation != delivery.target.generation
                || prior.target_digest != delivery.target.digest
            {
                return Err("TIMER_SUBMISSION_BINDING_MISMATCH".into());
            }
            if prior.response_state != ResponseState::Complete {
                prior.response_state = ResponseState::Observing;
                prior.completed_utc = None;
                prior.validate()?;
                atomic(&path, &prior)?;
            }
            return Ok(());
        }
        let timer = TurnTimer {
            schema_version: PROTOCOL_VERSION,
            event_id: delivery.event.event_id.clone(),
            project_id: delivery.event.project_id.clone(),
            target_generation: delivery.target.generation,
            target_digest: delivery.target.digest.clone(),
            started_utc: now(),
            completed_utc: None,
            response_state: ResponseState::Observing,
        };
        timer.validate()?;
        atomic(&path, &timer)
    }

    /// Reconcile the passive response timer once the exact durable USER
    /// receipt is proven. If SUBMISSION_ACCEPTED already started the timer,
    /// preserve that original start instant rather than shifting the measured
    /// turn to the later persistence-check boundary.
    pub fn record_exact_receipt_timer(&self, id: &str, receipt: &Receipt) -> Result<()> {
        let _lock = self.lock()?;
        let delivery = self.read_delivery(id)?.ok_or("TIMER_EVENT_MISSING")?;
        if !matches!(delivery.phase, Phase::Submitting | Phase::Sent)
            || receipt.schema_version != PROTOCOL_VERSION
            || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
            || receipt.event_id != delivery.event.event_id
            || receipt.target_generation != delivery.target.generation
            || receipt.target_digest != delivery.target.digest
            || receipt.message_digest != delivery.message_digest
            || receipt.sent_utc == 0
        {
            return Err("TIMER_RECEIPT_INVALID".into());
        }
        if let Some(committed) = delivery.receipt.as_ref()
            && committed != receipt
        {
            return Err("TIMER_RECEIPT_BINDING_MISMATCH".into());
        }
        let config = self.read_config()?;
        if config.targets.get(&delivery.event.project_id) != Some(&delivery.target) {
            return Err("TARGET_GENERATION_STALE".into());
        }
        let path = self.timer_path(&delivery.event.event_id);
        if let Some(mut prior) = self.read_timer(&delivery.event.event_id)? {
            if prior.project_id != delivery.event.project_id
                || prior.target_generation != delivery.target.generation
                || prior.target_digest != delivery.target.digest
            {
                return Err("TIMER_RECEIPT_BINDING_MISMATCH".into());
            }
            if prior.response_state != ResponseState::Complete {
                prior.response_state = ResponseState::Observing;
                prior.completed_utc = None;
                prior.validate()?;
                atomic(&path, &prior)?;
            }
            return Ok(());
        }
        let timer = TurnTimer {
            schema_version: PROTOCOL_VERSION,
            event_id: delivery.event.event_id.clone(),
            project_id: delivery.event.project_id.clone(),
            target_generation: delivery.target.generation,
            target_digest: delivery.target.digest.clone(),
            started_utc: receipt.sent_utc,
            completed_utc: None,
            response_state: ResponseState::Observing,
        };
        timer.validate()?;
        atomic(&path, &timer)
    }

    pub fn set_timer_response_state(&self, event_id: &str, state: ResponseState) -> Result<()> {
        let _lock = self.lock()?;
        let mut timer = self.read_timer(event_id)?.ok_or("TIMER_EVENT_MISSING")?;
        if timer.response_state == ResponseState::Complete {
            return if state == ResponseState::Complete {
                Ok(())
            } else {
                Err("TIMER_TERMINAL".into())
            };
        }
        if state == ResponseState::Complete {
            return Err("TIMER_COMPLETION_REQUIRES_SENT".into());
        }
        let delivery = self.read_delivery(event_id)?.ok_or("TIMER_EVENT_MISSING")?;
        if !matches!(delivery.phase, Phase::Submitting | Phase::Sent)
            || timer.project_id != delivery.event.project_id
            || timer.target_generation != delivery.target.generation
            || timer.target_digest != delivery.target.digest
        {
            return Err("TIMER_STATE_REFUSED".into());
        }
        timer.response_state = state;
        timer.completed_utc = None;
        timer.validate()?;
        atomic(&self.timer_path(event_id), &timer)
    }

    /// Freeze a passive response timer only after browser observation reports
    /// completion for its already-received USER event.
    pub fn complete_timer(&self, event_id: &str) -> Result<()> {
        let _lock = self.lock()?;
        let mut timer = self.read_timer(event_id)?.ok_or("TIMER_EVENT_MISSING")?;
        let delivery = self.read_delivery(event_id)?.ok_or("TIMER_EVENT_MISSING")?;
        if delivery.phase != Phase::Sent
            || delivery.receipt.as_ref().is_none_or(|receipt| {
                receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
                    || receipt.event_id != delivery.event.event_id
                    || receipt.target_generation != timer.target_generation
                    || receipt.target_digest != timer.target_digest
                    || receipt.message_digest != delivery.message_digest
            })
            || timer.project_id != delivery.event.project_id
            || timer.target_generation != delivery.target.generation
            || timer.target_digest != delivery.target.digest
        {
            return Err("TIMER_COMPLETION_REFUSED".into());
        }
        if timer.response_state == ResponseState::Complete {
            return Ok(());
        }
        timer.response_state = ResponseState::Complete;
        timer.completed_utc = Some(now());
        timer.validate()?;
        atomic(&self.timer_path(event_id), &timer)
    }

    /// Persist the specific observe-only reconciliation failure for an already
    /// SUBMITTING delivery without changing its phase or permitting replay.
    /// This preserves the original failure across host loops while keeping the
    /// submission boundary immutable.
    pub(crate) fn note_submitting_attention(&self, id: &str, reason: &str) -> Result<Delivery> {
        if !valid_id(reason) {
            return Err("SUBMISSION_ATTENTION_INVALID".into());
        }
        let _lock = self.lock()?;
        let mut delivery = self.read_delivery(id)?.ok_or("SUBMISSION_CLAIM_MISSING")?;
        if delivery.phase != Phase::Submitting || delivery.receipt.is_some() {
            return Err("SUBMISSION_RECONCILIATION_REFUSED".into());
        }
        delivery.reason = Some(reason.into());
        delivery.updated_utc = now();
        delivery.validate()?;
        atomic(&self.delivery_path(id), &delivery)?;
        Ok(delivery)
    }

    pub fn retry_pre_submit(&self, id: &str) -> Result<()> {
        let _lock = self.lock()?;
        let mut delivery = self.read_delivery(id)?.ok_or("SUBMISSION_CLAIM_MISSING")?;
        if delivery.phase != Phase::Attention || delivery.receipt.is_some() {
            return Err("SUBMISSION_RETRY_REFUSED".into());
        }
        if self.read_config()?.targets.get(&delivery.event.project_id) != Some(&delivery.target) {
            return Err("TARGET_GENERATION_STALE".into());
        }
        delivery.phase = Phase::Claimed;
        delivery.reason = None;
        delivery.updated_utc = now();
        atomic(&self.delivery_path(id), &delivery)
    }

    /// Retire one exact pre-submit queue event without deleting its evidence.
    /// SENT and SUBMITTING are immutable/ambiguous delivery boundaries and are
    /// always refused. The original event is copied to archive and the durable
    /// delivery journal records STALE + OPERATOR_RETIRED before queue removal.
    pub fn retire_stale(&self, id: &str) -> Result<()> {
        if !valid_id(id) {
            return Err("QUEUE_EVENT_INVALID".into());
        }
        let _lock = self.lock()?;
        let event_path = self.event_path(id);
        let archive_path = self.archive_path(id);
        if !event_path.try_exists().map_err(|_| "QUEUE_UNAVAILABLE")? {
            if archive_path.try_exists().map_err(|_| "QUEUE_UNAVAILABLE")? {
                let archived: Event = read(&archive_path)?;
                archived.validate()?;
                if archived.event_id != id {
                    return Err("QUEUE_ID_MISMATCH".into());
                }
                let prior = self.read_delivery(id)?.ok_or("STALE_RETIREMENT_REFUSED")?;
                if prior.event == archived
                    && prior.phase == Phase::Stale
                    && prior.receipt.is_none()
                    && prior.reason.as_deref() == Some("OPERATOR_RETIRED")
                {
                    return Ok(());
                }
                return Err("STALE_RETIREMENT_REFUSED".into());
            }
            return Err("STATE_UNAVAILABLE".into());
        }
        let event: Event = read(&event_path)?;
        event.validate()?;
        if event.event_id != id {
            return Err("QUEUE_ID_MISMATCH".into());
        }
        let config = self.read_config()?;
        let prior = self.read_delivery(id)?;
        let historical_target = prior
            .as_ref()
            .map(|delivery| delivery.target.clone())
            .or_else(|| {
                config
                    .targets
                    .get(&event.project_id)
                    .filter(|target| target.generation == event.target_generation)
                    .cloned()
                    .or_else(|| {
                        config
                            .history
                            .iter()
                            .rev()
                            .find(|change| {
                                change.project_id == event.project_id
                                    && change.target.generation == event.target_generation
                            })
                            .map(|change| change.target.clone())
                    })
            })
            .ok_or("TARGET_GENERATION_STALE")?;
        if prior
            .as_ref()
            .is_some_and(|delivery| matches!(delivery.phase, Phase::Submitting | Phase::Sent))
        {
            return Err("STALE_RETIREMENT_REFUSED".into());
        }
        if let Some(delivery) = prior.as_ref()
            && (delivery.event != event || delivery.receipt.is_some())
        {
            return Err("STALE_RETIREMENT_REFUSED".into());
        }
        let delivery = Delivery {
            schema_version: PROTOCOL_VERSION,
            event: event.clone(),
            target: historical_target,
            phase: Phase::Stale,
            owner: prior.as_ref().map_or_else(
                || "operator-retire".into(),
                |delivery| delivery.owner.clone(),
            ),
            updated_utc: now(),
            message_digest: digest(&event.message),
            receipt: None,
            reason: Some("OPERATOR_RETIRED".into()),
        };
        delivery.validate()?;
        atomic(&self.delivery_path(id), &delivery)?;
        atomic(&self.archive_path(id), &event)?;
        fs::remove_file(self.event_path(id)).map_err(|_| "QUEUE_ARCHIVE_FAILED".into())
    }
    pub fn host_lock(&self) -> Result<HostLease> {
        let path = self.root.join("host.lock");
        reject_link(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| "HOST_LOCK_UNAVAILABLE")?;
        file.try_lock().map_err(|_| "HOST_ALREADY_RUNNING")?;
        Ok(HostLease {
            _lock: file,
            root: self.root.clone(),
            owner: format!(
                "host-{}-{}-{}",
                std::process::id(),
                now(),
                TEMP.fetch_add(1, Ordering::Relaxed)
            ),
        })
    }
}

pub fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    reject_link(path)?;
    let file = File::open(path).map_err(|_| "STATE_UNAVAILABLE")?;
    if !file.metadata().map_err(|_| "STATE_UNAVAILABLE")?.is_file() {
        return Err("STATE_NOT_REGULAR".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "STATE_UNAVAILABLE")?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("STATE_TOO_LARGE".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "STATE_MALFORMED".into())
}

pub fn atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    reject_link(path)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "STATE_SERIALIZATION_FAILED")?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("STATE_TOO_LARGE".into());
    }
    let tmp = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|_| "STATE_WRITE_FAILED")?;
    let result = (|| {
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "STATE_WRITE_FAILED")?;
        drop(file);
        replace(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(windows)]
fn transient_replace_error(code: u32) -> bool {
    matches!(code, 5 | 32 | 33)
}

#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        fn GetLastError() -> u32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // Same-directory replacement, with write-through before acknowledging commit.
    // Windows readers/AV can briefly hold the destination without FILE_SHARE_DELETE.
    // Retry only known transient sharing/access errors and remain fail-closed otherwise.
    for attempt in 0..20 {
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } != 0 {
            return Ok(());
        }
        let code = unsafe { GetLastError() };
        if !transient_replace_error(code) || attempt == 19 {
            return Err("STATE_REPLACE_FAILED".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Err("STATE_REPLACE_FAILED".into())
}
#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to).map_err(|_| "STATE_REPLACE_FAILED")?;
    File::open(to.parent().ok_or("STATE_PARENT_MISSING")?)
        .and_then(|f| f.sync_all())
        .map_err(|_| "STATE_SYNC_FAILED".into())
}

fn reject_link(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if m.file_attributes() & 0x400 != 0 {
                    return Err("STATE_REPARSE_REFUSED".into());
                }
            }
            if m.file_type().is_symlink() {
                return Err("STATE_LINK_REFUSED".into());
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("STATE_UNAVAILABLE".into()),
    }
}
fn safe_directories(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err("CONFIG_ROOT_NOT_ABSOLUTE".into());
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        if matches!(part, std::path::Component::ParentDir) {
            return Err("CONFIG_ROOT_INVALID".into());
        }
        current.push(part);
        reject_link(&current)?;
        if !current
            .try_exists()
            .map_err(|_| "CONFIG_ROOT_UNAVAILABLE")?
        {
            fs::create_dir(&current).map_err(|_| "CONFIG_ROOT_UNAVAILABLE")?;
        }
        if !current.is_dir() {
            return Err("CONFIG_ROOT_NOT_DIRECTORY".into());
        }
    }
    Ok(())
}

#[cfg(feature = "test-support")]
fn safe_directories_beneath(trusted_parent: &Path, path: &Path) -> Result<()> {
    if !trusted_parent.is_absolute() || !path.is_absolute() {
        return Err("CONFIG_ROOT_NOT_ABSOLUTE".into());
    }
    let relative = path
        .strip_prefix(trusted_parent)
        .map_err(|_| "CONFIG_ROOT_OUTSIDE_TEST_PARENT")?;
    if relative.as_os_str().is_empty() {
        return Err("CONFIG_ROOT_INVALID".into());
    }
    let mut current = trusted_parent.to_path_buf();
    for part in relative.components() {
        match part {
            std::path::Component::Normal(value) => current.push(value),
            _ => return Err("CONFIG_ROOT_INVALID".into()),
        }
        reject_link(&current)?;
        if !current
            .try_exists()
            .map_err(|_| "CONFIG_ROOT_UNAVAILABLE")?
        {
            fs::create_dir(&current).map_err(|_| "CONFIG_ROOT_UNAVAILABLE")?;
        }
        if !current.is_dir() {
            return Err("CONFIG_ROOT_NOT_DIRECTORY".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod submission_reconciliation_tests {
    use super::*;

    fn fixture(label: &str) -> (PathBuf, Store, Event, HostLease) {
        let base = std::env::temp_dir().join(format!(
            "catdesk-wake-reconcile-{label}-{}-{}",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("wake");
        fs::create_dir_all(&base).expect("fixture parent");
        let store = Store::open_scoped_for_test(&root, &base).expect("store");
        store.initialize().expect("initialize");
        let target = store
            .set_target(
                "catdesk",
                0,
                "https://chatgpt.com/c/6ab06a56-56c0-83e9-9697-9bc7c380c154",
            )
            .expect("target");
        let event = store
            .produce(
                &format!("reconcile-{label}"),
                "catdesk",
                "review_ready",
                "CatDesk reconciliation fixture",
            )
            .expect("event");
        assert_eq!(event.target_generation, target.generation);
        let lease = store.host_lock().expect("lease");
        store.claim(&event, &lease).expect("claim");
        store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Submitting,
                None,
                None,
            )
            .expect("submitting");
        (base, store, event, lease)
    }

    fn receipt(event: &Event, target: &Target) -> Receipt {
        Receipt {
            schema_version: PROTOCOL_VERSION,
            event_id: event.event_id.clone(),
            target_generation: target.generation,
            target_digest: target.digest.clone(),
            message_digest: digest(&event.message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        }
    }

    #[test]
    fn submitting_can_reconcile_to_sent_only_with_exact_bound_receipt() {
        let (root, store, event, _lease) = fixture("success");
        let target = store.config().expect("config").targets["catdesk"].clone();
        let reconciled = store
            .reconcile_submitting_sent(&event.event_id, receipt(&event, &target))
            .expect("reconcile");
        assert_eq!(reconciled.phase, Phase::Sent);
        assert!(reconciled.receipt.is_some());
        assert!(!store.event_path(&event.event_id).exists());
        assert!(store.archive_path(&event.event_id).exists());
        assert_eq!(
            store
                .reconcile_submitting_sent(&event.event_id, receipt(&event, &target))
                .unwrap_err(),
            "SUBMISSION_RECONCILIATION_REFUSED"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn submitting_reconciliation_refuses_mismatched_receipt_without_state_change() {
        let (root, store, event, _lease) = fixture("mismatch");
        let target = store.config().expect("config").targets["catdesk"].clone();
        let mut wrong = receipt(&event, &target);
        wrong.message_digest = digest("different");
        assert_eq!(
            store
                .reconcile_submitting_sent(&event.event_id, wrong)
                .unwrap_err(),
            "RECEIPT_BINDING_MISMATCH"
        );
        let persisted = store
            .delivery(&event.event_id)
            .expect("delivery")
            .expect("present");
        assert_eq!(persisted.phase, Phase::Submitting);
        assert!(persisted.receipt.is_none());
        assert!(store.event_path(&event.event_id).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn submitting_attention_preserves_specific_reason_without_enabling_replay() {
        let (root, store, event, lease) = fixture("attention");
        let noted = store
            .note_submitting_attention(&event.event_id, "SUBMIT_CLEARED_NO_APPEND")
            .expect("note attention");
        assert_eq!(noted.phase, Phase::Submitting);
        assert_eq!(noted.reason.as_deref(), Some("SUBMIT_CLEARED_NO_APPEND"));
        assert!(noted.receipt.is_none());
        assert_eq!(
            store.claim(&event, &lease).unwrap_err(),
            "SUBMISSION_RECONCILIATION_REQUIRED"
        );
        assert_eq!(
            store.retry_pre_submit(&event.event_id).unwrap_err(),
            "SUBMISSION_RETRY_REFUSED"
        );
        let reread = store
            .delivery(&event.event_id)
            .expect("delivery")
            .expect("present");
        assert_eq!(reread.phase, Phase::Submitting);
        assert_eq!(reread.reason.as_deref(), Some("SUBMIT_CLEARED_NO_APPEND"));
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(all(test, windows))]
mod windows_replace_tests {
    use super::transient_replace_error;

    #[test]
    fn only_known_windows_sharing_failures_are_retryable() {
        assert!(transient_replace_error(5));
        assert!(transient_replace_error(32));
        assert!(transient_replace_error(33));
        assert!(!transient_replace_error(2));
        assert!(!transient_replace_error(87));
    }
}

#[cfg(test)]
mod turn_timer_tests {
    use super::*;

    fn fixture() -> (PathBuf, Store, Event, HostLease, Receipt) {
        let base = std::env::temp_dir().join(format!(
            "catdesk-wake-turn-timer-{}",
            TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("wake");
        fs::create_dir_all(&base).expect("fixture parent");
        let store = Store::open_scoped_for_test(&root, &base).expect("store");
        store.initialize().expect("initialize");
        let target = store
            .set_target("catdesk", 0, "https://chatgpt.com/c/turn-timer-fixture")
            .expect("target");
        let event = store
            .produce(
                "turn-timer-parent",
                "catdesk",
                "review_ready",
                "turn timer fixture",
            )
            .expect("event");
        let lease = store.host_lock().expect("lease");
        let claimed = store.claim(&event, &lease).expect("claim");
        store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Submitting,
                None,
                None,
            )
            .expect("submitting");
        let receipt = Receipt {
            schema_version: PROTOCOL_VERSION,
            event_id: event.event_id.clone(),
            target_generation: target.generation,
            target_digest: target.digest,
            message_digest: claimed.message_digest,
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        (base, store, event, lease, receipt)
    }

    #[test]
    fn submission_acceptance_starts_timer_and_later_receipt_preserves_start() {
        let (root, store, event, _lease, mut receipt) = fixture();
        store
            .start_submission_timer(&event.event_id)
            .expect("submission timer");
        let accepted = store.timer(&event.event_id).unwrap().unwrap();
        assert_eq!(accepted.response_state, ResponseState::Observing);
        assert!(accepted.completed_utc.is_none());

        receipt.sent_utc = accepted.started_utc + 100;
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .expect("durable receipt reconciles timer");
        let reread = store.timer(&event.event_id).unwrap().unwrap();
        assert_eq!(reread.started_utc, accepted.started_utc);
        assert_eq!(reread.response_state, ResponseState::Observing);
        assert!(reread.completed_utc.is_none());
        let delivery = store.delivery(&event.event_id).unwrap().unwrap();
        assert_eq!(delivery.phase, Phase::Submitting);
        assert!(delivery.receipt.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_user_receipt_starts_timer_before_final_sent_and_is_restart_idempotent() {
        let (root, store, event, _lease, receipt) = fixture();
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .expect("timer");
        let timer = store.timer(&event.event_id).unwrap().unwrap();
        assert_eq!(timer.started_utc, receipt.sent_utc);
        assert_eq!(timer.response_state, ResponseState::Observing);
        assert!(timer.completed_utc.is_none());

        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .expect("idempotent timer");
        let mut later_reconciliation = receipt.clone();
        later_reconciliation.sent_utc = receipt.sent_utc + 100;
        store
            .record_exact_receipt_timer(&event.event_id, &later_reconciliation)
            .expect("later reconciliation preserves original timer start");
        let reread = store.timer(&event.event_id).unwrap().unwrap();
        assert_eq!(reread.started_utc, receipt.sent_utc);
        assert_eq!(reread.response_state, ResponseState::Observing);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn retrying_and_attention_are_passive_and_completion_requires_final_sent() {
        let (root, store, event, lease, receipt) = fixture();
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .expect("timer");
        store
            .set_timer_response_state(&event.event_id, ResponseState::Retrying)
            .expect("retrying");
        assert_eq!(
            store
                .timer(&event.event_id)
                .unwrap()
                .unwrap()
                .response_state,
            ResponseState::Retrying
        );
        store
            .set_timer_response_state(&event.event_id, ResponseState::Attention)
            .expect("attention");
        assert_eq!(
            store
                .timer(&event.event_id)
                .unwrap()
                .unwrap()
                .response_state,
            ResponseState::Attention
        );
        assert_eq!(
            store.complete_timer(&event.event_id).unwrap_err(),
            "TIMER_COMPLETION_REFUSED"
        );

        let delivery = store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Sent,
                Some(receipt.clone()),
                None,
            )
            .expect("sent");
        assert_eq!(delivery.phase, Phase::Sent);
        store.complete_timer(&event.event_id).expect("complete");
        let timer = store.timer(&event.event_id).unwrap().unwrap();
        assert_eq!(timer.response_state, ResponseState::Complete);
        assert!(timer.completed_utc.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn timer_refuses_wrong_receipt_binding_without_queue_replay() {
        let (root, store, event, _lease, mut receipt) = fixture();
        receipt.message_digest = "0".repeat(64);
        assert_eq!(
            store
                .record_exact_receipt_timer(&event.event_id, &receipt)
                .unwrap_err(),
            "TIMER_RECEIPT_INVALID"
        );
        assert!(store.timer(&event.event_id).unwrap().is_none());
        assert_eq!(store.events().unwrap(), vec![event]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manual_turn_timer_is_durable_idempotent_target_bound_and_terminal() {
        let (root, store, event, _lease, _receipt) = fixture();
        let target = store.config().unwrap().targets["catdesk"].clone();
        let id = "manual-turn-store-test";

        let first = store
            .start_manual_turn_timer(id, "catdesk", &target)
            .expect("start manual timer");
        assert_eq!(first.response_state, ResponseState::Observing);
        assert!(first.completed_utc.is_none());

        let repeated = store
            .start_manual_turn_timer(id, "catdesk", &target)
            .expect("restart-idempotent start");
        assert_eq!(repeated.started_utc, first.started_utc);
        assert_eq!(
            store.timer(id).unwrap().unwrap().started_utc,
            first.started_utc
        );

        let wrong_target = Target::new(
            target.generation + 1,
            "https://chatgpt.com/c/wrong-turn-target".into(),
        )
        .expect("wrong target");
        assert_eq!(
            store
                .start_manual_turn_timer("manual-turn-wrong-target", "catdesk", &wrong_target)
                .unwrap_err(),
            "MANUAL_TURN_TARGET_MISMATCH"
        );

        let stopped = store
            .complete_manual_turn_timer(id, "catdesk", &target)
            .expect("stop manual timer");
        assert_eq!(stopped.response_state, ResponseState::Complete);
        assert!(stopped.completed_utc.is_some());
        let stopped_again = store
            .complete_manual_turn_timer(id, "catdesk", &target)
            .expect("idempotent stop");
        assert_eq!(stopped_again.completed_utc, stopped.completed_utc);
        assert_eq!(
            store
                .start_manual_turn_timer(id, "catdesk", &target)
                .unwrap_err(),
            "MANUAL_TURN_TERMINAL"
        );
        assert_eq!(store.events().unwrap(), vec![event]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manual_turn_timer_refuses_wake_authority_collision() {
        let (root, store, _event, _lease, _receipt) = fixture();
        let target = store.config().unwrap().targets["catdesk"].clone();
        let collision = store
            .produce(
                "manual-turn-authority-collision",
                "catdesk",
                "review_ready",
                "reserved namespace collision fixture",
            )
            .expect("collision event");
        assert_eq!(
            store
                .start_manual_turn_timer(&collision.event_id, "catdesk", &target)
                .unwrap_err(),
            "MANUAL_TURN_AUTHORITY_COLLISION"
        );
        assert!(store.timer(&collision.event_id).unwrap().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dev67_turn_timer_json_remains_schema_compatible() {
        let json = format!(
            r#"{{"schemaVersion":1,"eventId":"manual-turn-dev67-compat","projectId":"catdesk","targetGeneration":17,"targetDigest":"{}","startedUtc":100,"completedUtc":null,"responseState":"OBSERVING"}}"#,
            "a".repeat(64)
        );
        let timer: TurnTimer = serde_json::from_str(&json).expect("dev67 timer json");
        timer.validate().expect("dev67 timer remains valid");
        assert_eq!(timer.event_id, "manual-turn-dev67-compat");
        assert_eq!(timer.response_state, ResponseState::Observing);
    }

    #[test]
    fn manual_stop_cannot_complete_a_wake_event_timer() {
        let (root, store, event, _lease, receipt) = fixture();
        let target = store.config().unwrap().targets["catdesk"].clone();
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .expect("wake timer");
        assert_eq!(
            store
                .complete_manual_turn_timer(&event.event_id, "catdesk", &target)
                .unwrap_err(),
            "MANUAL_TURN_INVALID"
        );
        assert_eq!(
            store
                .timer(&event.event_id)
                .unwrap()
                .unwrap()
                .response_state,
            ResponseState::Observing
        );
        let _ = fs::remove_dir_all(root);
    }
}
