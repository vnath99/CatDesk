#![allow(dead_code)]

pub mod context;
pub mod contracts;
pub mod coordinator;
pub mod events;
pub mod journal;
pub mod patch_engine;
pub mod provider_router;
pub mod runtime;
pub mod state_machine;
pub mod supervisor;

pub const EXECUTION_CONTRACT_SCHEMA_VERSION: u32 = 1;
pub const EXECUTION_PROTOCOL_VERSION: &str = "catdesk.delegated.v1";
