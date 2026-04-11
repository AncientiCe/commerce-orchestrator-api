//! Model Context Protocol (MCP) tool server for commerce orchestration.
//!
//! Provides JSON-RPC 2.0 request/response types, tool definitions mapping to
//! `OrchestratorFacade` operations, and a handler for processing MCP messages.

pub mod jsonrpc;
pub mod tools;

pub use jsonrpc::*;
pub use tools::*;
