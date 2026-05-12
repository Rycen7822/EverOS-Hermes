//! Rust port of EverOS-Hermes.
//!
//! The crate keeps the same surfaces as the Python implementation:
//! a small EverOS REST client, formatting helpers, Hermes memory-provider core,
//! and a local stdio MCP server.

pub mod cli;
pub mod client;
pub mod context_assembler;
pub mod env;
pub mod formatting;
pub mod mcp;
pub mod policy;
pub mod provider;
pub mod trajectory;
pub mod workflows;
