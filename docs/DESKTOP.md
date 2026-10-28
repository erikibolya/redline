# DESKTOP — redline as a tray application (v1.0)

Status: **accepted**, branch `desktop`. Supersedes parts of `PLAN.md` and decision **D5**.
Owner: Erik. Last updated: 2026-10-28.

## Context

`v0.1.0` is a Python CLI plus a localhost web editor. It works, but every review starts with
"start a server, remember a port, keep the terminal open", and an agent can only hand over a
screenshot by writing a file into a checkout both sides can see.

The product we actually want: **an always-running tray app** that agents connect to directly,
upload a screenshot to, and get the human's redlines back from — with no terminal in the loop and
no shared folder.

## Decision

Ship **v1.0 as a Tauri v2 desktop application** that embeds the HTTP server, the existing web
editor, and an MCP endpoint, and runs **window-hidden in the system tray** (Windows) / menu bar
(macOS). Windows first; macOS and Linux installers follow in the same release train.

### Why Tauri v2

The requirement list — installer, tray, start-on-login, single instance, background listening,
auto-update — is Tauri's plugin surface, and the editor is already a single-page canvas app that
runs unchanged in any webview.

| Candidate | Verdict |
|-----------|---------|
| **Tauri v2** | Chosen. ~10 MB installer, first-class `tray-icon` / `autostart` / `single-instance` / `updater` plugins, NSIS + MSI with WebView2 bootstrap, DMG, AppImage/deb/rpm. Thin Rust backend. |
| Electron | Fastest to write and the best MCP SDK, but 100–150 MB installers for a screenshot annotator is a bad trade. Rejected. |
| Wails v3 | Natural Go fit, but v3 is alpha; a long-lived tool on an alpha runtime is the one risk not worth taking. Revisit if v3 ships stable. |
| Stay on Python (pywebview + pystray + PyInstaller) | No rewrite, but 40–80 MB binaries, antivirus false positives, weak Wayland tray, fragile Linux packaging. Rejected. |

Costs accepted: Rust toolchain, one build runner per OS (GitHub Actions matrix), `webkit2gtk-4.1`
required for Linux builds, WebView2 required at runtime on Windows.

## Architecture

One process, one binary, two front doors.

```
redline-app (tray-resident, window starts hidden)
├─ webview window ────────────── serves the embedded editor UI
├─ axum server on 127.0.0.1:8788
│   ├─ GET  /                    session list
│   ├─ GET  /s/<id>              canvas editor   (same static assets as v0.1)
│   ├─ /api/sessions/…           JSON API        (bearer token / cookie)
│   └─ POST /mcp                 MCP, JSON-RPC 2.0 over streamable HTTP
├─ redline-core                  session store, auth, MCP tool handlers
└─ tray                          status · Open · Copy MCP config · Pause · Quit
```

`redline-cli` (`redline open|list|get|wait|remove|mcp --stdio`) is a **thin HTTP client** of the
running app: same verbs as v0.1, so existing scripts and the Au.Net workflow keep working. If the
app is not running, the CLI starts it (Windows) or prints a clear error.

### Crate layout

```text
desktop/
  Cargo.toml                 workspace; default-members = core + cli (no GUI deps)
  crates/redline-core/       store, api, auth, mcp — compiles and tests on any OS
  crates/redline-cli/        thin client binary
  app/                       Tauri v2 shell (Windows build; needs WebView2/webkit2gtk)
  ui/                        editor assets (derived from static/, still served to browsers)
```

Splitting core out of the shell is deliberate: it means the entire backend is `cargo test`-able in
WSL and in CI without a display server, and the Tauri layer stays a few hundred lines of glue.

## Auth model (supersedes D5)

v0.1 was "localhost, no auth". A port that agents can reach must not be open to every process on
the machine, and a random web page must not be able to POST annotations to `127.0.0.1`.

- A **token is generated on first run** (32 bytes, base64url) and stored in the OS keychain
  (Windows Credential Manager / macOS Keychain / libsecret) with a config-file fallback.
- Every `/api` and `/mcp` request requires `Authorization: Bearer <token>`, or the
  `redline_token` cookie, or `?t=<token>` on first contact.
- Human-facing links carry the token once: `GET /s/<id>?t=…` sets an `HttpOnly; SameSite=Strict`
  cookie and 302s to the clean URL, so the address bar and history stay tidy.
- Mutating requests additionally require `Sec-Fetch-Site: same-origin|none`, defeating drive-by
  CSRF from a browser tab.
- Binding to anything other than loopback is opt-in and prints a warning; the tray shows the
  current bind address.
- Token rotation: tray → **Rotate token** revokes old credentials and re-copies the config.

## MCP surface

MCP is the primary agent integration; the HTTP API remains for scripts and for hosts without MCP.
Transport: **streamable HTTP** at `/mcp` (JSON responses, no SSE needed for these calls), plus
`redline mcp --stdio` as a bridge for stdio-only hosts (Claude Desktop today, most IDEs).

Tools:

