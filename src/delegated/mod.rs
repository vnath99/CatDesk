#![allow(dead_code)]

pub mod contracts;
pub mod events;
pub mod journal;
pub mod state_machine;

pub const EXECUTION_CONTRACT_SCHEMA_VERSION: u32 = 1;
pub const EXECUTION_PROTOCOL_VERSION: &str = "catdesk.delegated.v1";
