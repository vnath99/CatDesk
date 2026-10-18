//! Durable, project-scoped ownership for central Web work requests.
//! The caller holds the project registration lock while changing this ledger.

use catdesk_wake::protocol::{self, Target};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

const MAX_RECORDS: usize = 128;
const MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkRecord {
    pub(crate) request_id: String,
    pub(crate) event_id: String,
    pub(crate) message_sha256: String,
    pub(crate) target: Target,
    pub(crate) phase: String,
    pub(crate) outcome: Option<String>,
    pub(crate) summary_sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LedgerFile {
    schema_version: u32,
    project_id: String,
    records: Vec<WorkRecord>,
}

pub(crate) struct WorkLedger {
    path: PathBuf,
    project_id: String,
}

fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_request(value: &str) -> bool {
    value.len() <= 128 && protocol::valid_id(value)
}

fn plain(path: &Path, directory: bool) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "WEB_WORK_LEDGER_UNAVAILABLE")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("WEB_WORK_LEDGER_UNSAFE".into());
        }
    }
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err("WEB_WORK_LEDGER_UNSAFE".into());
    }
    Ok(())
}

fn expected_event(project: &str, request: &str) -> String {
    format!(
        "web-work-{}",
        protocol::digest(&format!("{project}\0{request}"))
    )
}

impl LedgerFile {
    fn empty(project_id: &str) -> Self {
        Self {
            schema_version: 1,
            project_id: project_id.into(),
            records: Vec::new(),
        }
    }

