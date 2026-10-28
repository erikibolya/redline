//! redline-core: everything redline knows that is not a window.
//!
//! Deliberately free of GUI dependencies so the whole backend is testable in CI and in WSL
//! without a display server. The Tauri shell (`redline-app`) is a thin wrapper around this crate.
//!
//! Layering:
//! - [`store`] — the session folder format on disk (`meta.json`, `annotations.json`, `review.md`).
//! - [`api`] — the HTTP surface used by the editor, the CLI and scripts.
//! - [`mcp`] — JSON-RPC 2.0 tool surface for AI agents, mounted at `POST /mcp`.
//! - [`auth`] — the bearer token that keeps a local port from being a public one.
//! - [`client`] — the loopback HTTP client used by the CLI and by the setup wizard.

pub mod api;
pub mod auth;
pub mod capture;
pub mod client;
pub mod error;
pub mod mcp;
pub mod model;
pub mod paths;
pub mod store;

pub use error::{Error, Result};

/// Hard cap for request bodies (screenshots + composited PNG). Matches v0.1.
pub const MAX_BODY_BYTES: usize = 60 * 1024 * 1024;

/// Protocol version reported to MCP clients.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Server version, surfaced in `initialize` and the `/health` payload.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
