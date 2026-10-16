//! Generic durable registry and scheduler guard for independent projects.
//!
//! It is deliberately a control-plane module: project workers continue to
//! own their project-local state stores. The registry only coordinates
//! workspace ownership, provider limits, and project/thread identity.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::codex_model_policy::required_effort;
use super::runtime::RuntimeError;

pub const PROJECT_REGISTRY_SCHEMA_VERSION: u32 = 1;
const MAX_PROJECT_REGISTRY_BYTES: u64 = 1024 * 1024;
const MAX_SCHEDULER_LEASE_BYTES: u64 = 128 * 1024;
const MAX_SCHEDULER_CURSOR_BYTES: u64 = 4 * 1024;
const MAX_SCHEDULER_LEASES: usize = 128;

fn redirected_entry(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Check each ancestor before creating a missing directory. `create_dir_all`
/// would otherwise follow a redirected parent before the final root is read.
fn checked_registry_directory(path: &Path, create: bool) -> Result<PathBuf, RuntimeError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        checked_registry_directory(parent, create)?;
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io_error(error)),
            }
            fs::symlink_metadata(path).map_err(io_error)?
        }
        Err(error) => return Err(io_error(error)),
    };
    if !metadata.is_dir() || redirected_entry(&metadata) {
        return Err(RuntimeError::Validation(
            "project registry directory is redirected or unsafe".into(),
        ));
    }
    path.canonicalize().map_err(io_error)
}

fn nofollow_read_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    options
}

fn checked_registry_file(
    path: &Path,
    maximum_length: u64,
    diagnostic: &'static str,
) -> Result<Option<File>, RuntimeError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    if !metadata.is_file() || redirected_entry(&metadata) || metadata.len() > maximum_length {
        return Err(RuntimeError::Validation(diagnostic.into()));
    }
    let file = nofollow_read_options().open(path).map_err(io_error)?;
    let opened = file.metadata().map_err(io_error)?;
    if !opened.is_file() || redirected_entry(&opened) || opened.len() > maximum_length {
        return Err(RuntimeError::Validation(diagnostic.into()));
    }
    Ok(Some(file))
}