    fn validate(&self, project_id: &str) -> Result<(), String> {
        if self.schema_version != 1
            || self.project_id != project_id
            || self.records.len() > MAX_RECORDS
        {
            return Err("WEB_WORK_LEDGER_INVALID".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut open = 0;
        for record in &self.records {
            if !valid_request(&record.request_id)
                || !ids.insert(&record.request_id)
                || record.event_id != expected_event(project_id, &record.request_id)
                || !sha256(&record.message_sha256)
                || record.target.validate().is_err()
            {
                return Err("WEB_WORK_LEDGER_INVALID".into());
            }
            match record.phase.as_str() {
                "OPEN" if record.outcome.is_none() && record.summary_sha256.is_none() => {
                    open += 1;
                }
                "CLOSED"
                    if matches!(record.outcome.as_deref(), Some("completed" | "retired"))
                        && record.summary_sha256.as_deref().is_some_and(sha256) => {}
                _ => return Err("WEB_WORK_LEDGER_INVALID".into()),
            }
        }
        if open > 1 {
            return Err("WEB_WORK_LEDGER_INVALID".into());
        }
        Ok(())
    }
}

impl WorkLedger {
    pub(crate) fn open(workspace: &Path, project_id: &str) -> Result<Self, String> {
        let ledger = Self::at(workspace, project_id)?;
        let parent = ledger.path.parent().ok_or("WEB_WORK_LEDGER_UNSAFE")?;
        let private = parent.parent().ok_or("WEB_WORK_LEDGER_UNSAFE")?;
        plain(private, true)?;
        match fs::symlink_metadata(parent) {
            Ok(_) => plain(parent, true)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(parent).map_err(|_| "WEB_WORK_LEDGER_UNAVAILABLE")?;
                plain(parent, true)?;
            }
            Err(_) => return Err("WEB_WORK_LEDGER_UNAVAILABLE".into()),
        }
        ledger.load()?;
        Ok(ledger)
    }

    fn at(workspace: &Path, project_id: &str) -> Result<Self, String> {
        if !valid_request(project_id) {
            return Err("WEB_WORK_PROJECT_INVALID".into());
        }
        let workspace = workspace
            .canonicalize()
            .map_err(|_| "WEB_WORK_LEDGER_UNAVAILABLE")?;
        plain(&workspace, true)?;
        Ok(Self {
            path: workspace
                .join(".catdesk")
                .join("web-work")
                .join(format!("{project_id}.json")),
            project_id: project_id.into(),
        })
    }

    fn load(&self) -> Result<LedgerFile, String> {
        let parent = self.path.parent().ok_or("WEB_WORK_LEDGER_UNSAFE")?;
        plain(parent.parent().ok_or("WEB_WORK_LEDGER_UNSAFE")?, true)?;
        plain(parent, true)?;
        match fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LedgerFile::empty(&self.project_id));
            }
            Err(_) => return Err("WEB_WORK_LEDGER_UNAVAILABLE".into()),
            Ok(_) => plain(&self.path, false)?,
        }
        let mut bytes = Vec::new();
        File::open(&self.path)
            .map_err(|_| "WEB_WORK_LEDGER_UNAVAILABLE")?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "WEB_WORK_LEDGER_UNAVAILABLE")?;
        if bytes.len() > MAX_BYTES {
            return Err("WEB_WORK_LEDGER_INVALID".into());
        }
        let state: LedgerFile =
            serde_json::from_slice(&bytes).map_err(|_| "WEB_WORK_LEDGER_INVALID")?;
        state.validate(&self.project_id)?;
        Ok(state)
    }

    fn save(&self, state: &LedgerFile) -> Result<(), String> {
        state.validate(&self.project_id)?;
        let mut bytes = serde_json::to_vec(state).map_err(|_| "WEB_WORK_LEDGER_INVALID")?;
        bytes.push(b'\n');
        if bytes.len() > MAX_BYTES {
            return Err("WEB_WORK_LEDGER_FULL".into());
        }
        let parent = self.path.parent().ok_or("WEB_WORK_LEDGER_UNSAFE")?;
        plain(parent.parent().ok_or("WEB_WORK_LEDGER_UNSAFE")?, true)?;
        plain(parent, true)?;
        match fs::symlink_metadata(&self.path) {
            Ok(_) => plain(&self.path, false)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("WEB_WORK_LEDGER_UNAVAILABLE".into()),
        }
        let temporary = parent.join(format!(".{}.tmp", Uuid::new_v4()));
        let outcome = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(|_| "WEB_WORK_LEDGER_UNAVAILABLE")?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "WEB_WORK_LEDGER_UNAVAILABLE")?;
            drop(file);
            replace_atomic(&temporary, &self.path)
        })();
        if outcome.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        outcome?;
        if self.load()? != *state {
            return Err("WEB_WORK_LEDGER_READBACK_CHANGED".into());
        }
        Ok(())
    }

    pub(crate) fn prepare(
        &self,
        request_id: &str,
        event_id: &str,
        message_sha256: &str,
        target: &Target,
    ) -> Result<String, String> {
        if !valid_request(request_id)
            || event_id != expected_event(&self.project_id, request_id)
            || !sha256(message_sha256)
            || target.validate().is_err()
        {
            return Err("WEB_WORK_LEDGER_ARGUMENTS_INVALID".into());
        }
        let mut state = self.load()?;
        if let Some(existing) = state
            .records
            .iter()
            .find(|record| record.request_id == request_id)
        {
            if existing.event_id != event_id
                || existing.message_sha256 != message_sha256
                || existing.target != *target
            {
                return Err("WEB_WORK_REQUEST_CONFLICT".into());
            }
            return Ok(if existing.phase == "OPEN" {
                "IDEMPOTENT_OPEN"
            } else {
                "IDEMPOTENT_CLOSED"
            }
            .into());
        }
        if state.records.iter().any(|record| record.phase == "OPEN") {
            return Err("WEB_WORK_ACTIVE_REQUEST".into());
        }
        if state.records.len() == MAX_RECORDS {
            return Err("WEB_WORK_LEDGER_FULL".into());
        }
        state.records.push(WorkRecord {
            request_id: request_id.into(),
            event_id: event_id.into(),
            message_sha256: message_sha256.into(),
            target: target.clone(),
            phase: "OPEN".into(),
            outcome: None,
            summary_sha256: None,
        });
        self.save(&state)?;
        Ok("OPEN_NEW".into())
    }

    pub(crate) fn record(&self, request_id: &str) -> Result<Option<WorkRecord>, String> {
        if !valid_request(request_id) {
            return Err("WEB_WORK_LEDGER_ARGUMENTS_INVALID".into());
        }
        Ok(self
            .load()?
            .records
            .into_iter()
            .find(|record| record.request_id == request_id))
    }

    pub(crate) fn resolve(
        &self,
        request_id: &str,
        target: &Target,
        outcome: &str,
        summary: &str,
    ) -> Result<Value, String> {
        if !valid_request(request_id)
            || target.validate().is_err()
            || !matches!(outcome, "completed" | "retired")
            || summary.trim().is_empty()
            || summary.len() > 1024
            || summary
                .chars()
                .any(|value| value.is_control() && !matches!(value, '\n' | '\r' | '\t'))
        {
            return Err("WEB_WORK_LEDGER_ARGUMENTS_INVALID".into());
        }
        let digest = protocol::digest(summary);
        let mut state = self.load()?;
        let record = state
            .records
            .iter_mut()
            .find(|record| record.request_id == request_id)
            .ok_or("WEB_WORK_REQUEST_NOT_FOUND")?;
        if record.target != *target {
            return Err("WEB_WORK_TARGET_CHANGED".into());
        }
        let event_id = record.event_id.clone();
        if record.phase == "CLOSED" {
            if record.outcome.as_deref() != Some(outcome)
                || record.summary_sha256.as_deref() != Some(digest.as_str())
            {
                return Err("WEB_WORK_REQUEST_CONFLICT".into());
            }
        } else {
            record.phase = "CLOSED".into();
            record.outcome = Some(outcome.into());
            record.summary_sha256 = Some(digest.clone());
            self.save(&state)?;
        }
        Ok(json!({"projectId":self.project_id,"requestId":request_id,
            "eventId":event_id,"phase":"CLOSED","outcome":outcome,
            "summarySha256":digest,"taskCompletionVerified":false}))
    }

    /// A status refresh never creates a ledger directory or file.
    pub(crate) fn status(workspace: &Path, project_id: &str) -> Value {
        let result = (|| -> Result<Value, String> {
            let ledger = Self::at(workspace, project_id)?;
            let private = ledger
                .path
                .parent()
                .and_then(Path::parent)
                .ok_or("WEB_WORK_LEDGER_UNSAFE")?;
            for directory in [
                private,
                ledger.path.parent().ok_or("WEB_WORK_LEDGER_UNSAFE")?,
            ] {
                match fs::symlink_metadata(directory) {
                    Ok(_) => plain(directory, true)?,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(json!({"status":"UNOBSERVED","active":null,"recordCount":0}));
                    }
                    Err(_) => return Err("WEB_WORK_LEDGER_UNAVAILABLE".into()),
                }
            }
            let state = ledger.load()?;
            let active = state.records.iter().find(|record| record.phase == "OPEN");
            Ok(
                json!({"status":"DURABLE_SNAPSHOT","recordCount":state.records.len(),
                "active":active.map(|record| json!({"requestId":record.request_id,
                    "eventId":record.event_id,"targetGeneration":record.target.generation,
                    "targetSha256":record.target.digest,"messageSha256":record.message_sha256,
                    "phase":record.phase}))}),
            )
        })();
        result.unwrap_or_else(
            |_| json!({"status":"ATTENTION","reason":"WEB_WORK_LEDGER_UNAVAILABLE"}),
        )
    }
}

