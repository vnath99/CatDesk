#![allow(dead_code)]

pub mod advisor;
pub mod autonomous_contract;
pub mod autonomous_controller;
pub mod autonomous_qwen_tools;
pub mod autonomy_accounting;
pub mod autonomy_observability;
pub mod autonomy_projects;
pub mod autonomy_runtime;
pub mod autonomy_state;
pub mod autonomy_supervisor;
pub mod autonomy_verifier;
pub mod codex_app_server;
pub mod codex_cli;
pub mod codex_model_policy;
pub mod context;
pub mod contracts;
pub mod coordinator;
pub mod events;
pub mod fault_injection;
pub mod github_bootstrap;
pub(crate) mod github_publication;
pub(crate) mod github_publication_executor;
pub mod integrated;
pub mod job_manager;
pub mod journal;
pub mod patch_engine;
pub mod provider_router;
pub mod runtime;
#[cfg(windows)]
pub(crate) mod sol_worker_identity_preflight;
#[cfg(windows)]
pub(crate) mod sol_worker_lifecycle;
pub(crate) mod sol_worker_nonce;
pub(crate) mod sol_worker_output;
pub(crate) mod sol_worker_staging;
pub(crate) mod sol_worker_staging_security;
#[cfg(windows)]
pub(crate) mod sol_worker_token_security;
pub mod state_machine;
pub mod supervisor;
pub mod worker_provider;

pub const EXECUTION_CONTRACT_SCHEMA_VERSION: u32 = 1;
pub const EXECUTION_PROTOCOL_VERSION: &str = "catdesk.delegated.v1";