| Tool | Purpose |
|------|---------|
| `review_screenshot` | Upload an image (base64 or absolute path on the app's host), create a session, optionally block until the human submits. Returns `session_id`, `review_url`, `status`. |
| `capture_review` | Screenshot a URL via headless Chrome (`--selector`, `--viewport`), then run `review_screenshot`. Removes "agent must supply a file". |
| `wait_for_feedback` | Long-poll a session until `submitted` or timeout; returns annotations, `review.md`, and an annotated-image resource link. |
| `get_feedback` | Same payload, non-blocking. |
| `list_reviews` | Sessions with status/title/timestamps, newest first. |
| `reply` | Add an agent message to a session (question or "what did you mean here?") shown in the editor. |
| `resolve` | Mark a session done with an optional summary; the tray badge clears. |
| `prune` | Delete submitted sessions older than N days. |

Resources: `redline://session/<id>/annotated.png`, `…/review.md`, `…/annotations.json`, so an
image-capable agent can look at the redlines instead of only reading coordinates.

Tray → **Copy MCP config** emits the exact JSON snippet (URL + token) for Claude Desktop and
OpenCode. That menu item is the whole onboarding story; no docs needed to connect an agent.

## Data locations

| OS | Sessions | Config |
|----|----------|--------|
| Windows | `%LOCALAPPDATA%\redline\sessions` | `%APPDATA%\redline\config.json` |
| macOS | `~/Library/Application Support/redline/sessions` | `~/Library/Application Support/redline` |
| Linux | `~/.local/share/redline/sessions` | `~/.config/redline` |

`--data-dir DIR` / `REDLINE_DATA_DIR` forces a portable layout (sessions + config side by side),
which keeps the current "shared folder" workflow alive for WSL/Windows splits.

## Build & release

- `cargo test` (workspace default members) → core + CLI, no GUI deps, runs on Linux/Windows/macOS.
- `cargo tauri build` on Windows → NSIS `.exe` + `.msi`, WebView2 bootstrapper, per-user install
  (no admin prompt), start-menu + optional autostart entry, file association for `.png` review.
- GitHub Actions matrix: `windows-latest` (primary), `macos-latest`, `ubuntu-22.04`
  (+ `libwebkit2gtk-4.1-dev`). Artifacts upload to the release for a tag.
- Updater: Tauri updater plugin against GitHub Releases; signature key generated once and stored
  as a repository secret.

## Migration from v0.1

1. Python stays the shipped product on `main` at `v0.1.0`. Desktop work lands on `desktop`.
2. `v1.0.0` is cut only at **parity**: every v0.1 CLI verb, the editor, and the session folder
   format still work; a v0.1 `sessions/` directory opens unchanged (same `meta.json` schema).
3. `static/` moves to `desktop/ui/`; the Python package is removed at v1.0.0 with a note in
   `CHANGELOG.md` and a link to the last Python tag.
4. Schema changes stay additive: `meta.json` gains `agent`, `reviewer`, `resolved_at`.

## Phases

| Phase | Deliverable | Exit criterion |
|-------|-------------|----------------|
| P0 | `redline-core` store + API + auth + MCP, `cargo test` green in WSL | Unit + HTTP tests pass without a GUI |
| P1 | `redline-cli` port of `serve/open/list/get/wait/remove` | v0.1 recipe works against the new server |
| P2 | Tauri shell: tray, hidden window, embedded UI, single instance | App runs on Windows, agent connects over MCP |
| P3 | Installers (NSIS/MSI), autostart, updater, Actions matrix | Signed-per-user installer, upgrade in place |
| P4 | `capture_review` via headless Chrome, before/after compare | Screenshot → annotation → follow-up in one loop |

### Status

- **P0 — done.** `cargo test` : 20 tests (store, HTTP, MCP, auth) green. Verified end-to-end with
  the real binary: MCP `review_screenshot` → human submit over HTTP → MCP `get_feedback` returns
  annotations + `review.md` (byte-identical format to v0.1) + `annotated.png` resource.
- `redline serve` (headless) exists and is the process the Tauri shell will host; `redline token`
  and `redline mcp-config` print the credentials and the client config the tray menu copies.
- **P1 — done.** `serve`, `open`, `list`, `get`, `wait`, `remove`, `token`, `mcp-config` all work.
  The client verbs talk HTTP to a running instance and keep the v0.1 contract: `wait` exits 2 on
  timeout, `get`/`remove` exit 1 for a missing session, `--json` stays machine-readable.
  `--sessions` moved server-side (the client no longer touches files); `DELETE /api/sessions/{id}`
  was added for `remove`. Covered by `tests/cli.rs`, which spawns the real binary.
- P2+ — not started; needs a Windows toolchain and WebView2 (this WSL box cannot link a GUI).

### Building in WSL without a C toolchain

This machine has no `cc`, so Rust build scripts cannot link and `webkit2gtk` cannot be installed
(no root). The workaround, already configured in `desktop/.cargo/config.toml`:

```bash
# one-time: self-contained C toolchain in user space
curl -sSf -o zig.tar.xz https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz
mkdir -p ~/.local/zig && tar -xJf zig.tar.xz -C ~/.local/zig --strip-components=1
printf '#!/bin/sh\nexec ~/.local/zig/zig cc "$@"\n' > ~/.local/bin/cc && chmod +x ~/.local/bin/cc

export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
rustup target add x86_64-unknown-linux-musl
cd desktop && cargo test --target x86_64-unknown-linux-musl
```

The musl target links with Rust's bundled `rust-lld`, so nothing else is needed. On Windows,
plain `cargo test` works with the MSVC toolchain; the `.cargo/config.toml` section is ignored.


## Risks

- **WebView2** missing on old Windows → NSIS bootstrapper installs it; document the download size.
- **Linux tray** is unreliable under Wayland/GNOME (no tray by default) → ship `.deb` with an
  AppIndicator dependency and document `redline serve` as the headless fallback.
- **Rust learning/maintenance cost** for a small tool → mitigated by keeping `redline-core` free
  of GUI types and Tauri glue minimal; all logic lives in testable crates.
- **Two implementations during migration** → timeboxed; Python is frozen, not developed.

## Open questions

- macOS: menu-bar-only app (`ActivationPolicy::Accessory`) or normal Dock presence with a window?
- Should `capture_review` bundle a headless Chrome (large) or require one on `PATH`?
- Auto-update on by default, or opt-in until the first few releases settle?
