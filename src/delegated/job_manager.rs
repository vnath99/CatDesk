use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep};
use uuid::Uuid;

use crate::command::validate_shell_safety;

use super::EXECUTION_CONTRACT_SCHEMA_VERSION;

const DEFAULT_MAX_LOG_BYTES: u64 = 1024 * 1024;
const DEFAULT_POLL_BYTES: usize = 16 * 1024;
const MAX_POLL_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(String);

impl JobId {
    pub fn new(value: impl Into<String>) -> Result<Self, JobError> {
        let value = value.into();
        if value.trim().is_empty()
            || value.len() > 128
            || !value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err(JobError::Validation("invalid job id".into()));
        }
        Ok(Self(value))
    }

    pub fn fresh() -> Self {
        Self(format!("job-{}", Uuid::new_v4()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Lost,
}

impl JobStatus {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Lost
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSpecV1 {
    pub command: String,
    pub cwd: PathBuf,
    pub command_profile: String,
    pub max_log_bytes: u64,
}

impl JobSpecV1 {
    pub fn new(command: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            cwd: cwd.into(),
            command_profile: "default".into(),
            max_log_bytes: DEFAULT_MAX_LOG_BYTES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecordV1 {
    pub schema_version: u32,
    pub job_id: JobId,
    pub status: JobStatus,
    pub command: String,
    pub cwd: PathBuf,
    pub command_profile: String,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub started_at_ms: u128,
    pub finished_at_ms: Option<u128>,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
    pub max_log_bytes: u64,
    pub status_detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogPoll {
    pub text: String,
    pub offset: u64,
    pub next_offset: u64,
    pub truncated: bool,
    pub rotated: bool,
    pub local_path: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum JobError {
    Io(String),
    Serde(String),
    Validation(String),
    Policy(String),
    NotFound(String),
    Timeout(String),
}

impl From<std::io::Error> for JobError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for JobError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value.to_string())
    }
}

#[derive(Debug)]
pub struct JobManager {
    root: PathBuf,
}

impl JobManager {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, JobError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn list_jobs(&self) -> Result<Vec<JobRecordV1>, JobError> {
        let mut records = Vec::new();
        if !self.root.exists() {
            return Ok(records);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path().join("record.json");
            if path.exists() {
                records.push(read_json(&path)?);
            }
        }
        records.sort_by(|left: &JobRecordV1, right| left.job_id.cmp(&right.job_id));
        Ok(records)
    }

    pub fn get_job(&self, job_id: &JobId) -> Result<JobRecordV1, JobError> {
        read_json(&self.record_path(job_id)?)
    }

    pub async fn start_job(&self, spec: JobSpecV1) -> Result<JobRecordV1, JobError> {
        validate_spec(&spec)?;
        validate_shell_safety(&spec.command).map_err(JobError::Policy)?;

        let job_id = JobId::fresh();
        let job_dir = self.job_dir(&job_id)?;
        fs::create_dir_all(&job_dir)?;
        let stdout_log = job_dir.join("stdout.log");
        let stderr_log = job_dir.join("stderr.log");

        let mut command = platform_shell_command(&spec.command);
        command
            .current_dir(&spec.cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = command.spawn()?;
        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let record = JobRecordV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            job_id: job_id.clone(),
            status: JobStatus::Running,
            command: spec.command,
            cwd: spec.cwd,
            command_profile: spec.command_profile,
            pid,
            exit_code: None,
            started_at_ms: now_ms(),
            finished_at_ms: None,
            stdout_log: stdout_log.clone(),
            stderr_log: stderr_log.clone(),
            max_log_bytes: spec.max_log_bytes,
            status_detail: None,
        };
        self.write_record(&record)?;

        let stdout_handle =
            stdout.map(|reader| spawn_log_pump(reader, stdout_log.clone(), spec.max_log_bytes));
        let stderr_handle =
            stderr.map(|reader| spawn_log_pump(reader, stderr_log.clone(), spec.max_log_bytes));
        spawn_waiter(
            self.root.clone(),
            job_id,
            child,
            stdout_handle,
            stderr_handle,
        );

        Ok(record)
    }

    pub async fn wait_for_terminal(
        &self,
        job_id: &JobId,
        timeout_duration: Duration,
    ) -> Result<JobRecordV1, JobError> {
        let deadline = Instant::now() + timeout_duration;
        loop {
            let record = self.get_job(job_id)?;
            if record.status.is_terminal() {
                return Ok(record);
            }
            if Instant::now() >= deadline {
                return Err(JobError::Timeout(format!(
                    "job {} did not finish before timeout",
                    job_id.as_str()
                )));
            }
            sleep(Duration::from_millis(25)).await;
        }
    }

    pub async fn cancel_job(&self, job_id: &JobId) -> Result<JobRecordV1, JobError> {
        let mut record = self.get_job(job_id)?;
        if record.status.is_terminal() {
            return Ok(record);
        }
        if let Some(pid) = record.pid {
            kill_process_tree(pid).await?;
        }
        record.status = JobStatus::Cancelled;
        record.finished_at_ms = Some(now_ms());
        record.status_detail = Some("cancelled by supervisor".into());
        self.write_record(&record)?;
        Ok(record)
    }

    pub fn poll_log(
        &self,
        job_id: &JobId,
        stream: LogStream,
        offset: u64,
        max_bytes: usize,
    ) -> Result<LogPoll, JobError> {
        let record = self.get_job(job_id)?;
        let local_path = match stream {
            LogStream::Stdout => record.stdout_log,
            LogStream::Stderr => record.stderr_log,
        };
        poll_log_file(local_path, offset, max_bytes)
    }

    pub fn recover_running_jobs(&self) -> Result<Vec<JobRecordV1>, JobError> {
        let mut recovered = Vec::new();
        for mut record in self.list_jobs()? {
            if record.status != JobStatus::Running {
                continue;
            }
            let alive = record.pid.is_some_and(process_is_alive);
            if !alive {
                record.status = JobStatus::Lost;
                record.finished_at_ms = Some(now_ms());
                record.status_detail = Some("process missing during restart recovery".into());
                self.write_record(&record)?;
            }
            recovered.push(record);
        }
        Ok(recovered)
    }

    fn job_dir(&self, job_id: &JobId) -> Result<PathBuf, JobError> {
        JobId::new(job_id.as_str())?;
        Ok(self.root.join(job_id.as_str()))
    }

    fn record_path(&self, job_id: &JobId) -> Result<PathBuf, JobError> {
        Ok(self.job_dir(job_id)?.join("record.json"))
    }

    fn write_record(&self, record: &JobRecordV1) -> Result<(), JobError> {
        write_json_atomic(&self.record_path(&record.job_id)?, record)
    }
}

fn validate_spec(spec: &JobSpecV1) -> Result<(), JobError> {
    if spec.command.trim().is_empty() {
        return Err(JobError::Validation("command must not be empty".into()));
    }
    if spec.command_profile.trim().is_empty() {
        return Err(JobError::Validation(
            "command_profile must not be empty".into(),
        ));
    }
    let cwd = spec
        .cwd
        .canonicalize()
        .map_err(|e| JobError::Validation(format!("cwd must exist: {e}")))?;
    if !cwd.is_dir() {
        return Err(JobError::Validation("cwd must be a directory".into()));
    }
    if spec.max_log_bytes == 0 {
        return Err(JobError::Validation(
            "max_log_bytes must be greater than zero".into(),
        ));
    }
    Ok(())
}

fn spawn_waiter(
    root: PathBuf,
    job_id: JobId,
    mut child: tokio::process::Child,
    stdout_handle: Option<JoinHandle<()>>,
    stderr_handle: Option<JoinHandle<()>>,
) {
    tokio::spawn(async move {
        let status = child.wait().await;
        if let Some(handle) = stdout_handle {
            let _ = handle.await;
        }
        if let Some(handle) = stderr_handle {
            let _ = handle.await;
        }

        let manager = match JobManager::open(root) {
            Ok(manager) => manager,
            Err(_) => return,
        };
        let mut record = match manager.get_job(&job_id) {
            Ok(record) => record,
            Err(_) => return,
        };
        if record.status.is_terminal() {
            return;
        }

        match status {
            Ok(status) if status.success() => {
                record.status = JobStatus::Succeeded;
                record.exit_code = status.code();
                record.status_detail = Some("process exited successfully".into());
            }
            Ok(status) => {
                record.status = JobStatus::Failed;
                record.exit_code = status.code();
                record.status_detail = Some("process exited unsuccessfully".into());
            }
            Err(error) => {
                record.status = JobStatus::Lost;
                record.status_detail = Some(format!("failed to observe process exit: {error}"));
            }
        }
        record.finished_at_ms = Some(now_ms());
        let _ = manager.write_record(&record);
    });
}

fn spawn_log_pump<R>(mut reader: R, path: PathBuf, max_log_bytes: u64) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(_) => return,
        };
        let mut current_len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
        let mut buffer = [0u8; 512];

        loop {
            let read = match reader.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => read,
                Err(_) => break,
            };
            if write_rotating(
                &mut file,
                &path,
                &buffer[..read],
                max_log_bytes,
                &mut current_len,
            )
            .await
            .is_err()
            {
                break;
            }
        }
        let _ = file.flush().await;
    })
}

