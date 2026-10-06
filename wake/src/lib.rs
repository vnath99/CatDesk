//! Independent Wake protocol and durable state. No CatDesk schemas or processes.
pub mod process_job;
pub mod protocol;
pub mod runtime;
pub mod store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL_VERSION: u32 = 1;
pub type Result<T> = std::result::Result<T, String>;
