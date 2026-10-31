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

pub mod agent;
pub mod api;
pub mod auth;
pub mod capture;
pub mod client;
pub mod diff;
pub mod error;
pub mod mcp;
pub mod model;
pub mod notify;
pub mod paths;
pub mod store;

pub use error::{Error, Result};

/// Hard cap for request bodies (screenshots + composited PNG). Matches v0.1.
pub const MAX_BODY_BYTES: usize = 60 * 1024 * 1024;
/// Video upload cap (250 MB). The multipart create route has its own body limit; JSON/base64
/// video uploads stay bounded by `MAX_BODY_BYTES`.
pub const MAX_VIDEO_BYTES: usize = 250 * 1024 * 1024;
/// Poster/thumbnail cap. A poster is a first frame, not a second video.
pub const MAX_POSTER_BYTES: usize = 8 * 1024 * 1024;
/// Product limit for a review clip. The UI checks the browser's duration and the server rejects a
/// declared duration over the limit; a client that omits duration is bounded by the byte cap.
pub const MAX_VIDEO_DURATION_MS: u64 = 5 * 60 * 1000;
/// Per-submission full-frame cap: 20 unique annotated timestamps, ~60 MB of frames.
pub const MAX_FRAME_COUNT: usize = 20;
pub const MAX_FRAMES_BYTES: usize = 60 * 1024 * 1024;
/// Multipart submit body: the contact sheet plus the frames, with room for MIME overhead.
pub const MAX_SUBMIT_BYTES: usize = 80 * 1024 * 1024;

/// Protocol version reported to MCP clients.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Server version, surfaced in `initialize` and the `/health` payload.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