async fn write_rotating(
    file: &mut tokio::fs::File,
    path: &Path,
    mut chunk: &[u8],
    max_log_bytes: u64,
    current_len: &mut u64,
) -> Result<(), std::io::Error> {
    while !chunk.is_empty() {
        if *current_len >= max_log_bytes {
            *file = rotate_open(path).await?;
            *current_len = 0;
        }
        let remaining_capacity = max_log_bytes.saturating_sub(*current_len).max(1) as usize;
        let take = remaining_capacity.min(chunk.len());
        file.write_all(&chunk[..take]).await?;
        *current_len += take as u64;
        chunk = &chunk[take..];
    }
    file.flush().await
}

async fn rotate_open(path: &Path) -> Result<tokio::fs::File, std::io::Error> {
    let rotated = rotated_log_path(path);
    let _ = tokio::fs::remove_file(&rotated).await;
    if tokio::fs::metadata(path).await.is_ok() {
        tokio::fs::rename(path, &rotated).await?;
    }
    tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .await
}

fn poll_log_file(path: PathBuf, offset: u64, max_bytes: usize) -> Result<LogPoll, JobError> {
    let limit = if max_bytes == 0 {
        DEFAULT_POLL_BYTES
    } else {
        max_bytes.min(MAX_POLL_BYTES)
    };
    if !path.exists() {
        return Ok(LogPoll {
            text: String::new(),
            offset,
            next_offset: offset,
            truncated: false,
            rotated: false,
            local_path: path,
        });
    }
    let len = fs::metadata(&path)?.len();
    let rotated = offset > len;
    let effective_offset = if rotated { 0 } else { offset };
    let mut file = File::open(&path)?;
    file.seek(SeekFrom::Start(effective_offset))?;

    let mut limited = file.take(limit as u64);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes)?;
    let next_offset = effective_offset + bytes.len() as u64;
    let truncated = next_offset < len;

    Ok(LogPoll {
        text: String::from_utf8_lossy(&bytes).to_string(),
        offset: effective_offset,
        next_offset,
        truncated,
        rotated,
        local_path: path,
    })
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), JobError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = File::create(&tmp)?;
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, JobError> {
    let file = File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            JobError::NotFound(path.display().to_string())
        } else {
            JobError::Io(error.to_string())
        }
    })?;
    Ok(serde_json::from_reader(file)?)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn rotated_log_path(path: &Path) -> PathBuf {
    let mut extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    if extension.is_empty() {
        extension.push('1');
    } else {
        extension.push_str(".1");
    }
    path.with_extension(extension)
}

