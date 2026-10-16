//! Independent Wake protocol and durable state. No CatDesk schemas or processes.
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
pub mod install_broker_offline_v2;
pub mod install_broker_operator;
#[cfg(windows)]
pub(crate) mod install_broker_pipe_io;
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
pub mod protocol;
pub mod review_binding;
pub mod runtime;
pub mod store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL_VERSION: u32 = 1;
pub type Result<T> = std::result::Result<T, String>;
