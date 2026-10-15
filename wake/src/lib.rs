//! Independent Wake protocol and durable state. No CatDesk schemas or processes.
pub mod install_authority;
pub mod install_broker_approval;
pub mod install_broker_ci_witness;
#[cfg(windows)]
pub mod install_broker_ledger;
pub mod install_broker_preflight;
pub mod process_job;
pub mod protocol;
pub mod review_binding;
pub mod runtime;
pub mod store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL_VERSION: u32 = 1;
pub type Result<T> = std::result::Result<T, String>;