#[cfg(windows)]
fn platform_shell_command(command: &str) -> Command {
    let mut shell = Command::new("powershell.exe");
    shell
        .arg("-NoLogo")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(command);
    shell
}

#[cfg(not(windows))]
fn platform_shell_command(command: &str) -> Command {
    let mut shell = Command::new("/bin/bash");
    shell.arg("-c").arg(command);
    shell
}

#[cfg(windows)]
async fn kill_process_tree(pid: u32) -> Result<(), JobError> {
    let output = Command::new("taskkill")
        .arg("/PID")
        .arg(pid.to_string())
        .arg("/T")
        .arg("/F")
        .output()
        .await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(JobError::Io(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

#[cfg(not(windows))]
async fn kill_process_tree(pid: u32) -> Result<(), JobError> {
    let _ = Command::new("pkill")
        .arg("-TERM")
        .arg("-P")
        .arg(pid.to_string())
        .output()
        .await;
    let output = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .output()
        .await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(JobError::Io(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    let Ok(output) = std::process::Command::new("tasklist")
        .arg("/FI")
        .arg(format!("PID eq {pid}"))
        .arg("/FO")
        .arg("CSV")
        .arg("/NH")
        .output()
    else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
}

#[cfg(not(windows))]
fn process_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn successful_job_persists_record_and_logs() {
        let manager = temp_manager("success");
        let record = manager
            .start_job(JobSpecV1::new(
                success_command(),
                temp_workspace("success-cwd"),
            ))
            .await
            .expect("start job");
        let finished = manager
            .wait_for_terminal(&record.job_id, Duration::from_secs(5))
            .await
            .expect("wait");

        assert_eq!(finished.status, JobStatus::Succeeded);
        assert_eq!(finished.exit_code, Some(0));
        let reopened = JobManager::open(manager.root()).expect("reopen");
        assert_eq!(
            reopened.get_job(&record.job_id).expect("record").status,
            JobStatus::Succeeded
        );
        let log = reopened
            .poll_log(&record.job_id, LogStream::Stdout, 0, 100)
            .expect("poll");
        assert!(log.text.contains("catdesk-job-ok"));
    }

    #[tokio::test]
    async fn failed_job_records_exit_code() {
        let manager = temp_manager("failed");
        let record = manager
            .start_job(JobSpecV1::new(fail_command(), temp_workspace("failed-cwd")))
            .await
            .expect("start job");
        let finished = manager
            .wait_for_terminal(&record.job_id, Duration::from_secs(5))
            .await
            .expect("wait");

        assert_eq!(finished.status, JobStatus::Failed);
        assert_eq!(finished.exit_code, Some(7));
    }

    #[tokio::test]
    async fn cancelled_job_uses_process_tree_termination() {
        let manager = temp_manager("cancelled");
        let record = manager
            .start_job(JobSpecV1::new(
                sleep_command(),
                temp_workspace("cancelled-cwd"),
            ))
            .await
            .expect("start job");

        let cancelled = manager.cancel_job(&record.job_id).await.expect("cancel");
        assert_eq!(cancelled.status, JobStatus::Cancelled);
        let finished = manager
            .wait_for_terminal(&record.job_id, Duration::from_secs(5))
            .await
            .expect("wait");
        assert_eq!(finished.status, JobStatus::Cancelled);
    }

    #[test]
    fn lost_process_is_marked_during_restart_recovery() {
        let manager = temp_manager("lost");
        let job_id = JobId::new("job-lost").expect("job id");
        let job_dir = manager.job_dir(&job_id).expect("job dir");
        fs::create_dir_all(&job_dir).expect("job dir exists");
        let record = JobRecordV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            job_id: job_id.clone(),
            status: JobStatus::Running,
            command: "sleep forever".into(),
            cwd: temp_workspace("lost-cwd"),
            command_profile: "default".into(),
            pid: Some(999_999),
            exit_code: None,
            started_at_ms: now_ms(),
            finished_at_ms: None,
            stdout_log: job_dir.join("stdout.log"),
            stderr_log: job_dir.join("stderr.log"),
            max_log_bytes: DEFAULT_MAX_LOG_BYTES,
            status_detail: None,
        };
        manager.write_record(&record).expect("write record");

        let recovered = manager.recover_running_jobs().expect("recover");

        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].status, JobStatus::Lost);
        assert_eq!(
            manager.get_job(&job_id).expect("record").status,
            JobStatus::Lost
        );
    }

    #[tokio::test]
    async fn restart_reopens_durable_records() {
        let manager = temp_manager("restart");
        let root = manager.root().to_path_buf();
        let record = manager
            .start_job(JobSpecV1::new(
                success_command(),
                temp_workspace("restart-cwd"),
            ))
            .await
            .expect("start job");
        let _ = manager
            .wait_for_terminal(&record.job_id, Duration::from_secs(5))
            .await
            .expect("wait");

        let restarted = JobManager::open(root).expect("restart manager");
        let jobs = restarted.list_jobs().expect("jobs");

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].job_id, record.job_id);
        assert_eq!(jobs[0].status, JobStatus::Succeeded);
    }

    #[tokio::test]
    async fn output_polling_is_bounded_and_logs_rotate() {
        let manager = temp_manager("output-limit");
        let mut spec = JobSpecV1::new(output_limit_command(), temp_workspace("output-limit-cwd"));
        spec.max_log_bytes = 1024;
        let record = manager.start_job(spec).await.expect("start job");
        let finished = manager
            .wait_for_terminal(&record.job_id, Duration::from_secs(5))
            .await
            .expect("wait");
        assert_eq!(finished.status, JobStatus::Succeeded);

        let first = manager
            .poll_log(&record.job_id, LogStream::Stdout, 0, 128)
            .expect("poll");
        assert!(first.text.len() <= 128);
        assert!(first.truncated);
        assert!(rotated_log_path(&finished.stdout_log).exists());
        assert!(fs::metadata(&finished.stdout_log).expect("metadata").len() <= 1024);
    }

    #[tokio::test]
    async fn command_policy_blocks_dangerous_long_jobs() {
        let manager = temp_manager("policy");
        let error = manager
            .start_job(JobSpecV1::new(
                "rm -rf notes.txt",
                temp_workspace("policy-cwd"),
            ))
            .await
            .expect_err("policy should reject");

        assert!(matches!(error, JobError::Policy(_)));
        assert!(manager.list_jobs().expect("jobs").is_empty());
    }

    fn temp_manager(name: &str) -> JobManager {
        JobManager::open(temp_root(name)).expect("manager")
    }

    fn temp_root(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "catdesk-job-manager-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn temp_workspace(name: &str) -> PathBuf {
        let path = temp_root(name).join("workspace");
        fs::create_dir_all(&path).expect("workspace");
        path
    }

    #[cfg(windows)]
    fn success_command() -> String {
        "Write-Output catdesk-job-ok".into()
    }

    #[cfg(not(windows))]
    fn success_command() -> String {
        "printf 'catdesk-job-ok\\n'".into()
    }

    #[cfg(windows)]
    fn fail_command() -> String {
        "exit 7".into()
    }

    #[cfg(not(windows))]
    fn fail_command() -> String {
        "exit 7".into()
    }

    #[cfg(windows)]
    fn sleep_command() -> String {
        "Start-Sleep -Seconds 30".into()
    }

    #[cfg(not(windows))]
    fn sleep_command() -> String {
        "sleep 30".into()
    }

    #[cfg(windows)]
    fn output_limit_command() -> String {
        "1..80 | ForEach-Object { Write-Output ((\"line\" + $_) + (\"x\" * 80)) }".into()
    }

    #[cfg(not(windows))]
    fn output_limit_command() -> String {
        "for i in $(seq 1 80); do printf 'line%sxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\\n' \"$i\"; done".into()
    }
}