fn read_bounded_registry_bytes(
    file: File,
    maximum_length: u64,
    diagnostic: &'static str,
) -> Result<Vec<u8>, RuntimeError> {
    let mut bytes = Vec::new();
    file.take(maximum_length + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > maximum_length {
        return Err(RuntimeError::Validation(diagnostic.into()));
    }
    Ok(bytes)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousProjectV1 {
    pub project_id: String,
    pub workspace: PathBuf,
    pub git_identity: String,
    pub verification_profile: String,
    #[serde(default)]
    pub codex_thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatgpt_target_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatgpt_target_sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousProjectRegistryV1 {
    pub schema_version: u32,
    pub maximum_codex_workers: u32,
    pub maximum_qwen_workers: u32,
    #[serde(default)]
    pub projects: Vec<AutonomousProjectV1>,
}

/// Non-secret, host-observed facts which bind a registration confirmation to
/// one exact external repository and one exact current-user Codex thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRegistrationEvidenceV1 {
    pub workspace: PathBuf,
    pub git_identity: String,
    pub codex_thread_id: String,
    pub selected_model: String,
    pub reasoning_effort: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRegistrationPreflightV1 {
    pub schema_version: u32,
    pub confirmation_token: String,
    pub confirmation_fingerprint: String,
    pub project_id: String,
    pub verification_profile: String,
    pub evidence: ProjectRegistrationEvidenceV1,
    pub expires_at_unix: u64,
    #[serde(default)]
    pub confirmed: bool,
}

pub const PROJECT_REGISTRATION_PREFLIGHT_SCHEMA_VERSION: u32 = 1;
pub const PROJECT_REGISTRATION_CONFIRMATION_TTL_SECONDS: u64 = 120;
pub const CATDESK_PROJECT_ID_V1: &str = "catdesk";

/// Metadata-only candidate retained between the operator's selection and a
/// confirmation. The thread id is never an MCP input: it is host-observed
/// evidence retained solely so confirmation can re-read that same thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectThreadAdoptionCandidateV1 {
    pub candidate_handle: String,
    pub title: Option<String>,
    pub thread_fingerprint: String,
    pub cwd_fingerprint: String,
    pub evidence: ProjectRegistrationEvidenceV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectThreadAdoptionPreflightV1 {
    pub schema_version: u32,
    pub confirmation_token: String,
    pub project_id: String,
    pub workspace: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_previous_thread_id: Option<String>,
    pub candidates: Vec<ProjectThreadAdoptionCandidateV1>,
    pub expires_at_unix: u64,
    #[serde(default)]
    pub confirmed_candidate_handle: Option<String>,
}

pub const PROJECT_THREAD_ADOPTION_PREFLIGHT_SCHEMA_VERSION: u32 = 1;
pub const PROJECT_THREAD_ADOPTION_CONFIRMATION_TTL_SECONDS: u64 = 120;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ScheduledProviderV1 {
    Codex,
    Qwen,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousWorkspaceLeaseV1 {
    pub project_id: String,
    pub workspace: PathBuf,
    pub owner_id: String,
    pub provider: ScheduledProviderV1,
    pub expires_at_unix: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalScheduleDecisionV1 {
    Granted(AutonomousWorkspaceLeaseV1),
    Rejected { reason: &'static str },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoundRobinCursorV1 {
    schema_version: u32,
    last_granted_project_id: String,
}

#[derive(Clone, Debug)]
pub struct AutonomousProjectRegistryStoreV1 {
    root: PathBuf,
}

/// Read-only identity resolved from the CatDesk control registry. This is not
/// a worker lease or permission to mutate either workspace; execution must
/// recheck the binding when it acquires its own task authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedProjectScopeV1 {
    pub control_workspace: PathBuf,
    pub project: AutonomousProjectV1,
}

/// Resolve an execution root from the already registered CatDesk project ID.
/// The caller supplies the trusted CatDesk control workspace, never an
/// arbitrary candidate execution path or a replacement Git identity. The
/// registry is opened read-only and no state is initialized in external roots.
pub fn resolve_registered_project_from_control_root(
    control_workspace: &Path,
    project_id: &str,
) -> Result<ResolvedProjectScopeV1, RuntimeError> {
    valid_slug(project_id, "project id")?;
    let control_workspace = control_workspace
        .canonicalize()
        .map_err(|_| RuntimeError::Validation("CatDesk control workspace is unavailable".into()))?;
    let store = AutonomousProjectRegistryStoreV1::open_read_only(
        control_workspace.join(".catdesk").join("projects"),
    )?;
    let registry = store.load_registry()?;
    let controller = registry
        .projects
        .iter()
        .find(|project| project.project_id == CATDESK_PROJECT_ID_V1)
        .ok_or_else(|| {
            RuntimeError::Validation("CatDesk control project is not registered".into())
        })?;
    if controller.workspace != control_workspace {
        return Err(RuntimeError::Validation(
            "CatDesk control registry is bound to a different workspace".into(),
        ));
    }
    let control_path = controller
        .workspace
        .to_str()
        .ok_or_else(|| RuntimeError::Validation("CatDesk control workspace is not UTF-8".into()))?;
    let observed_control = super::autonomy_supervisor::verified_external_git_workspace(
        control_path,
        &controller.git_identity,
    )
    .map_err(|_| RuntimeError::Validation("CatDesk control Git identity is unavailable".into()))?;
    if observed_control != control_workspace {
        return Err(RuntimeError::Validation(
            "CatDesk control workspace identity drifted".into(),
        ));
    }
    let project = registry
        .projects
        .into_iter()
        .find(|project| project.project_id == project_id)
        .ok_or_else(|| RuntimeError::Validation("project is not registered".into()))?;
    let registered_path = project.workspace.to_str().ok_or_else(|| {
        RuntimeError::Validation("registered project workspace is not UTF-8".into())
    })?;
    let observed_workspace = super::autonomy_supervisor::verified_external_git_workspace(
        registered_path,
        &project.git_identity,
    )
    .map_err(|_| {
        RuntimeError::Validation("registered project Git identity is unavailable".into())
    })?;
    if observed_workspace != project.workspace {
        return Err(RuntimeError::Validation(
            "registered project workspace identity drifted".into(),
        ));
    }
    Ok(ResolvedProjectScopeV1 {
        control_workspace,
        project,
    })
}

impl AutonomousProjectRegistryStoreV1 {
    /// Opens an existing registry root without creating any children.  Views
    /// such as the native operator GUI use this form so a status/readback
    /// refresh cannot initialize project authority as a side effect.
    pub fn open_read_only(root: impl AsRef<Path>) -> Result<Self, RuntimeError> {
        let root = checked_registry_directory(root.as_ref(), false)?;
        Ok(Self { root })
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self, RuntimeError> {
        let root = checked_registry_directory(root.as_ref(), true)?;
        for child in ["leases", "registration-confirmations", "thread-adoptions"] {
            checked_registry_directory(&root.join(child), true)?;
        }
        Ok(Self { root })
    }

    pub fn initialize(
        &self,
        maximum_codex_workers: u32,
        maximum_qwen_workers: u32,
    ) -> Result<(), RuntimeError> {
        if maximum_codex_workers == 0 || maximum_qwen_workers == 0 {
            return Err(RuntimeError::Validation(
                "global provider concurrency must be non-zero".into(),
            ));
        }
        let path = self.registry_path();
        match fs::symlink_metadata(&path) {
            Ok(_) => return self.load_registry().map(|_| ()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
        self.save_registry(&AutonomousProjectRegistryV1 {
            schema_version: PROJECT_REGISTRY_SCHEMA_VERSION,
            maximum_codex_workers,
            maximum_qwen_workers,
            projects: Vec::new(),
        })
    }

    pub fn load_registry(&self) -> Result<AutonomousProjectRegistryV1, RuntimeError> {
        let file = checked_registry_file(
            &self.registry_path(),
            MAX_PROJECT_REGISTRY_BYTES,
            "project registry file is redirected or oversized",
        )?
        .ok_or_else(|| RuntimeError::Validation("project registry is unavailable".into()))?;
        let bytes = read_bounded_registry_bytes(
            file,
            MAX_PROJECT_REGISTRY_BYTES,
            "project registry file is oversized",
        )?;
        let registry: AutonomousProjectRegistryV1 =
            serde_json::from_slice(&bytes).map_err(|error| {
                RuntimeError::Validation(format!("project registry is corrupted: {error}"))
            })?;
        validate_registry(&registry)?;
        Ok(registry)
    }

    /// Repair exactly one invalid CatDesk target digest when the canonical URL
    /// already agrees with independent Wake authority. This is intentionally
    /// unavailable for other projects, arbitrary target URLs, or other registry
    /// corruption. The registration lock and normal validated/atomic writer
    /// protect all other project records.
    pub(crate) fn reconcile_catdesk_digest_with_wake(
        &self,
        workspace: &Path,
        wake_url: &str,
        expected_wake_sha256: &str,
    ) -> Result<(), RuntimeError> {
        let canonical = canonical_project_chat_target(wake_url)?;
        if canonical != wake_url
            || !valid_sha256(expected_wake_sha256)
            || project_chat_target_digest(wake_url) != expected_wake_sha256
        {
            return Err(RuntimeError::Validation(
                "designated target reconciliation wake identity mismatch".into(),
            ));
        }
        let workspace = workspace.canonicalize().map_err(|_| {
            RuntimeError::Validation("designated target workspace unavailable".into())
        })?;
        let _lock = self.acquire_registration_lock()?;
        let file = OpenOptions::new()
            .read(true)
            .open(self.registry_path())
            .map_err(io_error)?;
        if file.metadata().map_err(io_error)?.len() > 1024 * 1024 {
            return Err(RuntimeError::Validation(
                "designated target registry is oversized".into(),
            ));
        }
        let mut registry: AutonomousProjectRegistryV1 =
            serde_json::from_reader(file).map_err(|_| {
                RuntimeError::Validation("designated target registry is malformed".into())
            })?;
        let matching: Vec<usize> = registry
            .projects
            .iter()
            .enumerate()
            .filter(|(_, project)| {
                project.project_id == CATDESK_PROJECT_ID_V1 && project.workspace == workspace
            })
            .map(|(index, _)| index)
            .collect();
        if matching.len() != 1 {
            return Err(RuntimeError::Validation(
                "designated target project identity mismatch".into(),
            ));
        }
        let project = &mut registry.projects[matching[0]];
        if project.chatgpt_target_url.as_deref() != Some(wake_url)
            || project.chatgpt_target_sha256.as_deref() == Some(expected_wake_sha256)
            || !project
                .chatgpt_target_sha256
                .as_deref()
                .is_some_and(valid_sha256)
        {
            return Err(RuntimeError::Validation(
                "designated target reconciliation preconditions absent".into(),
            ));
        }
        project.chatgpt_target_sha256 = Some(expected_wake_sha256.to_owned());
        // The regular validator checks every field of every project, including
        // duplicates. Only the single CatDesk digest discrepancy may be healed.
        validate_registry(&registry)?;
        self.save_registry(&registry)
    }

    pub fn register_project(&self, mut project: AutonomousProjectV1) -> Result<(), RuntimeError> {
        project.workspace = project.workspace.canonicalize().map_err(|_| {
            RuntimeError::Validation("registered project workspace canonicalization failed".into())
        })?;
        validate_project(&project)?;
        let mut registry = self.load_registry()?;
        if registry
            .projects
            .iter()
            .any(|existing| existing.project_id == project.project_id)
        {
            return Err(RuntimeError::Validation(
                "project id is already registered".into(),
            ));
        }
        if registry
            .projects
            .iter()
            .any(|existing| existing.workspace == project.workspace)
        {
            return Err(RuntimeError::Validation(
                "workspace is already registered".into(),
            ));
        }
        registry.projects.push(project);
        registry
            .projects
            .sort_by(|a, b| a.project_id.cmp(&b.project_id));
        self.save_registry(&registry)
    }

    pub fn bind_codex_thread(&self, project_id: &str, thread_id: &str) -> Result<(), RuntimeError> {
        valid_slug(project_id, "project id")?;
        valid_slug(thread_id, "Codex thread id")?;
        let mut registry = self.load_registry()?;
        if registry.projects.iter().any(|project| {
            project.project_id != project_id
                && project.codex_thread_id.as_deref() == Some(thread_id)
        }) {
            return Err(RuntimeError::Validation(
                "Codex thread is already bound to another project".into(),
            ));
        }
        let project = registry
            .projects
            .iter_mut()
            .find(|project| project.project_id == project_id)
            .ok_or_else(|| RuntimeError::Validation("project is not registered".into()))?;
        if project
            .codex_thread_id
            .as_deref()
            .is_some_and(|current| current != thread_id)
        {
            return Err(RuntimeError::Validation(
                "project already has a different Codex thread binding".into(),
            ));
        }
        project.codex_thread_id = Some(thread_id.into());
        self.save_registry(&registry)
    }

    /// Test-only entry to exercise the compare-and-swap failure cases without
    /// creating an operator path that bypasses quiescence confirmation.
    #[cfg(test)]
    fn rotate_codex_thread(
        &self,
        project_id: &str,
        expected_old_thread_id: &str,
        evidence: &ProjectRegistrationEvidenceV1,
    ) -> Result<AutonomousProjectV1, RuntimeError> {
        self.rotate_codex_thread_after(project_id, expected_old_thread_id, evidence, || Ok(()))
    }

    /// Only the confirmed rotation path invokes this in production; its guard
    /// runs under the registry lock immediately before the durable write.
    fn rotate_codex_thread_after<F>(
        &self,
        project_id: &str,
        expected_old_thread_id: &str,
        evidence: &ProjectRegistrationEvidenceV1,
        before_commit: F,
    ) -> Result<AutonomousProjectV1, RuntimeError>
    where
        F: FnOnce() -> Result<(), RuntimeError>,
    {
        valid_slug(project_id, "project id")?;
        valid_slug(expected_old_thread_id, "Codex thread id")?;
        validate_registration_evidence(evidence)?;
        if evidence.selected_model != super::codex_model_policy::SOL_MEDIUM_MODEL
            || evidence.codex_thread_id == expected_old_thread_id
        {
            return Err(RuntimeError::Validation(
                "thread rotation requires a distinct exact Sol/medium thread".into(),
            ));
        }
        let workspace = evidence.workspace.canonicalize().map_err(|_| {
            RuntimeError::Validation("thread rotation workspace canonicalization failed".into())
        })?;
        let _transition_lock = self.acquire_thread_transition_lock()?;
        let _lock = self.acquire_registration_lock()?;
        let mut registry = self.load_registry()?;
        if registry.projects.iter().any(|project| {
            project.project_id != project_id
                && project.codex_thread_id.as_deref() == Some(&evidence.codex_thread_id)
        }) {
            return Err(RuntimeError::Validation(
                "rotation thread is already bound to another project".into(),
            ));
        }
        let project = registry
            .projects
            .iter_mut()
            .find(|project| project.project_id == project_id)
            .ok_or_else(|| RuntimeError::Validation("project is not registered".into()))?;
        if project.codex_thread_id.as_deref() != Some(expected_old_thread_id)
            || project.workspace != workspace
            || project.git_identity != evidence.git_identity
        {
            return Err(RuntimeError::Validation(
                "thread rotation compare-and-swap or project identity did not match".into(),
            ));
        }
        before_commit()?;
        project.codex_thread_id = Some(evidence.codex_thread_id.clone());
        let rotated = project.clone();
        self.save_registry(&registry)?;
        Ok(rotated)
    }

    /// Resolves one execution identity only when both the durable project id
    /// and the caller's already-canonical workspace agree.  A project id is
    /// never a workspace selector: callers must not use a sibling project's
    /// thread, target, lease, or accounting identity merely by naming it.
    pub fn project_for_workspace(
        &self,
        project_id: &str,
        workspace: &Path,
    ) -> Result<AutonomousProjectV1, RuntimeError> {
        valid_slug(project_id, "project id")?;
        let workspace = workspace.canonicalize().map_err(|_| {
            RuntimeError::Validation("project workspace canonicalization failed".into())
        })?;
        let registry = self.load_registry()?;
        let project = registry
            .projects
            .iter()
            .find(|project| project.project_id == project_id)
            .ok_or_else(|| RuntimeError::Validation("project is not registered".into()))?;
        if project.workspace != workspace {
            return Err(RuntimeError::Validation(
                "project is registered to a different workspace".into(),
            ));
        }
        Ok(project.clone())
    }

    /// Stores a short-lived, opaque confirmation record without mutating the
    /// project registry.  The caller supplies only host-verified evidence;
    /// callers never choose a thread as an authority at confirmation time.
    pub fn preflight_project_registration(
        &self,
        project_id: &str,
        verification_profile: &str,
        mut evidence: ProjectRegistrationEvidenceV1,
        now_unix: u64,
    ) -> Result<ProjectRegistrationPreflightV1, RuntimeError> {
        valid_slug(project_id, "project id")?;
        validate_registration_profile(verification_profile)?;
        validate_registration_evidence(&evidence)?;
        evidence.workspace = evidence.workspace.canonicalize().map_err(|_| {
            RuntimeError::Validation("registration workspace canonicalization failed".into())
        })?;
        let registry = self.load_registry()?;
        reject_registration_conflicts(&registry, project_id, verification_profile, &evidence)?;
        let confirmation_token = format!("prjreg-{}", Uuid::new_v4().simple());
        let fingerprint = registration_fingerprint(project_id, verification_profile, &evidence);
        let preflight = ProjectRegistrationPreflightV1 {
            schema_version: PROJECT_REGISTRATION_PREFLIGHT_SCHEMA_VERSION,
            confirmation_token: confirmation_token.clone(),
            confirmation_fingerprint: fingerprint,
            project_id: project_id.into(),
            verification_profile: verification_profile.into(),
            evidence,
            expires_at_unix: now_unix
                .checked_add(PROJECT_REGISTRATION_CONFIRMATION_TTL_SECONDS)
                .ok_or_else(|| {
                    RuntimeError::Validation("registration confirmation expiry overflow".into())
                })?,
            confirmed: false,
        };
        self.write_preflight(&preflight)?;
        Ok(preflight)
    }

    /// Revalidates all host-observed evidence and atomically writes a single
    /// project record containing its canonical thread. An exact replay after
    /// success converges; every drift/conflict leaves the registry unchanged.
    pub fn confirm_project_registration(
        &self,
        confirmation_token: &str,
        evidence: &ProjectRegistrationEvidenceV1,
        now_unix: u64,
    ) -> Result<AutonomousProjectV1, RuntimeError> {
        valid_confirmation_token(confirmation_token)?;
        validate_registration_evidence(evidence)?;
        let _lock = self.acquire_registration_lock()?;
        let mut preflight = self.load_preflight(confirmation_token)?;
        validate_preflight(&preflight)?;
        let canonical = evidence.workspace.canonicalize().map_err(|_| {
            RuntimeError::Validation("registration workspace canonicalization failed".into())
        })?;
        if canonical != preflight.evidence.workspace
            || evidence.git_identity != preflight.evidence.git_identity
            || evidence.codex_thread_id != preflight.evidence.codex_thread_id
            || evidence.selected_model != preflight.evidence.selected_model
            || evidence.reasoning_effort != preflight.evidence.reasoning_effort
        {
            return Err(RuntimeError::Validation(
                "registration confirmation evidence drifted from preflight".into(),
            ));
        }
        let expected_fingerprint = registration_fingerprint(
            &preflight.project_id,
            &preflight.verification_profile,
            &preflight.evidence,
        );
        if preflight.confirmation_fingerprint != expected_fingerprint {
            return Err(RuntimeError::Validation(
                "registration confirmation fingerprint is invalid".into(),
            ));
        }
        let mut registry = self.load_registry()?;
        if preflight.confirmed {
            return exact_registered_binding(&registry, &preflight);
        }
        if now_unix > preflight.expires_at_unix {
            return Err(RuntimeError::Validation(
                "registration confirmation expired".into(),
            ));
        }
        reject_registration_conflicts(
            &registry,
            &preflight.project_id,
            &preflight.verification_profile,
            &preflight.evidence,
        )?;
        let project = AutonomousProjectV1 {
            project_id: preflight.project_id.clone(),
            workspace: preflight.evidence.workspace.clone(),
            git_identity: preflight.evidence.git_identity.clone(),
            verification_profile: preflight.verification_profile.clone(),
            codex_thread_id: Some(preflight.evidence.codex_thread_id.clone()),
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        };
        registry.projects.push(project.clone());
        registry
            .projects
            .sort_by(|a, b| a.project_id.cmp(&b.project_id));
        self.save_registry(&registry)?;
        preflight.confirmed = true;
        self.write_preflight(&preflight)?;
        Ok(project)
    }

    pub fn registration_preflight(
        &self,
        confirmation_token: &str,
    ) -> Result<ProjectRegistrationPreflightV1, RuntimeError> {
        valid_confirmation_token(confirmation_token)?;
        self.load_preflight(confirmation_token)
    }

    /// Creates an operator-selectable, short-lived metadata-only candidate
    /// list. Callers receive opaque handles, never a thread id they can claim
    /// as authority. Existing project bindings are intentionally immutable.
    pub fn preflight_thread_adoption(
        &self,
        project_id: &str,
        workspace: &Path,
        candidates: Vec<(Option<String>, ProjectRegistrationEvidenceV1)>,
        now_unix: u64,
    ) -> Result<ProjectThreadAdoptionPreflightV1, RuntimeError> {
        self.preflight_thread_selection(project_id, workspace, candidates, now_unix, None)
    }

    pub(crate) fn preflight_thread_rotation(
        &self,
        project_id: &str,
        workspace: &Path,
        candidates: Vec<(Option<String>, ProjectRegistrationEvidenceV1)>,
        now_unix: u64,
    ) -> Result<ProjectThreadAdoptionPreflightV1, RuntimeError> {
        let registry = self.load_registry()?;
        let current = registry
            .projects
            .iter()
            .find(|project| project.project_id == project_id)
            .and_then(|project| project.codex_thread_id.as_deref())
            .ok_or_else(|| {
                RuntimeError::Validation("rotation requires an existing project thread".into())
            })?;
        self.preflight_thread_selection(project_id, workspace, candidates, now_unix, Some(current))
    }

    fn preflight_thread_selection(
        &self,
        project_id: &str,
        workspace: &Path,
        mut candidates: Vec<(Option<String>, ProjectRegistrationEvidenceV1)>,
        now_unix: u64,
        expected_previous_thread_id: Option<&str>,
    ) -> Result<ProjectThreadAdoptionPreflightV1, RuntimeError> {
        valid_slug(project_id, "project id")?;
        if candidates.is_empty() || candidates.len() > 8 {
            return Err(RuntimeError::Validation(
                "thread adoption candidates are invalid".into(),
            ));
        }
        let workspace = workspace.canonicalize().map_err(|_| {
            RuntimeError::Validation("thread adoption workspace canonicalization failed".into())
        })?;
        let registry = self.load_registry()?;
        let project = registry
            .projects
            .iter()
            .find(|project| project.project_id == project_id)
            .ok_or_else(|| RuntimeError::Validation("project is not registered".into()))?;
        if project.workspace != workspace
            || project.codex_thread_id.as_deref() != expected_previous_thread_id
        {
            return Err(RuntimeError::Validation(
                "project thread selection no longer matches its canonical binding".into(),
            ));
        }
        let mut prepared = Vec::with_capacity(candidates.len());
        for (title, mut evidence) in candidates.drain(..) {
            validate_registration_evidence(&evidence)?;
            evidence.workspace = evidence.workspace.canonicalize().map_err(|_| {
                RuntimeError::Validation("thread adoption evidence workspace is invalid".into())
            })?;
            if evidence.workspace != workspace || evidence.git_identity != project.git_identity {
                return Err(RuntimeError::Validation(
                    "thread adoption evidence drifted from project".into(),
                ));
            }
            if expected_previous_thread_id.is_some()
                && (evidence.selected_model != super::codex_model_policy::SOL_MEDIUM_MODEL
                    || Some(evidence.codex_thread_id.as_str()) == expected_previous_thread_id)
            {
                return Err(RuntimeError::Validation(
                    "rotation candidate is not a distinct Sol/medium thread".into(),
                ));
            }
            if registry
                .projects
                .iter()
                .any(|other| other.codex_thread_id.as_deref() == Some(&evidence.codex_thread_id))
            {
                return Err(RuntimeError::Validation(
                    "Codex thread is already registered".into(),
                ));
            }
            let title = title.filter(|value| {
                !value.is_empty() && value.len() <= 512 && !contains_secret_marker(value)
            });
            prepared.push(ProjectThreadAdoptionCandidateV1 {
                candidate_handle: format!("candidate-{}", Uuid::new_v4().simple()),
                title,
                thread_fingerprint: adoption_fingerprint("thread", &evidence.codex_thread_id),
                cwd_fingerprint: adoption_fingerprint("cwd", &workspace.to_string_lossy()),
                evidence,
            });
        }
        let preflight = ProjectThreadAdoptionPreflightV1 {
            schema_version: PROJECT_THREAD_ADOPTION_PREFLIGHT_SCHEMA_VERSION,
            confirmation_token: format!("adopt-{}", Uuid::new_v4().simple()),
            project_id: project_id.into(),
            workspace,
            expected_previous_thread_id: expected_previous_thread_id.map(str::to_string),
            candidates: prepared,
            expires_at_unix: now_unix
                .checked_add(PROJECT_THREAD_ADOPTION_CONFIRMATION_TTL_SECONDS)
                .ok_or_else(|| {
                    RuntimeError::Validation("thread adoption expiry overflow".into())
                })?,
            confirmed_candidate_handle: None,
        };
        self.write_adoption_preflight(&preflight)?;
        Ok(preflight)
    }

    /// Revalidates host-observed metadata for exactly the candidate chosen by
    /// its opaque handle, then atomically binds it or leaves the registry as-is.
    pub fn confirm_thread_adoption(
        &self,
        confirmation_token: &str,
        candidate_handle: &str,
        evidence: &ProjectRegistrationEvidenceV1,
        now_unix: u64,
    ) -> Result<AutonomousProjectV1, RuntimeError> {
        valid_adoption_token(confirmation_token)?;
        valid_candidate_handle(candidate_handle)?;
        validate_registration_evidence(evidence)?;
        let _lock = self.acquire_registration_lock()?;
        let mut preflight = self.load_thread_adoption_preflight(confirmation_token)?;
        validate_adoption_preflight(&preflight)?;
        if preflight.expected_previous_thread_id.is_some() {
            return Err(RuntimeError::Validation(
                "thread rotation cannot use adoption confirmation".into(),
            ));
        }
        let candidate = preflight
            .candidates
            .iter()
            .find(|candidate| candidate.candidate_handle == candidate_handle)
            .ok_or_else(|| {
                RuntimeError::Validation("thread adoption candidate is unknown".into())
            })?;
        if candidate.evidence != *evidence
            || evidence.workspace.canonicalize().ok().as_deref()
                != Some(preflight.workspace.as_path())
        {
            return Err(RuntimeError::Validation(
                "thread adoption evidence drifted from preflight".into(),
            ));
        }
        let mut registry = self.load_registry()?;
        let thread_conflict = registry.projects.iter().any(|other| {
            other.project_id != preflight.project_id
                && other.codex_thread_id.as_deref() == Some(&evidence.codex_thread_id)
        });
        let project = registry
            .projects
            .iter_mut()
            .find(|project| project.project_id == preflight.project_id)
            .ok_or_else(|| RuntimeError::Validation("project is not registered".into()))?;
        if preflight.confirmed_candidate_handle.as_deref() == Some(candidate_handle)
            && project.codex_thread_id.as_deref() == Some(&evidence.codex_thread_id)
        {
            return Ok(project.clone());
        }
        if preflight.confirmed_candidate_handle.is_some()
            || now_unix > preflight.expires_at_unix
            || project.workspace != preflight.workspace
            || project.git_identity != evidence.git_identity
            || project.codex_thread_id.is_some()
            || thread_conflict
        {
            return Err(RuntimeError::Validation(
                "thread adoption confirmation was rejected".into(),
            ));
        }
        project.codex_thread_id = Some(evidence.codex_thread_id.clone());
        let bound = project.clone();
        self.save_registry(&registry)?;
        preflight.confirmed_candidate_handle = Some(candidate_handle.into());
        self.write_adoption_preflight(&preflight)?;
        Ok(bound)
    }

    /// Confirms one host-selected Sol/medium candidate against a short-lived
    /// preflight and the exact previous binding. The caller's quiescence
    /// check runs under the registry lock immediately before the CAS write.
    pub(crate) fn confirm_thread_rotation<F>(
        &self,
        confirmation_token: &str,
        candidate_handle: &str,
        evidence: &ProjectRegistrationEvidenceV1,
        now_unix: u64,
        before_commit: F,
    ) -> Result<AutonomousProjectV1, RuntimeError>
    where
        F: FnOnce() -> Result<(), RuntimeError>,
    {
        valid_adoption_token(confirmation_token)?;
        valid_candidate_handle(candidate_handle)?;
        validate_registration_evidence(evidence)?;
        let mut preflight = self.load_thread_adoption_preflight(confirmation_token)?;
        let old = preflight
            .expected_previous_thread_id
            .as_deref()
            .ok_or_else(|| {
                RuntimeError::Validation("adoption preflight cannot rotate a thread".into())
            })?;
        let candidate = preflight
            .candidates
            .iter()
            .find(|candidate| candidate.candidate_handle == candidate_handle)
            .ok_or_else(|| RuntimeError::Validation("rotation candidate is unknown".into()))?;
        if candidate.evidence != *evidence
            || evidence.workspace.canonicalize().ok().as_deref()
                != Some(preflight.workspace.as_path())
        {
            return Err(RuntimeError::Validation(
                "rotation evidence drifted from preflight".into(),
            ));
        }
        if preflight.confirmed_candidate_handle.as_deref() == Some(candidate_handle) {
            let project =
                self.project_for_workspace(&preflight.project_id, &preflight.workspace)?;
            if project.codex_thread_id.as_deref() == Some(&evidence.codex_thread_id) {
                return Ok(project);
            }
        }
        if preflight.confirmed_candidate_handle.is_some() || now_unix > preflight.expires_at_unix {
            return Err(RuntimeError::Validation(
                "thread rotation confirmation expired or was already used".into(),
            ));
        }
        let project =
            self.rotate_codex_thread_after(&preflight.project_id, old, evidence, || {
                if self.load_thread_adoption_preflight(confirmation_token)? != preflight {
                    return Err(RuntimeError::Validation(
                        "thread rotation preflight changed during confirmation".into(),
                    ));
                }
                before_commit()
            })?;
        preflight.confirmed_candidate_handle = Some(candidate_handle.into());
        self.write_adoption_preflight(&preflight)?;
        Ok(project)
    }

    pub fn bind_project_chat_target(
        &self,
        project_id: &str,
        target_url: &str,
        expected_current_target_sha256: Option<&str>,
    ) -> Result<AutonomousProjectV1, RuntimeError> {
        self.bind_project_chat_target_after(
            project_id,
            target_url,
            expected_current_target_sha256,
            || Ok(()),
        )
    }

    /// Runs one fixed caller-supplied pre-commit operation while holding the
    /// same registry lock as the target CAS. This is crate-internal plumbing
    /// for the designated-chat transaction: it prevents another reviewed
    /// project-target update from interleaving between the effective-wake
    /// update and its matching registry commit. It is not a general project
    /// patch or a caller-selected path authority.
    pub(crate) fn bind_project_chat_target_after<F>(
        &self,
        project_id: &str,
        target_url: &str,
        expected_current_target_sha256: Option<&str>,
        before_commit: F,
    ) -> Result<AutonomousProjectV1, RuntimeError>
    where
        F: FnOnce() -> Result<(), RuntimeError>,
    {
        valid_slug(project_id, "project id")?;
        let target_url = canonical_project_chat_target(target_url)?;
        let target_sha256 = project_chat_target_digest(&target_url);
        if let Some(expected) = expected_current_target_sha256 {
            if !valid_sha256(expected) {
                return Err(RuntimeError::Validation(
                    "expected project target digest is invalid".into(),
                ));
            }
        }
        let _lock = self.acquire_registration_lock()?;
        let mut registry = self.load_registry()?;
        let project = registry
            .projects
            .iter_mut()
            .find(|project| project.project_id == project_id)
            .ok_or_else(|| RuntimeError::Validation("project is not registered".into()))?;
        if project.chatgpt_target_sha256.as_deref() != expected_current_target_sha256 {
            return Err(RuntimeError::Validation(
                "project target compare-and-swap did not match".into(),
            ));
        }
        before_commit()?;
        project.chatgpt_target_url = Some(target_url);
        project.chatgpt_target_sha256 = Some(target_sha256);
        let bound = project.clone();
        self.save_registry(&registry)?;
        Ok(bound)
    }

    /// One-time initialization for an already registered project. Unlike CAS,
    /// this deliberately rejects every existing target, including an equal one.
    pub fn initialize_project_chat_target(
        &self,
        project_id: &str,
        target_url: &str,
    ) -> Result<AutonomousProjectV1, RuntimeError> {
        valid_slug(project_id, "project id")?;
        let target_url = canonical_project_chat_target(target_url)?;
        let _lock = self.acquire_registration_lock()?;
        let mut registry = self.load_registry()?;
        let project = registry
            .projects
            .iter_mut()
            .find(|project| project.project_id == project_id)
            .ok_or_else(|| RuntimeError::Validation("project is not registered".into()))?;
        if project.chatgpt_target_url.is_some() || project.chatgpt_target_sha256.is_some() {
            return Err(RuntimeError::Validation(
                "project ChatGPT target is already initialized".into(),
            ));
        }
        project.chatgpt_target_url = Some(target_url.clone());
        project.chatgpt_target_sha256 = Some(project_chat_target_digest(&target_url));
        let result = project.clone();
        self.save_registry(&registry)?;
        Ok(result)
    }

    /// One-way compatibility migration for the historical CatDesk-local wake
    /// target. This is deliberately unavailable for every external project;
    /// it never overwrites an existing durable per-project target.
    pub fn migrate_catdesk_chat_target_if_unbound(
        &self,
        workspace: &Path,
        target_url: &str,
    ) -> Result<Option<AutonomousProjectV1>, RuntimeError> {
        let workspace = workspace.canonicalize().map_err(|_| {
            RuntimeError::Validation("CatDesk migration workspace canonicalization failed".into())
        })?;
        let target_url = canonical_project_chat_target(target_url)?;
        let _lock = self.acquire_registration_lock()?;
        let mut registry = self.load_registry()?;
        let Some(project) = registry.projects.iter_mut().find(|project| {
            project.project_id == CATDESK_PROJECT_ID_V1 && project.workspace == workspace
        }) else {
            return Ok(None);
        };
        if project.chatgpt_target_url.is_some() || project.chatgpt_target_sha256.is_some() {
            return Ok(Some(project.clone()));
        }
        project.chatgpt_target_url = Some(target_url.clone());
        project.chatgpt_target_sha256 = Some(project_chat_target_digest(&target_url));
        let migrated = project.clone();
        self.save_registry(&registry)?;
        Ok(Some(migrated))
    }

    /// Returns a durable one-writer lease, rejecting same-workspace work even
    /// when it is registered under distinct project ids. Expiration is only a
    /// recovery signal: the owner must explicitly release the lease after the
    /// worker has stopped. Elapsed time alone cannot prove that it stopped.
    pub fn schedule_mutating_worker(
        &self,
        project_id: &str,
        owner_id: &str,
        provider: ScheduledProviderV1,
        now_unix: u64,
        expires_at_unix: u64,
    ) -> Result<GlobalScheduleDecisionV1, RuntimeError> {
        let _lock = self.acquire_scheduler_lock()?;
        if let Some(cursor) = self.load_round_robin_cursor()? {
            let registry = self.load_registry()?;
            if !registry
                .projects
                .iter()
                .any(|project| project.project_id == cursor.last_granted_project_id)
            {
                return Err(RuntimeError::Validation(
                    "scheduler cursor project is not registered".into(),
                ));
            }
        }
        self.schedule_mutating_worker_locked(
            project_id,
            owner_id,
            provider,
            now_unix,
            expires_at_unix,
        )
    }

    /// Grant one registered, ready project in persistent registry order. The
    /// caller may mark projects ready, but cannot supply a workspace or change
    /// their rotation order. A held lease is skipped without consuming a turn.
    /// This only reserves a workspace; it never starts a provider or worker.
    pub fn schedule_next_ready_worker(
        &self,
        ready_project_ids: &[&str],
        owner_id: &str,
        provider: ScheduledProviderV1,
        now_unix: u64,
        expires_at_unix: u64,
    ) -> Result<GlobalScheduleDecisionV1, RuntimeError> {
        valid_slug(owner_id, "lease owner id")?;
        if expires_at_unix <= now_unix {
            return Err(RuntimeError::Validation(
                "lease expiry must be in the future".into(),
            ));
        }
        let _lock = self.acquire_scheduler_lock()?;
        let registry = self.load_registry()?;
        for (index, project_id) in ready_project_ids.iter().enumerate() {
            valid_slug(project_id, "project id")?;
            if ready_project_ids[..index].contains(project_id)
                || !registry
                    .projects
                    .iter()
                    .any(|project| project.project_id == *project_id)
            {
                return Err(RuntimeError::Validation(
                    "ready project ids must be unique registered projects".into(),
                ));
            }
        }
        if ready_project_ids.is_empty() || registry.projects.is_empty() {
            return Ok(GlobalScheduleDecisionV1::Rejected {
                reason: "NO_READY_PROJECTS",
            });
        }
        let start = match self.load_round_robin_cursor()? {
            Some(cursor) => {
                registry
                    .projects
                    .iter()
                    .position(|project| project.project_id == cursor.last_granted_project_id)
                    .ok_or_else(|| {
                        RuntimeError::Validation(
                            "scheduler cursor project is not registered".into(),
                        )
                    })?
                    .wrapping_add(1)
                    % registry.projects.len()
            }
            None => 0,
        };
        for offset in 0..registry.projects.len() {
            let project = &registry.projects[(start + offset) % registry.projects.len()];
            if !ready_project_ids.contains(&project.project_id.as_str()) {
                continue;
            }
            match self.schedule_mutating_worker_locked(
                &project.project_id,
                owner_id,
                provider.clone(),
                now_unix,
                expires_at_unix,
            )? {
                GlobalScheduleDecisionV1::Granted(lease) => {
                    if let Err(error) = self.save_round_robin_cursor(&project.project_id) {
                        self.release_workspace_lease_locked(&lease)?;
                        return Err(error);
                    }
                    return Ok(GlobalScheduleDecisionV1::Granted(lease));
                }
                GlobalScheduleDecisionV1::Rejected {
                    reason: "GLOBAL_PROVIDER_CAPACITY_REACHED",
                } => {
                    return Ok(GlobalScheduleDecisionV1::Rejected {
                        reason: "GLOBAL_PROVIDER_CAPACITY_REACHED",
                    });
                }
                GlobalScheduleDecisionV1::Rejected { .. } => {}
            }
        }
        Ok(GlobalScheduleDecisionV1::Rejected {
            reason: "NO_READY_WORKSPACE_AVAILABLE",
        })
    }

    fn schedule_mutating_worker_locked(
        &self,
        project_id: &str,
        owner_id: &str,
        provider: ScheduledProviderV1,
        now_unix: u64,
        expires_at_unix: u64,
    ) -> Result<GlobalScheduleDecisionV1, RuntimeError> {
        valid_slug(project_id, "project id")?;
        valid_slug(owner_id, "lease owner id")?;
        if expires_at_unix <= now_unix {
            return Err(RuntimeError::Validation(
                "lease expiry must be in the future".into(),
            ));
        }
        let registry = self.load_registry()?;
        let project = registry
            .projects
            .iter()
            .find(|project| project.project_id == project_id)
            .ok_or_else(|| RuntimeError::Validation("project is not registered".into()))?;
        let leases = self.all_leases()?;
        if leases
            .iter()
            .any(|lease| lease.workspace == project.workspace)
        {
            return Ok(GlobalScheduleDecisionV1::Rejected {
                reason: "WORKSPACE_WRITER_LEASE_HELD",
            });
        }
        let active = leases
            .iter()
            .filter(|lease| lease.provider == provider)
            .count() as u32;
        let maximum = match provider {
            ScheduledProviderV1::Codex => registry.maximum_codex_workers,
            ScheduledProviderV1::Qwen => registry.maximum_qwen_workers,
        };
        if active >= maximum {
            return Ok(GlobalScheduleDecisionV1::Rejected {
                reason: "GLOBAL_PROVIDER_CAPACITY_REACHED",
            });
        }
        let lease = AutonomousWorkspaceLeaseV1 {
            project_id: project.project_id.clone(),
            workspace: project.workspace.clone(),
            owner_id: owner_id.into(),
            provider,
            expires_at_unix,
        };
        let lease_path = self.lease_path(&lease.workspace);
        // Write outside the enumerated leases directory, then link the fully
        // synced file into it. A crash before the link cannot expose partial
        // JSON as a lease; a competing writer cannot replace an existing link.
        let draft_path = self
            .root
            .join(format!(".lease-draft-{}.tmp", Uuid::new_v4()));
        let encoded = serde_json::to_vec_pretty(&lease)
            .map_err(|_| RuntimeError::Validation("lease serialization failed".into()))?;
        if encoded.len() as u64 + 1 > MAX_SCHEDULER_LEASE_BYTES {
            return Err(RuntimeError::Validation(
                "workspace lease exceeds the scheduler record limit".into(),
            ));
        }
        let mut draft = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&draft_path)
            .map_err(io_error)?;
        let write_result = draft
            .write_all(&encoded)
            .and_then(|()| draft.write_all(b"\n"))
            .and_then(|()| draft.sync_all());
        drop(draft);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&draft_path);
            return Err(io_error(error));
        }
        let publication = fs::hard_link(&draft_path, &lease_path);
        // A failed cleanup may leave a complete draft outside leases, but the
        // publication outcome remains authoritative to the caller.
        let _ = fs::remove_file(&draft_path);
        match publication {
            Ok(()) => Ok(GlobalScheduleDecisionV1::Granted(lease)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Ok(GlobalScheduleDecisionV1::Rejected {
                    reason: "WORKSPACE_WRITER_LEASE_HELD",
                })
            }
            Err(error) => Err(io_error(error)),
        }
    }

    pub fn release_workspace_lease(
        &self,
        lease: &AutonomousWorkspaceLeaseV1,
    ) -> Result<(), RuntimeError> {
        let _lock = self.acquire_scheduler_lock()?;
        self.release_workspace_lease_locked(lease)
    }

    fn release_workspace_lease_locked(
        &self,
        lease: &AutonomousWorkspaceLeaseV1,
    ) -> Result<(), RuntimeError> {
        let path = self.lease_path(&lease.workspace);
        checked_registry_directory(&self.root.join("leases"), false)?;
        let file = checked_registry_file(
            &path,
            MAX_SCHEDULER_LEASE_BYTES,
            "workspace lease is redirected or oversized",
        )?
        .ok_or_else(|| RuntimeError::Validation("workspace lease is unavailable".into()))?;
        let bytes = read_bounded_registry_bytes(
            file,
            MAX_SCHEDULER_LEASE_BYTES,
            "workspace lease is oversized",
        )?;
        let stored: AutonomousWorkspaceLeaseV1 = serde_json::from_slice(&bytes)
            .map_err(|_| RuntimeError::Validation("workspace lease is corrupted".into()))?;
        if &stored != lease {
            return Err(RuntimeError::Validation(
                "workspace lease ownership mismatch".into(),
            ));
        }
        checked_registry_file(
            &path,
            MAX_SCHEDULER_LEASE_BYTES,
            "workspace lease is redirected or oversized",
        )?
        .ok_or_else(|| RuntimeError::Validation("workspace lease is unavailable".into()))?;
        fs::remove_file(path).map_err(io_error)
    }

    fn all_leases(&self) -> Result<Vec<AutonomousWorkspaceLeaseV1>, RuntimeError> {
        checked_registry_directory(&self.root.join("leases"), false)?;
        let registry = self.load_registry()?;
        let mut leases = Vec::new();
        for entry in fs::read_dir(self.root.join("leases")).map_err(io_error)? {
            if leases.len() >= MAX_SCHEDULER_LEASES {
                return Err(RuntimeError::Validation(
                    "workspace lease set exceeds registered project limit".into(),
                ));
            }
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let file = checked_registry_file(
                &path,
                MAX_SCHEDULER_LEASE_BYTES,
                "workspace lease is redirected or oversized",
            )?
            .ok_or_else(|| RuntimeError::Validation("workspace lease is unavailable".into()))?;
            let bytes = read_bounded_registry_bytes(
                file,
                MAX_SCHEDULER_LEASE_BYTES,
                "workspace lease is oversized",
            )?;
            let lease: AutonomousWorkspaceLeaseV1 = serde_json::from_slice(&bytes)
                .map_err(|_| RuntimeError::Validation("workspace lease is corrupted".into()))?;
            if path != self.lease_path(&lease.workspace)
                || valid_slug(&lease.project_id, "workspace lease project id").is_err()
                || valid_slug(&lease.owner_id, "workspace lease owner id").is_err()
                || lease.expires_at_unix == 0
                || !registry.projects.iter().any(|project| {
                    project.project_id == lease.project_id && project.workspace == lease.workspace
                })
            {
                return Err(RuntimeError::Validation(
                    "workspace lease does not match its registered scope".into(),
                ));
            }
            leases.push(lease);
        }
        Ok(leases)
    }

    fn registry_path(&self) -> PathBuf {
        self.root.join("projects.json")
    }
    fn preflight_path(&self, token: &str) -> PathBuf {
        self.root
            .join("registration-confirmations")
            .join(format!("{token}.json"))
    }
    fn adoption_preflight_path(&self, token: &str) -> PathBuf {
        self.root
            .join("thread-adoptions")
            .join(format!("{token}.json"))
    }
    fn registration_lock_path(&self) -> PathBuf {
        self.root.join("registration-confirm.lock")
    }
    fn round_robin_cursor_path(&self) -> PathBuf {
        self.root.join("scheduler-cursor.json")
    }
    fn load_round_robin_cursor(&self) -> Result<Option<RoundRobinCursorV1>, RuntimeError> {
        let file = match checked_registry_file(
            &self.round_robin_cursor_path(),
            MAX_SCHEDULER_CURSOR_BYTES,
            "scheduler cursor is redirected or oversized",
        )? {
            Some(file) => file,
            None => return Ok(None),
        };
        let bytes = read_bounded_registry_bytes(
            file,
            MAX_SCHEDULER_CURSOR_BYTES,
            "scheduler cursor is oversized",
        )?;
        let cursor: RoundRobinCursorV1 = serde_json::from_slice(&bytes)
            .map_err(|_| RuntimeError::Validation("scheduler cursor is corrupted".into()))?;
        if cursor.schema_version != 1
            || valid_slug(
                &cursor.last_granted_project_id,
                "scheduler cursor project id",
            )
            .is_err()
        {
            return Err(RuntimeError::Validation(
                "scheduler cursor is corrupted".into(),
            ));
        }
        Ok(Some(cursor))
    }
    fn save_round_robin_cursor(&self, project_id: &str) -> Result<(), RuntimeError> {
        let tmp = self
            .root
            .join(format!(".scheduler-cursor-{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(io_error)?;
        serde_json::to_writer(
            &mut file,
            &RoundRobinCursorV1 {
                schema_version: 1,
                last_granted_project_id: project_id.into(),
            },
        )
        .map_err(|_| RuntimeError::Validation("scheduler cursor serialization failed".into()))?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        if let Err(error) = checked_registry_file(
            &self.round_robin_cursor_path(),
            MAX_SCHEDULER_CURSOR_BYTES,
            "scheduler cursor is redirected or oversized",
        ) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        fs::rename(tmp, self.round_robin_cursor_path()).map_err(io_error)
    }
    fn lease_path(&self, workspace: &Path) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(workspace.to_string_lossy().as_bytes());
        self.root
            .join("leases")
            .join(format!("{:x}.json", hasher.finalize()))
    }
    fn save_registry(&self, registry: &AutonomousProjectRegistryV1) -> Result<(), RuntimeError> {
        validate_registry(registry)?;
        let encoded = serde_json::to_vec_pretty(registry).map_err(|_| {
            RuntimeError::Validation("project registry serialization failed".into())
        })?;
        if encoded.len() as u64 + 1 > MAX_PROJECT_REGISTRY_BYTES {
            return Err(RuntimeError::Validation(
                "project registry exceeds the record limit".into(),
            ));
        }
        let path = self.registry_path();
        let tmp = self.root.join(format!(".projects-{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(io_error)?;
        file.write_all(&encoded).map_err(io_error)?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        fs::rename(tmp, path).map_err(io_error)
    }
    fn write_preflight(
        &self,
        preflight: &ProjectRegistrationPreflightV1,
    ) -> Result<(), RuntimeError> {
        validate_preflight(preflight)?;
        let path = self.preflight_path(&preflight.confirmation_token);
        let tmp = self.root.join("registration-confirmations").join(format!(
            ".{}-{}.tmp",
            preflight.confirmation_token,
            Uuid::new_v4()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(io_error)?;
        serde_json::to_writer(&mut file, preflight).map_err(|_| {
            RuntimeError::Validation("registration preflight serialization failed".into())
        })?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        fs::rename(tmp, path).map_err(io_error)
    }
    fn load_preflight(&self, token: &str) -> Result<ProjectRegistrationPreflightV1, RuntimeError> {
        let file = OpenOptions::new()
            .read(true)
            .open(self.preflight_path(token))
            .map_err(io_error)?;
        let preflight = serde_json::from_reader(file).map_err(|_| {
            RuntimeError::Validation("registration confirmation is corrupted".into())
        })?;
        validate_preflight(&preflight)?;
        Ok(preflight)
    }
    fn write_adoption_preflight(
        &self,
        preflight: &ProjectThreadAdoptionPreflightV1,
    ) -> Result<(), RuntimeError> {
        validate_adoption_preflight(preflight)?;
        let path = self.adoption_preflight_path(&preflight.confirmation_token);
        let tmp = self.root.join("thread-adoptions").join(format!(
            ".{}-{}.tmp",
            preflight.confirmation_token,
            Uuid::new_v4()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(io_error)?;
        serde_json::to_writer(&mut file, preflight)
            .map_err(|_| RuntimeError::Validation("thread adoption serialization failed".into()))?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        fs::rename(tmp, path).map_err(io_error)
    }
    pub fn load_thread_adoption_preflight(
        &self,
        token: &str,
    ) -> Result<ProjectThreadAdoptionPreflightV1, RuntimeError> {
        let file = OpenOptions::new()
            .read(true)
            .open(self.adoption_preflight_path(token))
            .map_err(io_error)?;
        let preflight = serde_json::from_reader(file).map_err(|_| {
            RuntimeError::Validation("thread adoption confirmation is corrupted".into())
        })?;
        validate_adoption_preflight(&preflight)?;
        Ok(preflight)
    }
    fn acquire_registration_lock(&self) -> Result<RegistrationLockV1, RuntimeError> {
        let path = self.registration_lock_path();
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => Ok(RegistrationLockV1 { path }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(
                RuntimeError::Validation("registration confirmation is already in progress".into()),
            ),
            Err(error) => Err(io_error(error)),
        }
    }

    fn acquire_thread_transition_lock(&self) -> Result<std::fs::File, RuntimeError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("thread-transition.lock"))
            .map_err(io_error)?;
        file.try_lock().map_err(|_| {
            RuntimeError::Validation("project thread transition is already in progress".into())
        })?;
        Ok(file)
    }

    fn acquire_scheduler_lock(&self) -> Result<std::fs::File, RuntimeError> {
        checked_registry_directory(&self.root, false)?;
        checked_registry_directory(&self.root.join("leases"), false)?;
        let path = self.root.join("scheduler.lock");
        checked_registry_file(&path, 0, "project scheduler lock is redirected or corrupt")?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW);
        let file = options.open(&path).map_err(io_error)?;
        let opened = file.metadata().map_err(io_error)?;
        if !opened.is_file() || redirected_entry(&opened) || opened.len() != 0 {
            return Err(RuntimeError::Validation(
                "project scheduler lock is redirected or corrupt".into(),
            ));
        }
        checked_registry_file(&path, 0, "project scheduler lock is redirected or corrupt")?
            .ok_or_else(|| RuntimeError::Validation("project scheduler lock vanished".into()))?;
        file.try_lock().map_err(|_| {
            RuntimeError::Validation("project scheduler is already in progress".into())
        })?;
        Ok(file)
    }

    /// Session approval takes this same lock while it changes a draft into a
    /// runnable writer. Rotation can then check quiescence and commit without
    /// an approval interleaving between those steps.
    pub(crate) fn with_thread_transition_lock<T, F>(&self, operation: F) -> Result<T, String>
    where
        F: FnOnce() -> Result<T, String>,
    {
        let _lock = self
            .acquire_thread_transition_lock()
            .map_err(|_| "project thread transition is already in progress".to_string())?;
        operation()
    }
}

struct RegistrationLockV1 {
    path: PathBuf,
}
impl Drop for RegistrationLockV1 {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn validate_registry(registry: &AutonomousProjectRegistryV1) -> Result<(), RuntimeError> {
    if registry.schema_version != PROJECT_REGISTRY_SCHEMA_VERSION
        || registry.maximum_codex_workers == 0
        || registry.maximum_qwen_workers == 0
        || registry.projects.len() > 128
    {
        return Err(RuntimeError::Validation(
            "project registry shape is invalid".into(),
        ));
    }
    for project in &registry.projects {
        validate_project(project)?;
    }
    for (index, project) in registry.projects.iter().enumerate() {
        if registry
            .projects
            .iter()
            .skip(index + 1)
            .any(|other| other.project_id == project.project_id)
        {
            return Err(RuntimeError::Validation(
                "project registry ids must be unique".into(),
            ));
        }
        if registry.projects.iter().skip(index + 1).any(|other| {
            other.workspace == project.workspace
                || project.codex_thread_id.is_some()
                    && project.codex_thread_id == other.codex_thread_id
        }) {
            return Err(RuntimeError::Validation(
                "project registry workspace or Codex thread binding is not unique".into(),
            ));
        }
    }
    Ok(())
}
fn validate_project(project: &AutonomousProjectV1) -> Result<(), RuntimeError> {
    valid_slug(&project.project_id, "project id")?;
    if project.workspace.as_os_str().is_empty()
        || project.git_identity.trim().is_empty()
        || project.git_identity.len() > 1024
        || project.verification_profile.trim().is_empty()
        || project.verification_profile.len() > 256
    {
        return Err(RuntimeError::Validation(
            "project registration is incomplete or unbounded".into(),
        ));
    }
    if let Some(thread_id) = &project.codex_thread_id {
        valid_slug(thread_id, "Codex thread id")?;
    }
    match (&project.chatgpt_target_url, &project.chatgpt_target_sha256) {
        (None, None) => {}
        (Some(url), Some(digest))
            if canonical_project_chat_target(url).ok().as_deref() == Some(url)
                && project_chat_target_digest(url) == *digest => {}
        _ => {
            return Err(RuntimeError::Validation(
                "project ChatGPT target binding is invalid".into(),
            ));
        }
    }
    Ok(())
}
pub fn canonical_project_chat_target(value: &str) -> Result<String, RuntimeError> {
    if value.is_empty()
        || value.len() > 512
        || !value.is_ascii()
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(RuntimeError::Validation(
            "project ChatGPT target is invalid".into(),
        ));
    }
    let parsed = Url::parse(value)
        .map_err(|_| RuntimeError::Validation("project ChatGPT target is invalid".into()))?;
    if parsed.scheme() != "https"
        || !matches!(parsed.host_str(), Some("chatgpt.com" | "chat.openai.com"))
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(RuntimeError::Validation(
            "project ChatGPT target is invalid".into(),
        ));
    }
    let segments = parsed
        .path_segments()
        .ok_or_else(|| RuntimeError::Validation("project ChatGPT target is invalid".into()))?
        .collect::<Vec<_>>();
    let valid_id = |value: &str| {
        !value.is_empty()
            && value.len() <= 200
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    };
    let valid_conversation_id = |value: &str| {
        valid_id(value) || (value.len() <= 200 && value.strip_prefix("WEB:").is_some_and(valid_id))
    };
    let path = match segments.as_slice() {
        ["c", conversation] if valid_conversation_id(conversation) => {
            format!("/c/{conversation}")
        }
        ["g", project, "c", conversation]
            if valid_id(project) && valid_conversation_id(conversation) =>
        {
            format!("/g/{project}/c/{conversation}")
        }
        _ => {
            return Err(RuntimeError::Validation(
                "project ChatGPT target is invalid".into(),
            ));
        }
    };
    Ok(format!(
        "https://{}{path}",
        parsed.host_str().expect("validated target host")
    ))
}
pub fn project_chat_target_digest(target: &str) -> String {
    format!("{:x}", Sha256::digest(target.as_bytes()))
}
fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn validate_registration_profile(value: &str) -> Result<(), RuntimeError> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(RuntimeError::Validation(
            "registration verification profile is invalid".into(),
        ));
    }
    Ok(())
}
fn validate_registration_evidence(
    evidence: &ProjectRegistrationEvidenceV1,
) -> Result<(), RuntimeError> {
    if evidence.workspace.as_os_str().is_empty()
        || evidence.git_identity.is_empty()
        || evidence.git_identity.len() > 1024
        || required_effort(&evidence.selected_model) != Some(evidence.reasoning_effort.as_str())
    {
        return Err(RuntimeError::Validation(
            "registration evidence is incomplete or has no exact supported model profile".into(),
        ));
    }
    valid_slug(&evidence.codex_thread_id, "Codex thread id")
}
fn valid_confirmation_token(token: &str) -> Result<(), RuntimeError> {
    if token.len() != 39
        || !token.starts_with("prjreg-")
        || !token[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(RuntimeError::Validation(
            "registration confirmation token is invalid".into(),
        ));
    }
    Ok(())
}
fn valid_adoption_token(token: &str) -> Result<(), RuntimeError> {
    if token.len() != 38
        || !token.starts_with("adopt-")
        || !token[6..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(RuntimeError::Validation(
            "thread adoption confirmation token is invalid".into(),
        ));
    }
    Ok(())
}
fn valid_candidate_handle(value: &str) -> Result<(), RuntimeError> {
    if value.len() != 42
        || !value.starts_with("candidate-")
        || !value[10..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(RuntimeError::Validation(
            "thread adoption candidate handle is invalid".into(),
        ));
    }
    Ok(())
}
fn adoption_fingerprint(domain: &str, value: &str) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(format!("{domain}\0{value}").as_bytes())
    )
}
fn contains_secret_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "authorization",
        "bearer ",
        "password",
        "secret",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}
fn validate_adoption_preflight(
    preflight: &ProjectThreadAdoptionPreflightV1,
) -> Result<(), RuntimeError> {
    if preflight.schema_version != PROJECT_THREAD_ADOPTION_PREFLIGHT_SCHEMA_VERSION
        || preflight.expires_at_unix == 0
        || preflight.candidates.is_empty()
        || preflight.candidates.len() > 8
    {
        return Err(RuntimeError::Validation(
            "thread adoption preflight is invalid".into(),
        ));
    }
    valid_adoption_token(&preflight.confirmation_token)?;
    valid_slug(&preflight.project_id, "project id")?;
    if let Some(previous) = &preflight.expected_previous_thread_id {
        valid_slug(previous, "previous Codex thread id")?;
    }
    let workspace = preflight
        .workspace
        .canonicalize()
        .map_err(|_| RuntimeError::Validation("thread adoption workspace is invalid".into()))?;
    if workspace != preflight.workspace {
        return Err(RuntimeError::Validation(
            "thread adoption workspace is not canonical".into(),
        ));
    }
    for candidate in &preflight.candidates {
        valid_candidate_handle(&candidate.candidate_handle)?;
        validate_registration_evidence(&candidate.evidence)?;
        if candidate.evidence.workspace != preflight.workspace
            || preflight.expected_previous_thread_id.as_deref()
                == Some(candidate.evidence.codex_thread_id.as_str())
            || preflight.expected_previous_thread_id.is_some()
                && candidate.evidence.selected_model != super::codex_model_policy::SOL_MEDIUM_MODEL
            || candidate.thread_fingerprint
                != adoption_fingerprint("thread", &candidate.evidence.codex_thread_id)
            || candidate.cwd_fingerprint
                != adoption_fingerprint("cwd", &preflight.workspace.to_string_lossy())
            || candidate
                .title
                .as_deref()
                .is_some_and(|title| title.len() > 512 || contains_secret_marker(title))
        {
            return Err(RuntimeError::Validation(
                "thread adoption candidate is invalid".into(),
            ));
        }
    }
    if let Some(handle) = &preflight.confirmed_candidate_handle {
        valid_candidate_handle(handle)?;
    }
    Ok(())
}
fn registration_fingerprint(
    project_id: &str,
    profile: &str,
    evidence: &ProjectRegistrationEvidenceV1,
) -> String {
    let mut hasher = Sha256::new();
    let workspace = evidence.workspace.to_string_lossy();
    for item in [
        project_id,
        profile,
        workspace.as_ref(),
        &evidence.git_identity,
        &evidence.codex_thread_id,
        &evidence.selected_model,
        &evidence.reasoning_effort,
    ] {
        hasher.update(item.as_bytes());
        hasher.update([0]);
    }
    format!("sha256:{:x}", hasher.finalize())
}
fn validate_preflight(preflight: &ProjectRegistrationPreflightV1) -> Result<(), RuntimeError> {
    if preflight.schema_version != PROJECT_REGISTRATION_PREFLIGHT_SCHEMA_VERSION
        || preflight.confirmation_fingerprint.len() != 71
        || !preflight.confirmation_fingerprint.starts_with("sha256:")
        || !preflight.confirmation_fingerprint[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || preflight.expires_at_unix == 0
    {
        return Err(RuntimeError::Validation(
            "registration preflight is invalid".into(),
        ));
    }
    valid_confirmation_token(&preflight.confirmation_token)?;
    valid_slug(&preflight.project_id, "project id")?;
    validate_registration_profile(&preflight.verification_profile)?;
    validate_registration_evidence(&preflight.evidence)
}
fn reject_registration_conflicts(
    registry: &AutonomousProjectRegistryV1,
    project_id: &str,
    profile: &str,
    evidence: &ProjectRegistrationEvidenceV1,
) -> Result<(), RuntimeError> {
    if let Some(existing) = registry
        .projects
        .iter()
        .find(|project| project.project_id == project_id)
    {
        if existing.workspace == evidence.workspace
            && existing.git_identity == evidence.git_identity
            && existing.verification_profile == profile
            && existing.codex_thread_id.as_deref() == Some(&evidence.codex_thread_id)
        {
            return Ok(());
        }
        return Err(RuntimeError::Validation(
            "project id conflicts with an existing registration".into(),
        ));
    }
    if registry
        .projects
        .iter()
        .any(|project| project.workspace == evidence.workspace)
    {
        return Err(RuntimeError::Validation(
            "workspace is already registered".into(),
        ));
    }
    if registry
        .projects
        .iter()
        .any(|project| project.codex_thread_id.as_deref() == Some(&evidence.codex_thread_id))
    {
        return Err(RuntimeError::Validation(
            "Codex thread is already registered".into(),
        ));
    }
    Ok(())
}
fn exact_registered_binding(
    registry: &AutonomousProjectRegistryV1,
    preflight: &ProjectRegistrationPreflightV1,
) -> Result<AutonomousProjectV1, RuntimeError> {
    let project = registry
        .projects
        .iter()
        .find(|project| project.project_id == preflight.project_id)
        .ok_or_else(|| RuntimeError::Validation("confirmed registration is absent".into()))?;
    if project.workspace != preflight.evidence.workspace
        || project.git_identity != preflight.evidence.git_identity
        || project.verification_profile != preflight.verification_profile
        || project.codex_thread_id.as_deref() != Some(&preflight.evidence.codex_thread_id)
    {
        return Err(RuntimeError::Validation(
            "confirmed registration no longer matches preflight".into(),
        ));
    }
    Ok(project.clone())
}
fn valid_slug(value: &str, label: &str) -> Result<(), RuntimeError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_'))
    {
        Err(RuntimeError::Validation(format!(
            "{label} must be a conservative slug"
        )))
    } else {
        Ok(())
    }
}
fn io_error(error: std::io::Error) -> RuntimeError {
    RuntimeError::Provider(format!("project registry I/O error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn project(id: &str, workspace: PathBuf) -> AutonomousProjectV1 {
        AutonomousProjectV1 {
            project_id: id.into(),
            workspace,
            git_identity: format!("fixture-{id}"),
            verification_profile: "cargo-test".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        }
    }

    fn scheduler_preflight_fixture(
        label: &str,
    ) -> (PathBuf, AutonomousProjectRegistryStoreV1, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "catdesk-scheduler-preflight-{label}-{}",
            Uuid::new_v4()
        ));
        let workspace = root.join("alpha");
        fs::create_dir_all(&workspace).expect("fixture workspace");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry"))
            .expect("fixture registry");
        store
            .initialize(1, 1)
            .expect("fixture registry initialized");
        store
            .register_project(project("alpha", workspace.clone()))
            .expect("fixture project registered");
        (
            root,
            store,
            workspace.canonicalize().expect("canonical workspace"),
        )
    }

    #[cfg(windows)]
    fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    #[cfg(unix)]
    fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[cfg(unix)]
    fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    fn symlink_available(result: std::io::Result<()>) -> bool {
        match result {
            Ok(()) => true,
            Err(error)
                if cfg!(windows)
                    && (error.kind() == std::io::ErrorKind::PermissionDenied
                        || error.raw_os_error() == Some(1314)) =>
            {
                // Windows hosts without Developer Mode or the symlink privilege
                // cannot create this fixture. Type/size tests still run there.
                false
            }
            Err(error) => panic!("fixture symlink creation failed: {error}"),
        }
    }

    fn fixture_git_root(root: &Path, origin: &str) {
        fs::create_dir_all(root).expect("fixture root");
        for args in [
            &["init", "-q"][..],
            &["remote", "add", "origin", origin][..],
        ] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(args)
                    .status()
                    .expect("fixture Git available")
                    .success()
            );
        }
    }

    fn cleanup_control_resolver_fixture(root: &Path) {
        let temp_root = std::env::temp_dir()
            .canonicalize()
            .expect("canonical temp root");
        let exact = root.canonicalize().expect("canonical fixture root");
        assert_eq!(exact.parent(), Some(temp_root.as_path()));
        assert!(
            exact
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("catdesk-control-resolver-"))
        );
        fs::remove_dir_all(exact).expect("remove verified fixture root");
    }

    #[test]
    fn control_root_resolves_three_git_projects_without_external_registry_writes() {
        let fixture =
            std::env::temp_dir().join(format!("catdesk-control-resolver-{}", Uuid::new_v4()));
        let control = fixture.join("catdesk");
        let driver = fixture.join("driver");
        let bounty = fixture.join("bounty");
        let roots = [
            (
                "catdesk",
                control.as_path(),
                "https://example.test/catdesk.git",
                "thread-catdesk",
                "https://chatgpt.com/c/6ace36d3-8634-8333-a79c-1a4cba32caeb",
            ),
            (
                "BYOVD_DRIVER_PIPELINE",
                driver.as_path(),
                "https://example.test/driver.git",
                "thread-driver",
                "https://chatgpt.com/c/69e7a205-23c4-83ea-a93a-38344c7f9ff7",
            ),
            (
                "BUG_BOUNTY_RECON_PLATFORM",
                bounty.as_path(),
                "https://example.test/bounty.git",
                "thread-bounty",
                "https://chatgpt.com/c/6a763063-adcc-83ea-9400-ea5689b5c910",
            ),
        ];
        for (_, root, origin, _, _) in roots {
            fixture_git_root(root, origin);
        }
        let store = AutonomousProjectRegistryStoreV1::open(control.join(".catdesk/projects"))
            .expect("control registry");
        store.initialize(1, 1).expect("registry");
        for (id, root, origin, thread, target) in roots {
            let mut registered = project(id, root.to_path_buf());
            registered.git_identity = origin.into();
            registered.codex_thread_id = Some(thread.into());
            registered.chatgpt_target_url = Some(target.into());
            registered.chatgpt_target_sha256 = Some(project_chat_target_digest(target));
            store
                .register_project(registered)
                .expect("register fixture");
        }
        let registry_path = control.join(".catdesk/projects/projects.json");
        let before = fs::read(&registry_path).expect("registry bytes before read");
        for (id, root, origin, thread, target) in roots {
            let resolved = resolve_registered_project_from_control_root(&control, id)
                .expect("registered project resolution");
            assert_eq!(
                resolved.control_workspace,
                control.canonicalize().expect("control")
            );
            assert_eq!(resolved.project.project_id, id);
            assert_eq!(
                resolved.project.workspace,
                root.canonicalize().expect("project")
            );
            assert_eq!(resolved.project.git_identity, origin);
            assert_eq!(resolved.project.codex_thread_id.as_deref(), Some(thread));
            assert_eq!(resolved.project.chatgpt_target_url.as_deref(), Some(target));
        }
        assert!(resolve_registered_project_from_control_root(&control, "unregistered").is_err());
        assert!(resolve_registered_project_from_control_root(&driver, "catdesk").is_err());
        assert_eq!(
            fs::read(&registry_path).expect("registry bytes after read"),
            before
        );
        for root in [&driver, &bounty] {
            assert!(!root.join(".catdesk").exists());
        }
        cleanup_control_resolver_fixture(&fixture);
    }

    #[test]
    fn control_root_rejects_registered_project_git_origin_drift() {
        let fixture =
            std::env::temp_dir().join(format!("catdesk-control-resolver-{}", Uuid::new_v4()));
        let control = fixture.join("catdesk");
        let driver = fixture.join("driver");
        fixture_git_root(&control, "https://example.test/catdesk.git");
        fixture_git_root(&driver, "https://example.test/driver.git");
        let store = AutonomousProjectRegistryStoreV1::open(control.join(".catdesk/projects"))
            .expect("control registry");
        store.initialize(1, 1).expect("registry");
        for (id, root, origin) in [
            ("catdesk", &control, "https://example.test/catdesk.git"),
            ("driver", &driver, "https://example.test/driver.git"),
        ] {
            let mut registered = project(id, root.to_path_buf());
            registered.git_identity = origin.into();
            store
                .register_project(registered)
                .expect("register fixture");
        }
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&driver)
                .args([
                    "remote",
                    "set-url",
                    "origin",
                    "https://example.test/changed.git",
                ])
                .status()
                .expect("fixture Git available")
                .success()
        );
        assert!(resolve_registered_project_from_control_root(&control, "driver").is_err());
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&driver)
                .args([
                    "remote",
                    "set-url",
                    "origin",
                    "https://example.test/driver.git"
                ])
                .status()
                .expect("restore fixture Git origin")
                .success()
        );
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&control)
                .args([
                    "remote",
                    "set-url",
                    "origin",
                    "https://example.test/changed.git"
                ])
                .status()
                .expect("change control Git origin")
                .success()
        );
        assert!(resolve_registered_project_from_control_root(&control, "driver").is_err());
        assert!(!driver.join(".catdesk").exists());
        cleanup_control_resolver_fixture(&fixture);
    }
    #[test]
    fn independent_projects_schedule_concurrently_but_same_workspace_writer_is_rejected() {
        let root = std::env::temp_dir().join(format!("catdesk-projects-{}", Uuid::new_v4()));
        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir_all(&a).expect("a");
        fs::create_dir_all(&b).expect("b");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(2, 1).expect("init");
        store
            .register_project(project("alpha", a.clone()))
            .expect("alpha");
        store.register_project(project("beta", b)).expect("beta");
        assert!(store.register_project(project("alias", a)).is_err());
        let alpha = store
            .schedule_mutating_worker("alpha", "owner-a", ScheduledProviderV1::Codex, 10, 20)
            .expect("schedule");
        let alpha = match alpha {
            GlobalScheduleDecisionV1::Granted(lease) => lease,
            other => panic!("expected alpha grant, got {other:?}"),
        };
        let published = fs::read(store.lease_path(&alpha.workspace)).expect("published lease");
        assert!(matches!(
            store
                .schedule_mutating_worker("beta", "owner-b", ScheduledProviderV1::Codex, 10, 20)
                .expect("schedule"),
            GlobalScheduleDecisionV1::Granted(_)
        ));
        assert_eq!(
            store
                .schedule_mutating_worker("alpha", "owner-c", ScheduledProviderV1::Qwen, 10, 20)
                .expect("schedule"),
            GlobalScheduleDecisionV1::Rejected {
                reason: "WORKSPACE_WRITER_LEASE_HELD"
            }
        );
        assert_eq!(
            fs::read(store.lease_path(&alpha.workspace)).expect("original lease retained"),
            published
        );
        let mut wrong_owner = alpha.clone();
        wrong_owner.owner_id = "owner-c".into();
        assert!(store.release_workspace_lease(&wrong_owner).is_err());
        store
            .release_workspace_lease(&alpha)
            .expect("owner release");
    }

    #[test]
    fn round_robin_scheduler_persists_three_project_rotation_across_reopen() {
        let root = std::env::temp_dir().join(format!("catdesk-round-robin-{}", Uuid::new_v4()));
        let registry_root = root.join("registry");
        let store = AutonomousProjectRegistryStoreV1::open(&registry_root).expect("store");
        store.initialize(1, 1).expect("init");
        for id in ["alpha", "beta", "gamma"] {
            let workspace = root.join(id);
            fs::create_dir_all(&workspace).expect("workspace");
            store
                .register_project(project(id, workspace))
                .expect("register");
            store
                .bind_codex_thread(id, &format!("thread-{id}"))
                .expect("bind thread");
        }
        let expected = ["alpha", "beta", "gamma", "alpha"];
        for (turn, expected_id) in expected.iter().enumerate() {
            let reopened = AutonomousProjectRegistryStoreV1::open(&registry_root).expect("reopen");
            let now = 10 + turn as u64 * 10;
            let granted = reopened
                .schedule_next_ready_worker(
                    &["gamma", "alpha", "beta"],
                    &format!("owner-{turn}"),
                    ScheduledProviderV1::Codex,
                    now,
                    now + 5,
                )
                .expect("next ready project");
            let lease = match granted {
                GlobalScheduleDecisionV1::Granted(lease) => lease,
                other => panic!("expected a grant, got {other:?}"),
            };
            assert_eq!(lease.project_id, *expected_id);
            assert_eq!(
                lease.workspace,
                root.join(expected_id).canonicalize().expect("root")
            );
            assert_eq!(lease.owner_id, format!("owner-{turn}"));
            reopened.release_workspace_lease(&lease).expect("release");
        }
        let registry = store.load_registry().expect("registry");
        for project in registry.projects {
            assert_eq!(
                project.codex_thread_id.as_deref(),
                Some(format!("thread-{}", project.project_id).as_str())
            );
        }
    }

    #[test]
    fn round_robin_scheduler_rejects_wrong_registration_and_skips_busy_project() {
        let root = std::env::temp_dir().join(format!("catdesk-ready-{}", Uuid::new_v4()));
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(2, 1).expect("init");
        for id in ["alpha", "beta", "gamma"] {
            let workspace = root.join(id);
            fs::create_dir_all(&workspace).expect("workspace");
            store
                .register_project(project(id, workspace))
                .expect("register");
        }
        for ready in [&["alpha", "missing"][..], &["alpha", "alpha"][..]] {
            assert!(
                store
                    .schedule_next_ready_worker(
                        ready,
                        "owner-invalid",
                        ScheduledProviderV1::Codex,
                        10,
                        20,
                    )
                    .is_err()
            );
        }
        assert!(!store.round_robin_cursor_path().exists());
        assert!(
            fs::read_dir(root.join("registry/leases"))
                .expect("leases")
                .next()
                .is_none()
        );

        let beta = match store
            .schedule_next_ready_worker(
                &["beta", "gamma"],
                "owner-beta",
                ScheduledProviderV1::Codex,
                10,
                20,
            )
            .expect("beta grant")
        {
            GlobalScheduleDecisionV1::Granted(lease) => lease,
            other => panic!("expected beta grant, got {other:?}"),
        };
        assert_eq!(beta.project_id, "beta");
        let gamma = match store
            .schedule_next_ready_worker(
                &["alpha", "beta", "gamma"],
                "owner-gamma",
                ScheduledProviderV1::Codex,
                11,
                21,
            )
            .expect("gamma grant")
        {
            GlobalScheduleDecisionV1::Granted(lease) => lease,
            other => panic!("expected gamma grant, got {other:?}"),
        };
        assert_eq!(gamma.project_id, "gamma");
        store
            .release_workspace_lease(&gamma)
            .expect("release gamma");
        let alpha = match store
            .schedule_next_ready_worker(
                &["alpha", "beta", "gamma"],
                "owner-alpha",
                ScheduledProviderV1::Codex,
                12,
                22,
            )
            .expect("alpha grant")
        {
            GlobalScheduleDecisionV1::Granted(lease) => lease,
            other => panic!("expected alpha grant, got {other:?}"),
        };
        assert_eq!(alpha.project_id, "alpha");
        assert_eq!(
            store
                .schedule_next_ready_worker(
                    &["alpha", "beta"],
                    "owner-blocked",
                    ScheduledProviderV1::Codex,
                    13,
                    23,
                )
                .expect("capacity response"),
            GlobalScheduleDecisionV1::Rejected {
                reason: "NO_READY_WORKSPACE_AVAILABLE"
            }
        );
        store
            .release_workspace_lease(&alpha)
            .expect("release alpha");
        store.release_workspace_lease(&beta).expect("release beta");
    }

    #[test]
    fn round_robin_scheduler_keeps_expired_lease_until_explicit_release() {
        let root = std::env::temp_dir().join(format!("catdesk-ready-expiry-{}", Uuid::new_v4()));
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(1, 1).expect("init");
        for id in ["alpha", "beta", "gamma"] {
            let workspace = root.join(id);
            fs::create_dir_all(&workspace).expect("workspace");
            store
                .register_project(project(id, workspace))
                .expect("register");
        }
        let alpha = match store
            .schedule_next_ready_worker(
                &["alpha", "beta", "gamma"],
                "owner-alpha",
                ScheduledProviderV1::Codex,
                10,
                11,
            )
            .expect("alpha grant")
        {
            GlobalScheduleDecisionV1::Granted(lease) => lease,
            other => panic!("expected alpha grant, got {other:?}"),
        };
        assert_eq!(
            store
                .schedule_next_ready_worker(
                    &["alpha", "beta", "gamma"],
                    "owner-beta",
                    ScheduledProviderV1::Codex,
                    12,
                    22,
                )
                .expect("held lease response"),
            GlobalScheduleDecisionV1::Rejected {
                reason: "GLOBAL_PROVIDER_CAPACITY_REACHED"
            }
        );
        assert!(store.lease_path(&alpha.workspace).exists());
        store
            .release_workspace_lease(&alpha)
            .expect("owner release after worker stop");
        let beta = match store
            .schedule_next_ready_worker(
                &["alpha", "beta", "gamma"],
                "owner-beta",
                ScheduledProviderV1::Codex,
                13,
                23,
            )
            .expect("beta grant after owner release")
        {
            GlobalScheduleDecisionV1::Granted(lease) => lease,
            other => panic!("expected beta grant, got {other:?}"),
        };
        assert_eq!(alpha.project_id, "alpha");
        assert_eq!(beta.project_id, "beta");
        store.release_workspace_lease(&beta).expect("release beta");
    }

    #[test]
    fn partial_published_lease_blocks_grants_but_orphan_draft_does_not() {
        let root = std::env::temp_dir().join(format!("catdesk-partial-lease-{}", Uuid::new_v4()));
        let a = root.join("alpha");
        let b = root.join("beta");
        fs::create_dir_all(&a).expect("alpha");
        fs::create_dir_all(&b).expect("beta");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(2, 1).expect("init");
        store.register_project(project("alpha", a)).expect("alpha");
        store.register_project(project("beta", b)).expect("beta");

        // A crash before the no-replace link may leave an incomplete draft.
        // Drafts outside leases are inert and cannot claim a workspace.
        let draft = root.join("registry/.lease-draft-orphan.tmp");
        fs::write(&draft, b"{\"projectId\":").expect("partial draft");
        let alpha = match store
            .schedule_mutating_worker("alpha", "owner-a", ScheduledProviderV1::Codex, 10, 20)
            .expect("grant despite orphan draft")
        {
            GlobalScheduleDecisionV1::Granted(lease) => lease,
            other => panic!("expected alpha grant, got {other:?}"),
        };
        store
            .release_workspace_lease(&alpha)
            .expect("release alpha");

        // A partial final lease from an older writer is uncertain authority.
        // It must not be deleted or skipped to make progress elsewhere.
        let partial_path = store.lease_path(&alpha.workspace);
        let partial = b"{\"projectId\":\"alpha\",";
        fs::write(&partial_path, partial).expect("partial final lease");
        assert!(
            store
                .schedule_mutating_worker("beta", "owner-b", ScheduledProviderV1::Codex, 21, 30)
                .is_err()
        );
        assert_eq!(
            fs::read(&partial_path).expect("retained partial lease"),
            partial
        );
        assert!(
            !store
                .lease_path(&root.join("beta").canonicalize().expect("beta root"))
                .exists()
        );
    }

    #[test]
    fn project_execution_binding_rejects_registered_sibling_workspace() {
        let root = std::env::temp_dir().join(format!("catdesk-projects-{}", Uuid::new_v4()));
        let alpha_workspace = root.join("alpha");
        let beta_workspace = root.join("beta");
        fs::create_dir_all(&alpha_workspace).expect("alpha workspace");
        fs::create_dir_all(&beta_workspace).expect("beta workspace");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(2, 1).expect("init");
        store
            .register_project(project("alpha", alpha_workspace.clone()))
            .expect("alpha");
        store
            .register_project(project("beta", beta_workspace.clone()))
            .expect("beta");

        assert_eq!(
            store
                .project_for_workspace("alpha", &alpha_workspace)
                .expect("exact alpha")
                .project_id,
            "alpha"
        );
        assert!(
            store
                .project_for_workspace("beta", &alpha_workspace)
                .is_err(),
            "a workspace-bound run cannot select a sibling project"
        );
        assert!(
            store
                .project_for_workspace("alpha", &beta_workspace)
                .is_err(),
            "a sibling workspace cannot borrow alpha's identity"
        );
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn thread_bindings_and_expired_leases_require_owner_release() {
        let root = std::env::temp_dir().join(format!("catdesk-projects-{}", Uuid::new_v4()));
        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir_all(&a).expect("a");
        fs::create_dir_all(&b).expect("b");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(1, 1).expect("init");
        store.register_project(project("alpha", a)).expect("alpha");
        store.register_project(project("beta", b)).expect("beta");
        store.bind_codex_thread("alpha", "thread-a").expect("bind");
        assert!(store.bind_codex_thread("beta", "thread-a").is_err());
        let lease = match store
            .schedule_mutating_worker("alpha", "owner-a", ScheduledProviderV1::Codex, 10, 11)
            .expect("lease")
        {
            GlobalScheduleDecisionV1::Granted(lease) => lease,
            _ => panic!("grant"),
        };
        assert_eq!(
            store
                .schedule_mutating_worker("alpha", "owner-b", ScheduledProviderV1::Codex, 12, 20)
                .expect("held expired lease"),
            GlobalScheduleDecisionV1::Rejected {
                reason: "WORKSPACE_WRITER_LEASE_HELD"
            }
        );
        store
            .release_workspace_lease(&lease)
            .expect("explicit owner release");
        assert!(matches!(
            store
                .schedule_mutating_worker("alpha", "owner-b", ScheduledProviderV1::Codex, 13, 20)
                .expect("grant after release"),
            GlobalScheduleDecisionV1::Granted(_)
        ));
    }

    fn registration_evidence(workspace: PathBuf, thread: &str) -> ProjectRegistrationEvidenceV1 {
        ProjectRegistrationEvidenceV1 {
            workspace,
            git_identity: "https://example.test/external.git".into(),
            codex_thread_id: thread.into(),
            selected_model: "gpt-5.6-terra".into(),
            reasoning_effort: "high".into(),
        }
    }

    #[test]
    fn sol_thread_rotation_requires_exact_old_binding_and_project_identity() {
        let root = std::env::temp_dir().join(format!("catdesk-rotate-{}", Uuid::new_v4()));
        let alpha = root.join("alpha");
        let beta = root.join("beta");
        fs::create_dir_all(&alpha).expect("alpha");
        fs::create_dir_all(&beta).expect("beta");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(2, 1).expect("init");
        store
            .register_project(project("alpha", alpha.clone()))
            .expect("alpha project");
        store
            .register_project(project("beta", beta.clone()))
            .expect("beta project");
        store
            .bind_codex_thread("alpha", "old-alpha")
            .expect("old binding");
        store
            .bind_codex_thread("beta", "other-beta")
            .expect("sibling binding");
        let mut evidence = ProjectRegistrationEvidenceV1 {
            workspace: alpha.canonicalize().expect("canonical alpha"),
            git_identity: "fixture-alpha".into(),
            codex_thread_id: "new-sol".into(),
            selected_model: "gpt-6.1-sol".into(),
            reasoning_effort: "medium".into(),
        };
        assert!(
            store
                .rotate_codex_thread("alpha", "wrong-old", &evidence)
                .is_err()
        );
        let mut wrong_workspace = evidence.clone();
        wrong_workspace.workspace = beta.canonicalize().expect("canonical beta");
        assert!(
            store
                .rotate_codex_thread("alpha", "old-alpha", &wrong_workspace)
                .is_err()
        );
        let mut wrong_profile = evidence.clone();
        wrong_profile.reasoning_effort = "high".into();
        assert!(
            store
                .rotate_codex_thread("alpha", "old-alpha", &wrong_profile)
                .is_err()
        );
        evidence.codex_thread_id = "other-beta".into();
        assert!(
            store
                .rotate_codex_thread("alpha", "old-alpha", &evidence)
                .is_err()
        );
        evidence.codex_thread_id = "new-sol".into();
        let rotated = store
            .rotate_codex_thread("alpha", "old-alpha", &evidence)
            .expect("rotate");
        assert_eq!(rotated.codex_thread_id.as_deref(), Some("new-sol"));
        assert!(
            store
                .rotate_codex_thread("alpha", "old-alpha", &evidence)
                .is_err()
        );
        let registry = store.load_registry().expect("registry");
        assert_eq!(
            registry.projects[1].codex_thread_id.as_deref(),
            Some("other-beta")
        );
    }

    #[test]
    fn rotation_preflight_is_opaque_expiring_and_separate_from_adoption() {
        let root = std::env::temp_dir().join(format!("catdesk-rotate-confirm-{}", Uuid::new_v4()));
        let workspace = root.join("alpha");
        fs::create_dir_all(&workspace).expect("workspace");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(1, 1).expect("init");
        store
            .register_project(project("alpha", workspace.clone()))
            .expect("project");
        store.bind_codex_thread("alpha", "old-alpha").expect("old");
        let evidence = ProjectRegistrationEvidenceV1 {
            workspace: workspace.canonicalize().expect("canonical"),
            git_identity: "fixture-alpha".into(),
            codex_thread_id: "new-sol".into(),
            selected_model: "gpt-6.1-sol".into(),
            reasoning_effort: "medium".into(),
        };
        let preflight = store
            .preflight_thread_rotation(
                "alpha",
                &workspace,
                vec![(Some("Sol thread".into()), evidence.clone())],
                10,
            )
            .expect("preflight");
        let handle = &preflight.candidates[0].candidate_handle;
        assert_eq!(
            preflight.expected_previous_thread_id.as_deref(),
            Some("old-alpha")
        );
        assert_ne!(handle, &evidence.codex_thread_id);
        assert!(
            store
                .confirm_thread_adoption(&preflight.confirmation_token, handle, &evidence, 11,)
                .is_err()
        );
        assert!(
            store
                .confirm_thread_rotation(
                    &preflight.confirmation_token,
                    handle,
                    &evidence,
                    131,
                    || Ok(()),
                )
                .is_err()
        );
        let mut called = false;
        assert!(
            store
                .confirm_thread_rotation(
                    &preflight.confirmation_token,
                    handle,
                    &evidence,
                    12,
                    || {
                        called = true;
                        Err(RuntimeError::Validation("active session".into()))
                    },
                )
                .is_err()
        );
        assert!(called);
        assert_eq!(
            store.load_registry().expect("registry").projects[0]
                .codex_thread_id
                .as_deref(),
            Some("old-alpha")
        );
        let rotated = store
            .confirm_thread_rotation(&preflight.confirmation_token, handle, &evidence, 13, || {
                Ok(())
            })
            .expect("confirm");
        assert_eq!(rotated.codex_thread_id.as_deref(), Some("new-sol"));
        let replay = store
            .confirm_thread_rotation(
                &preflight.confirmation_token,
                handle,
                &evidence,
                200,
                || Ok(()),
            )
            .expect("exact replay");
        assert_eq!(replay.codex_thread_id.as_deref(), Some("new-sol"));
    }

    #[test]
    fn transition_lock_rejects_overlap_and_releases_with_file_handle() {
        let root = std::env::temp_dir().join(format!("catdesk-transition-lock-{}", Uuid::new_v4()));
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store
            .with_thread_transition_lock(|| {
                assert!(store.with_thread_transition_lock(|| Ok(())).is_err());
                Ok(())
            })
            .expect("first lock");
        store
            .with_thread_transition_lock(|| Ok(()))
            .expect("released lock");
    }

    #[test]
    fn existing_thread_adoption_is_opaque_atomic_expiring_and_idempotent() {
        let root = std::env::temp_dir().join(format!("catdesk-adopt-{}", Uuid::new_v4()));
        let workspace = root.join("external");
        fs::create_dir_all(&workspace).expect("workspace");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(1, 1).expect("init");
        store
            .register_project(project("external", workspace.clone()))
            .expect("project");
        let canonical = workspace.canonicalize().expect("canonical");
        let evidence = ProjectRegistrationEvidenceV1 {
            workspace: canonical.clone(),
            git_identity: "fixture-external".into(),
            codex_thread_id: "thread-existing".into(),
            selected_model: "gpt-5.6-terra".into(),
            reasoning_effort: "high".into(),
        };
        let preflight = store
            .preflight_thread_adoption(
                "external",
                &canonical,
                vec![(Some("Visible Codex thread".into()), evidence.clone())],
                10,
            )
            .expect("preflight");
        let candidate = &preflight.candidates[0];
        assert_ne!(candidate.candidate_handle, evidence.codex_thread_id);
        assert_eq!(candidate.thread_fingerprint.len(), 71);
        assert!(
            store.load_registry().expect("registry").projects[0]
                .codex_thread_id
                .is_none()
        );
        assert!(
            store
                .confirm_thread_adoption(
                    &preflight.confirmation_token,
                    "candidate-bad",
                    &evidence,
                    11
                )
                .is_err()
        );
        let bound = store
            .confirm_thread_adoption(
                &preflight.confirmation_token,
                &candidate.candidate_handle,
                &evidence,
                11,
            )
            .expect("confirm");
        assert_eq!(bound.codex_thread_id.as_deref(), Some("thread-existing"));
        assert_eq!(
            store
                .confirm_thread_adoption(
                    &preflight.confirmation_token,
                    &candidate.candidate_handle,
                    &evidence,
                    999
                )
                .expect("replay"),
            bound
        );
        assert!(
            store
                .preflight_thread_adoption(
                    "external",
                    &canonical,
                    vec![(None, evidence.clone())],
                    12
                )
                .is_err()
        );

        let second = root.join("second");
        fs::create_dir_all(&second).expect("second");
        store
            .register_project(project("second", second.clone()))
            .expect("second project");
        let expiry = ProjectRegistrationEvidenceV1 {
            workspace: second.canonicalize().expect("canonical"),
            git_identity: "fixture-second".into(),
            codex_thread_id: "thread-expired".into(),
            selected_model: "gpt-5.6-terra".into(),
            reasoning_effort: "high".into(),
        };
        let expired = store
            .preflight_thread_adoption(
                "second",
                &expiry.workspace,
                vec![(None, expiry.clone())],
                10,
            )
            .expect("expiry preflight");
        assert!(
            store
                .confirm_thread_adoption(
                    &expired.confirmation_token,
                    &expired.candidates[0].candidate_handle,
                    &expiry,
                    200
                )
                .is_err()
        );
        assert!(
            store
                .load_registry()
                .expect("registry")
                .projects
                .iter()
                .find(|p| p.project_id == "second")
                .expect("second")
                .codex_thread_id
                .is_none()
        );
    }

    #[test]
    fn external_registration_confirm_is_atomic_idempotent_and_conflict_safe() {
        let root =
            std::env::temp_dir().join(format!("catdesk-project-register-{}", Uuid::new_v4()));
        let workspace = root.join("external");
        fs::create_dir_all(&workspace).expect("workspace");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(1, 1).expect("init");
        let evidence = registration_evidence(workspace, "thread-external");
        let preflight = store
            .preflight_project_registration("external", "rust_full", evidence.clone(), 10)
            .expect("preflight");
        assert!(store.load_registry().expect("registry").projects.is_empty());
        let project = store
            .confirm_project_registration(&preflight.confirmation_token, &evidence, 11)
            .expect("confirm");
        assert_eq!(project.codex_thread_id.as_deref(), Some("thread-external"));
        assert_eq!(
            store
                .confirm_project_registration(&preflight.confirmation_token, &evidence, 999)
                .expect("replay"),
            project
        );
        let conflicting = registration_evidence(project.workspace.clone(), "thread-other");
        assert!(
            store
                .preflight_project_registration("other", "rust_full", conflicting, 12)
                .is_err()
        );
    }

    #[test]
    fn registration_expiry_drift_and_wrong_terra_evidence_leave_registry_empty() {
        let root = std::env::temp_dir().join(format!("catdesk-project-expiry-{}", Uuid::new_v4()));
        let workspace = root.join("external");
        fs::create_dir_all(&workspace).expect("workspace");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(1, 1).expect("init");
        let evidence = registration_evidence(workspace, "thread-external");
        let preflight = store
            .preflight_project_registration("external", "rust_full", evidence.clone(), 10)
            .expect("preflight");
        let mut drift = evidence.clone();
        drift.git_identity = "https://example.test/drift.git".into();
        assert!(
            store
                .confirm_project_registration(&preflight.confirmation_token, &drift, 11)
                .is_err()
        );
        assert!(
            store
                .confirm_project_registration(&preflight.confirmation_token, &evidence, 200)
                .is_err()
        );
        assert!(store.load_registry().expect("registry").projects.is_empty());
        let mut wrong = evidence;
        wrong.selected_model = "gpt-5.6".into();
        assert!(
            store
                .preflight_project_registration("wrong", "rust_full", wrong, 10)
                .is_err()
        );
    }

    #[test]
    fn registration_accepts_only_exact_sol_medium_evidence() {
        let root = std::env::temp_dir().join(format!("catdesk-project-sol-{}", Uuid::new_v4()));
        let workspace = root.join("external");
        fs::create_dir_all(&workspace).expect("workspace");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(1, 1).expect("init");
        let mut evidence = registration_evidence(workspace, "thread-sol");
        evidence.selected_model = "gpt-6.1-sol".into();
        evidence.reasoning_effort = "high".into();
        assert!(
            store
                .preflight_project_registration("external", "rust_full", evidence.clone(), 10)
                .is_err()
        );
        evidence.reasoning_effort = "medium".into();
        let preflight = store
            .preflight_project_registration("external", "rust_full", evidence.clone(), 10)
            .expect("Sol/medium preflight");
        let mut drift = evidence.clone();
        drift.reasoning_effort = "high".into();
        assert!(
            store
                .confirm_project_registration(&preflight.confirmation_token, &drift, 11)
                .is_err()
        );
        let bound = store
            .confirm_project_registration(&preflight.confirmation_token, &evidence, 12)
            .expect("Sol/medium confirmation");
        assert_eq!(bound.codex_thread_id.as_deref(), Some("thread-sol"));
    }

    #[test]
    fn project_chat_target_binding_is_cas_safe_and_accepts_project_conversations() {
        let root = std::env::temp_dir().join(format!("catdesk-project-target-{}", Uuid::new_v4()));
        let workspace = root.join("external");
        fs::create_dir_all(&workspace).expect("workspace");
        let store = AutonomousProjectRegistryStoreV1::open(root.join("registry")).expect("store");
        store.initialize(1, 1).expect("init");
        let evidence = registration_evidence(workspace, "thread-external");
        let preflight = store
            .preflight_project_registration("external", "rust_full", evidence.clone(), 10)
            .expect("preflight");
        store
            .confirm_project_registration(&preflight.confirmation_token, &evidence, 11)
            .expect("confirm");
        let bound = store
            .bind_project_chat_target(
                "external",
                "https://chatgpt.com/g/project-1/c/thread-1",
                None,
            )
            .expect("bind");
        assert_eq!(
            bound.chatgpt_target_url.as_deref(),
            Some("https://chatgpt.com/g/project-1/c/thread-1")
        );
        assert!(
            store
                .bind_project_chat_target("external", "https://chatgpt.com/c/other", None)
                .is_err()
        );
        let digest = bound.chatgpt_target_sha256.expect("digest");
        assert!(
            store
                .bind_project_chat_target("external", "https://chatgpt.com/c/other", Some(&digest))
                .is_ok()
        );
        let web_target = "https://chatgpt.com/c/WEB:1229263e-88d1-41a1-8ce2-b4aa97fbcb0f";
        assert_eq!(
            canonical_project_chat_target(web_target).expect("WEB-prefixed conversation target"),
            web_target
        );
        for invalid in [
            "https://chatgpt.com/c/WEB:",
            "https://chatgpt.com/c/web:thread-1",
            "https://chatgpt.com/c/WEB:thread:extra",
            "https://chatgpt.com/g/WEB:project/c/thread",
            "https://chatgpt.com/g/project/c/thread/extra",
            "https://chatgpt.com/g/project/c/thread?q=1",
            "https://evil.example/c/thread",
        ] {
            assert!(canonical_project_chat_target(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn scheduler_rejects_oversized_registry_cursor_and_lease_without_changing_them() {
        for kind in ["registry", "cursor", "lease"] {
            let (_root, store, workspace) = scheduler_preflight_fixture(kind);
            let path = match kind {
                "registry" => store.registry_path(),
                "cursor" => store.round_robin_cursor_path(),
                "lease" => store.lease_path(&workspace),
                _ => unreachable!(),
            };
            let limit = match kind {
                "registry" => MAX_PROJECT_REGISTRY_BYTES,
                "cursor" => MAX_SCHEDULER_CURSOR_BYTES,
                "lease" => MAX_SCHEDULER_LEASE_BYTES,
                _ => unreachable!(),
            };
            let oversized = vec![b'x'; (limit + 1) as usize];
            fs::write(&path, &oversized).expect("oversized fixture state");
            let result = if kind == "cursor" {
                store.schedule_next_ready_worker(
                    &["alpha"],
                    "owner",
                    ScheduledProviderV1::Codex,
                    10,
                    20,
                )
            } else {
                store.schedule_mutating_worker("alpha", "owner", ScheduledProviderV1::Codex, 10, 20)
            };
            assert!(result.is_err(), "oversized {kind} must block scheduling");
            assert_eq!(fs::read(&path).expect("retained state"), oversized);
            if kind != "lease" {
                assert!(!store.lease_path(&workspace).exists());
            }
        }
    }

    #[test]
    fn scheduler_rejects_nonempty_lock_and_redirected_directory_types() {
        let (_root, store, workspace) = scheduler_preflight_fixture("lock");
        let lock = store.root.join("scheduler.lock");
        fs::write(&lock, b"unexpected lock payload").expect("lock fixture");
        assert!(
            store
                .schedule_mutating_worker("alpha", "owner", ScheduledProviderV1::Codex, 10, 20)
                .is_err()
        );
        assert_eq!(
            fs::read(&lock).expect("retained lock"),
            b"unexpected lock payload"
        );
        assert!(!store.lease_path(&workspace).exists());

        let root = std::env::temp_dir().join(format!(
            "catdesk-scheduler-preflight-wrong-type-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("fixture parent");
        let wrong_root = root.join("registry");
        fs::write(&wrong_root, b"not a directory").expect("wrong root type");
        assert!(AutonomousProjectRegistryStoreV1::open(&wrong_root).is_err());
        assert_eq!(
            fs::read(&wrong_root).expect("retained wrong root"),
            b"not a directory"
        );
        let registry = root.join("registry-directory");
        fs::create_dir_all(&registry).expect("registry parent");
        let wrong_leases = registry.join("leases");
        fs::write(&wrong_leases, b"not a directory").expect("wrong leases type");
        assert!(AutonomousProjectRegistryStoreV1::open(&registry).is_err());
        assert_eq!(
            fs::read(&wrong_leases).expect("retained wrong leases"),
            b"not a directory"
        );
    }

    #[test]
    fn scheduler_rejects_malformed_cursor_without_replacing_it() {
        let (_root, store, workspace) = scheduler_preflight_fixture("malformed-cursor");
        let cursor = store.round_robin_cursor_path();
        let malformed = b"{\"schemaVersion\":1,\"lastGrantedProjectId\":";
        fs::write(&cursor, malformed).expect("malformed cursor fixture");
        assert!(
            store
                .schedule_next_ready_worker(
                    &["alpha"],
                    "owner",
                    ScheduledProviderV1::Codex,
                    10,
                    20,
                )
                .is_err()
        );
        assert!(
            store
                .schedule_mutating_worker("alpha", "owner", ScheduledProviderV1::Codex, 10, 20)
                .is_err()
        );
        assert_eq!(fs::read(&cursor).expect("retained cursor"), malformed);
        assert!(!store.lease_path(&workspace).exists());
    }

    #[test]
    fn scheduler_json_reader_stops_at_the_limit_even_if_open_file_is_larger() {
        let (root, _store, _workspace) = scheduler_preflight_fixture("bounded-read");
        let path = root.join("grown-after-open.json");
        fs::write(&path, vec![b' '; 4097]).expect("oversized file");
        let file = File::open(&path).expect("open before bounded read");
        assert!(read_bounded_registry_bytes(file, 4096, "bounded read").is_err());
    }

    #[test]
    fn registry_writer_refuses_a_record_that_its_reader_would_reject() {
        let (_root, store, _workspace) = scheduler_preflight_fixture("registry-write-limit");
        let before = fs::read(store.registry_path()).expect("existing registry");
        let mut oversized = store.load_registry().expect("registered project");
        oversized.projects[0].workspace = PathBuf::from("x".repeat(1_100_000));
        assert!(store.save_registry(&oversized).is_err());
        assert_eq!(
            fs::read(store.registry_path()).expect("retained registry"),
            before
        );
    }

    #[test]
    fn initialization_does_not_silently_accept_corrupt_existing_registry() {
        let (_root, store, _workspace) = scheduler_preflight_fixture("corrupt-initialize");
        let malformed = b"{\"schemaVersion\":1,";
        fs::write(store.registry_path(), malformed).expect("malformed registry");
        assert!(store.initialize(1, 1).is_err());
        assert_eq!(
            fs::read(store.registry_path()).expect("retained registry"),
            malformed
        );
    }

    #[test]
    fn operator_supplied_registry_read_only_compatibility_probe() {
        let Some(root) = std::env::var_os("CATDESK_PROJECT_REGISTRY_READ_ONLY_PROBE") else {
            return;
        };
        let store = AutonomousProjectRegistryStoreV1::open_read_only(PathBuf::from(root))
            .expect("existing registry opens without creating state");
        let registry = store.load_registry().expect("existing registry validates");
        let cursor = store
            .load_round_robin_cursor()
            .expect("existing scheduler cursor validates");
        if let Some(cursor) = &cursor {
            assert!(
                registry
                    .projects
                    .iter()
                    .any(|project| project.project_id == cursor.last_granted_project_id)
            );
        }
        let leases = store.all_leases().expect("existing leases validate");
        println!(
            "read-only scheduler probe: projects={} leases={} cursor_present={}",
            registry.projects.len(),
            leases.len(),
            cursor.is_some()
        );
    }

    #[cfg(windows)]
    #[test]
    fn scheduler_rejects_windows_junction_root_and_leases_directory() {
        fn quote(path: &Path) -> String {
            path.to_string_lossy().replace('\'', "''")
        }
        fn junction(target: &Path, link: &Path) {
            let script = format!(
                "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
                quote(link),
                quote(target)
            );
            assert!(
                Command::new("powershell")
                    .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                    .status()
                    .expect("PowerShell junction fixture")
                    .success()
            );
        }
        let root = std::env::temp_dir().join(format!(
            "catdesk-scheduler-preflight-junction-{}",
            Uuid::new_v4()
        ));
        let outside = root.join("outside");
        fs::create_dir_all(&outside).expect("outside directory");
        let root_alias = root.join("registry-alias");
        junction(&outside, &root_alias);
        assert!(AutonomousProjectRegistryStoreV1::open(&root_alias).is_err());
        assert!(AutonomousProjectRegistryStoreV1::open_read_only(&root_alias).is_err());
        assert!(!outside.join("leases").exists());

        let registry = root.join("registry");
        fs::create_dir_all(&registry).expect("registry directory");
        let leases_alias = registry.join("leases");
        junction(&outside, &leases_alias);
        assert!(AutonomousProjectRegistryStoreV1::open(&registry).is_err());
        assert!(!outside.join("scheduler.lock").exists());
    }

    #[test]
    fn scheduler_rejects_redirected_root_cursor_lock_and_lease() {
        let (root, store, workspace) = scheduler_preflight_fixture("redirected");
        let outside = root.join("outside");
        fs::create_dir_all(&outside).expect("outside fixture");
        let alias = root.join("registry-alias");
        if !symlink_available(symlink_dir(&store.root, &alias)) {
            return;
        }
        assert!(AutonomousProjectRegistryStoreV1::open(&alias).is_err());
        assert!(AutonomousProjectRegistryStoreV1::open_read_only(&alias).is_err());
        assert!(!alias.join("unexpected").exists());

        let cursor_target = outside.join("cursor.json");
        let cursor_bytes = b"{\"schemaVersion\":1,\"lastGrantedProjectId\":\"alpha\"}";
        fs::write(&cursor_target, cursor_bytes).expect("cursor target");
        symlink_file(&cursor_target, &store.round_robin_cursor_path()).expect("cursor symlink");
        assert!(
            store
                .schedule_next_ready_worker(
                    &["alpha"],
                    "owner",
                    ScheduledProviderV1::Codex,
                    10,
                    20,
                )
                .is_err()
        );
        assert_eq!(
            fs::read(&cursor_target).expect("cursor retained"),
            cursor_bytes
        );
        assert!(!store.lease_path(&workspace).exists());

        // The cursor is only the first obstruction. Keep it intact and use
        // separate fresh stores for the other redirected entries.
        for kind in ["lock", "lease"] {
            let (root, store, workspace) = scheduler_preflight_fixture(kind);
            let outside = root.join("outside.json");
            let path = if kind == "lock" {
                fs::write(&outside, b"").expect("lock target");
                store.root.join("scheduler.lock")
            } else {
                let lease = AutonomousWorkspaceLeaseV1 {
                    project_id: "alpha".into(),
                    workspace: workspace.clone(),
                    owner_id: "other-owner".into(),
                    provider: ScheduledProviderV1::Codex,
                    expires_at_unix: 20,
                };
                fs::write(&outside, serde_json::to_vec(&lease).expect("lease JSON"))
                    .expect("lease target");
                store.lease_path(&workspace)
            };
            let before = fs::read(&outside).expect("outside before");
            symlink_file(&outside, &path).expect("scheduler state symlink");
            assert!(
                store
                    .schedule_mutating_worker("alpha", "owner", ScheduledProviderV1::Codex, 10, 20,)
                    .is_err(),
                "redirected {kind} must block scheduling"
            );
            assert_eq!(fs::read(&outside).expect("outside retained"), before);
        }
    }
}
