//! Generic durable registry and scheduler guard for independent projects.
//!
//! It is deliberately a control-plane module: project workers continue to
//! own their project-local state stores. The registry only coordinates
//! workspace ownership, provider limits, and project/thread identity.

use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::runtime::RuntimeError;

pub const PROJECT_REGISTRY_SCHEMA_VERSION: u32 = 1;

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

#[derive(Clone, Debug)]
pub struct AutonomousProjectRegistryStoreV1 {
    root: PathBuf,
}

impl AutonomousProjectRegistryStoreV1 {
    /// Opens an existing registry root without creating any children.  Views
    /// such as the native operator GUI use this form so a status/readback
    /// refresh cannot initialize project authority as a side effect.
    pub fn open_read_only(root: impl AsRef<Path>) -> Result<Self, RuntimeError> {
        let root = root.as_ref().to_path_buf();
        let metadata = fs::symlink_metadata(&root).map_err(io_error)?;
        #[cfg(windows)]
        let is_reparse = metadata.file_attributes() & 0x400 != 0;
        #[cfg(not(windows))]
        let is_reparse = false;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse {
            return Err(RuntimeError::Validation(
                "project registry root is unavailable for read-only access".into(),
            ));
        }
        Ok(Self { root })
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self, RuntimeError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("leases")).map_err(io_error)?;
        fs::create_dir_all(root.join("registration-confirmations")).map_err(io_error)?;
        fs::create_dir_all(root.join("thread-adoptions")).map_err(io_error)?;
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
        if path.exists() {
            return Ok(());
        }
        self.save_registry(&AutonomousProjectRegistryV1 {
            schema_version: PROJECT_REGISTRY_SCHEMA_VERSION,
            maximum_codex_workers,
            maximum_qwen_workers,
            projects: Vec::new(),
        })
    }

    pub fn load_registry(&self) -> Result<AutonomousProjectRegistryV1, RuntimeError> {
        let file = OpenOptions::new()
            .read(true)
            .open(self.registry_path())
            .map_err(io_error)?;
        let registry: AutonomousProjectRegistryV1 =
            serde_json::from_reader(file).map_err(|error| {
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
        mut candidates: Vec<(Option<String>, ProjectRegistrationEvidenceV1)>,
        now_unix: u64,
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
        if project.workspace != workspace || project.codex_thread_id.is_some() {
            return Err(RuntimeError::Validation(
                "project cannot adopt a different Codex thread".into(),
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
    /// when it is registered under distinct project ids. Expired lease files
    /// are safely reclaimed before provider budget decisions are made.
    pub fn schedule_mutating_worker(
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
        let leases = self.live_leases(now_unix)?;
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
        let encoded = serde_json::to_vec_pretty(&lease)
            .map_err(|_| RuntimeError::Validation("lease serialization failed".into()))?;
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lease_path)
        {
            Ok(mut file) => {
                file.write_all(&encoded).map_err(io_error)?;
                file.write_all(b"\n").map_err(io_error)?;
                file.sync_all().map_err(io_error)?;
                Ok(GlobalScheduleDecisionV1::Granted(lease))
            }
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
        let path = self.lease_path(&lease.workspace);
        let file = OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(io_error)?;
        let stored: AutonomousWorkspaceLeaseV1 = serde_json::from_reader(file)
            .map_err(|_| RuntimeError::Validation("workspace lease is corrupted".into()))?;
        if &stored != lease {
            return Err(RuntimeError::Validation(
                "workspace lease ownership mismatch".into(),
            ));
        }
        fs::remove_file(path).map_err(io_error)
    }

    fn live_leases(&self, now_unix: u64) -> Result<Vec<AutonomousWorkspaceLeaseV1>, RuntimeError> {
        let mut leases = Vec::new();
        for entry in fs::read_dir(self.root.join("leases")).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let file = OpenOptions::new()
                .read(true)
                .open(&path)
                .map_err(io_error)?;
            let lease: AutonomousWorkspaceLeaseV1 = serde_json::from_reader(file)
                .map_err(|_| RuntimeError::Validation("workspace lease is corrupted".into()))?;
            if lease.expires_at_unix <= now_unix {
                fs::remove_file(path).map_err(io_error)?;
            } else {
                leases.push(lease);
            }
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
    fn lease_path(&self, workspace: &Path) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(workspace.to_string_lossy().as_bytes());
        self.root
            .join("leases")
            .join(format!("{:x}.json", hasher.finalize()))
    }
    fn save_registry(&self, registry: &AutonomousProjectRegistryV1) -> Result<(), RuntimeError> {
        validate_registry(registry)?;
        let path = self.registry_path();
        let tmp = self.root.join(format!(".projects-{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(io_error)?;
        serde_json::to_writer_pretty(&mut file, registry).map_err(|_| {
            RuntimeError::Validation("project registry serialization failed".into())
        })?;
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
        || evidence.selected_model != "gpt-5.6-terra"
        || evidence.reasoning_effort != "high"
    {
        return Err(RuntimeError::Validation(
            "registration evidence is incomplete or not Terra/High".into(),
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
        assert!(matches!(alpha, GlobalScheduleDecisionV1::Granted(_)));
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
    fn thread_bindings_and_expired_leases_are_project_scoped() {
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
        assert!(matches!(
            store
                .schedule_mutating_worker("alpha", "owner-b", ScheduledProviderV1::Codex, 12, 20)
                .expect("reclaim"),
            GlobalScheduleDecisionV1::Granted(_)
        ));
        assert!(store.release_workspace_lease(&lease).is_err());
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
}
