//! CatDesk-owned in-process registry for autonomous Codex controller ticks.
//!
//! The executable path is operator-local environment configuration. MCP can
//! name a persisted session but cannot select the executable, sandbox, or
//! authentication source.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;

use super::autonomous_contract::AutonomousPolicyEngineV1;
use super::autonomous_controller::{AutonomousControllerOutcomeV1, AutonomousControllerV1};
use super::autonomy_state::AutonomousStateStoreV1;
use super::autonomy_verifier::ContractVerifierV1;
use super::codex_cli::{CodexCliConfigV1, CodexCliProviderV1, CodexCliSandboxV1};
use super::runtime::RuntimeError;

type LiveController = AutonomousControllerV1<CodexCliProviderV1, ContractVerifierV1>;
type RuntimeRegistry = BTreeMap<String, LiveController>;

static RUNTIME_REGISTRY: OnceLock<Arc<Mutex<RuntimeRegistry>>> = OnceLock::new();

fn registry() -> &'static Arc<Mutex<RuntimeRegistry>> {
    RUNTIME_REGISTRY.get_or_init(|| Arc::new(Mutex::new(BTreeMap::new())))
}

pub async fn start_or_tick(
    workspace: &Path,
    session_id: &str,
) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let key = format!("{}:{session_id}", workspace.to_string_lossy());
    let mut registry = registry().lock().await;
    if !registry.contains_key(&key) {
        let store = AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
            .map_err(RuntimeError::from)?;
        let contract = store
            .load_contract(session_id)
            .map_err(RuntimeError::from)?;
        let policy = AutonomousPolicyEngineV1::new(contract).map_err(|error| {
            RuntimeError::Validation(format!("persisted autonomous contract rejected: {error:?}"))
        })?;
        let provider = CodexCliProviderV1::discover(operator_config(
            &workspace,
            policy.contract().provider_policy.primary_model.clone(),
        )?)
        .await?;
        let verifier = ContractVerifierV1::new(policy.clone())?;
        registry.insert(
            key.clone(),
            AutonomousControllerV1::new(policy, store, provider, verifier, session_id.into()),
        );
    }
    registry
        .get_mut(&key)
        .expect("controller was inserted")
        .run_once(now_unix())
        .await
}

pub async fn cancel_owned_turn(workspace: &Path, session_id: &str) -> Result<bool, RuntimeError> {
    let workspace = workspace.canonicalize().map_err(|_| {
        RuntimeError::Validation("autonomous workspace canonicalization failed".into())
    })?;
    let key = format!("{}:{session_id}", workspace.to_string_lossy());
    let mut registry = registry().lock().await;
    if let Some(controller) = registry.get_mut(&key) {
        controller.cancel().await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn operator_configuration_available(
    workspace: &Path,
    model_id: String,
) -> Result<(), RuntimeError> {
    operator_config(workspace, model_id).map(|_| ())
}

fn operator_config(workspace: &Path, model_id: String) -> Result<CodexCliConfigV1, RuntimeError> {
    let executable = std::env::var_os("CATDESK_CODEX_CLI_EXECUTABLE").ok_or_else(|| {
        RuntimeError::Validation(
            "operator-local CATDESK_CODEX_CLI_EXECUTABLE is required for autonomous start".into(),
        )
    })?;
    let mut config = CodexCliConfigV1::new(executable.into(), workspace.into())?;
    config.model_id = Some(model_id);
    config.sandbox = CodexCliSandboxV1::WorkspaceWrite;
    Ok(config)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