#[cfg(windows)]
fn replace_atomic(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
    }
    let source = source
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) } == 0 {
        return Err("WEB_WORK_LEDGER_UNAVAILABLE".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_atomic(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|_| "WEB_WORK_LEDGER_UNAVAILABLE".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, WorkLedger, WorkLedger) {
        let workspace = std::env::temp_dir().join(format!("web-work-ledger-{}", Uuid::new_v4()));
        fs::create_dir_all(workspace.join(".catdesk")).unwrap();
        let alpha = WorkLedger::open(&workspace, "alpha").unwrap();
        let beta = WorkLedger::open(&workspace, "beta").unwrap();
        (workspace, alpha, beta)
    }

    fn target() -> Target {
        Target::new(1, "https://chatgpt.com/c/ledger-fixture".into()).unwrap()
    }

    fn prepare(ledger: &WorkLedger, request: &str, message: &str) -> Result<String, String> {
        ledger.prepare(
            request,
            &expected_event(&ledger.project_id, request),
            &protocol::digest(message),
            &target(),
        )
    }

    #[test]
    fn exact_replay_and_distinct_active_request_are_project_scoped() {
        let (workspace, alpha, beta) = fixture();
        assert_eq!(prepare(&alpha, "one", "body").unwrap(), "OPEN_NEW");
        assert_eq!(prepare(&alpha, "one", "body").unwrap(), "IDEMPOTENT_OPEN");
        assert_eq!(
            prepare(&alpha, "one", "changed").unwrap_err(),
            "WEB_WORK_REQUEST_CONFLICT"
        );
        assert_eq!(
            prepare(&alpha, "two", "body").unwrap_err(),
            "WEB_WORK_ACTIVE_REQUEST"
        );
        assert_eq!(prepare(&beta, "one", "body").unwrap(), "OPEN_NEW");
        assert_eq!(alpha.record("one").unwrap().unwrap().phase, "OPEN");
        assert_eq!(beta.record("one").unwrap().unwrap().phase, "OPEN");
        assert_eq!(
            WorkLedger::status(&workspace, "alpha")["active"]["requestId"],
            "one"
        );
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn closing_keeps_history_and_exact_replay_without_claiming_completion() {
        let (workspace, alpha, _) = fixture();
        prepare(&alpha, "one", "body").unwrap();
        let closed = alpha
            .resolve("one", &target(), "completed", "Reported review done")
            .unwrap();
        assert_eq!(closed["taskCompletionVerified"], false);
        assert_eq!(
            alpha
                .resolve("one", &target(), "completed", "Reported review done")
                .unwrap(),
            closed
        );
        assert_eq!(
            alpha
                .resolve("one", &target(), "retired", "Reported review done")
                .unwrap_err(),
            "WEB_WORK_REQUEST_CONFLICT"
        );
        assert_eq!(prepare(&alpha, "one", "body").unwrap(), "IDEMPOTENT_CLOSED");
        assert_eq!(prepare(&alpha, "two", "next").unwrap(), "OPEN_NEW");
        assert_eq!(alpha.record("one").unwrap().unwrap().phase, "CLOSED");
        assert_eq!(alpha.record("two").unwrap().unwrap().phase, "OPEN");
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn read_only_missing_status_and_malformed_or_redirected_record_fail_closed() {
        let (workspace, alpha, _) = fixture();
        assert_eq!(
            WorkLedger::status(&workspace, "alpha")["status"],
            "DURABLE_SNAPSHOT"
        );
        assert!(!alpha.path.exists());
        fs::write(&alpha.path, b"{}").unwrap();
        assert_eq!(
            prepare(&alpha, "one", "body").unwrap_err(),
            "WEB_WORK_LEDGER_INVALID"
        );
        assert_eq!(
            WorkLedger::status(&workspace, "alpha")["status"],
            "ATTENTION"
        );
        fs::remove_file(&alpha.path).unwrap();
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_file(workspace.join("outside"), &alpha.path).is_ok() {
                assert_eq!(
                    prepare(&alpha, "one", "body").unwrap_err(),
                    "WEB_WORK_LEDGER_UNSAFE"
                );
            }
        }
        fs::remove_dir_all(workspace).unwrap();
    }
}
