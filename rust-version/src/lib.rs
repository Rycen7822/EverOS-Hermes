//! Rust port of EverOS-Hermes.
//!
//! The crate keeps the same surfaces as the Python implementation:
//! a small EverOS REST client, formatting helpers, Hermes memory-provider core,
//! and a local stdio MCP server.

pub mod agent_visibility;
pub mod cli;
pub mod client;
pub mod context_assembler;
pub mod env;
pub mod flush_retry;
pub mod formatting;
pub mod mcp;
pub mod policy;
pub mod provider;
pub mod provider_tools;
pub mod redaction;
pub mod response_normalization;
pub mod trajectory;
pub mod workflows;
