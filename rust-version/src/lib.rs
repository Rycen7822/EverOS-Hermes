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
mod mcp_dispatch;
mod mcp_tools;
pub mod policy;
pub mod provider;
mod provider_config;
pub mod provider_tools;
pub mod redaction;
pub mod response_normalization;
pub mod trajectory;
pub mod workflows;
