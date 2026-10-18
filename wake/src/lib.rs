//! Independent Wake protocol and durable state. No CatDesk schemas or processes.
#[cfg(windows)]
pub mod development_runtime;
pub mod install_authority;
pub mod install_broker_approval;
#[cfg(windows)]
pub mod install_broker_candidate_stage;
#[cfg(windows)]
pub(crate) mod install_broker_ci_fetch;
pub mod install_broker_ci_witness;
#[cfg(windows)]
pub mod install_broker_client;
#[cfg(windows)]
pub mod install_broker_current_observer;
#[cfg(windows)]
pub(crate) mod install_broker_durability;
#[cfg(windows)]
pub mod install_broker_endpoint;
#[cfg(windows)]
pub mod install_broker_ledger;
#[cfg(windows)]
pub mod install_broker_legacy_owner;
#[cfg(windows)]
pub mod install_broker_offline_pins;
pub mod install_broker_offline_v2;
pub mod install_broker_operator;
#[cfg(windows)]
pub(crate) mod install_broker_pipe_io;
#[cfg(windows)]
pub mod install_broker_pointer_writer;
#[cfg(windows)]
pub mod install_broker_policy;
pub mod install_broker_preflight;
#[cfg(windows)]
pub mod install_broker_reader;
#[cfg(windows)]
pub mod install_broker_request;
#[cfg(windows)]
pub mod install_broker_service_core;
#[cfg(windows)]
pub mod install_broker_service_runtime;
#[cfg(windows)]
pub(crate) mod install_broker_source;
#[cfg(windows)]
pub mod install_broker_stage_writer;
pub mod process_job;
#[cfg(not(windows))]
pub mod development_runtime {
    use crate::Result;
    use serde_json::Value;
    use std::path::{Path, PathBuf};

    pub fn prepare(_: &Path, _: &Path, _: &Path) -> Result<Value> {
        Err("DEV_RUNTIME_PLATFORM_UNSUPPORTED".into())
    }
    pub fn root_for_workspace(_: &Path) -> Result<Option<PathBuf>> {
        Err("DEV_RUNTIME_PLATFORM_UNSUPPORTED".into())
    }
    pub fn status(_: &Path) -> Result<Value> {
        Err("DEV_RUNTIME_PLATFORM_UNSUPPORTED".into())
    }
    pub fn start(_: &Path) -> Result<Value> {
        Err("DEV_RUNTIME_PLATFORM_UNSUPPORTED".into())
    }
    pub fn stop(_: &Path) -> Result<Value> {
        Err("DEV_RUNTIME_PLATFORM_UNSUPPORTED".into())
    }
    pub fn run_foreground(_: &Path) -> Result<Value> {
        Err("DEV_RUNTIME_PLATFORM_UNSUPPORTED".into())
    }
    pub fn run_host(_: &Path) -> Result<()> {
        Err("DEV_RUNTIME_PLATFORM_UNSUPPORTED".into())
    }
    pub fn validate_target_isolation(_: &Path, _: &str) -> Result<()> {
        Err("DEV_RUNTIME_PLATFORM_UNSUPPORTED".into())
    }
    pub fn validate_selected_target_isolation(_: &Path, _: &Path, _: &str) -> Result<()> {
        Err("DEV_RUNTIME_PLATFORM_UNSUPPORTED".into())
    }
    pub fn validate_target_isolation_for_store(_: &Path, _: &str) -> Result<()> {
        Err("DEV_RUNTIME_PLATFORM_UNSUPPORTED".into())
    }
}
pub mod protocol;
pub mod review_binding;
pub mod runtime;
pub mod store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL_VERSION: u32 = 1;
pub type Result<T> = std::result::Result<T, String>;
