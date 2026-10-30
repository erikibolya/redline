# DESKTOP — RedLine as a tray application (v1.0)

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

## Server on another machine (Unraid / NAS / WSL + Windows desktop)

The common real setup: repositories and the agent run on a server (`holly`), the human draws on a
Windows desktop. The app then runs in **thin-client mode** — the server lives where the agents are,
so reviews can be created while the desktop is off.

```
holly (always on)                         Windows desktop
redline container :8788  ←── HTTP ──────  redline-app (tray)
  ├─ /mcp   ← agents on holly (127.0.0.1)
  ├─ /data  ← sessions next to the repos
  └─ /      → editor, rendered in the app's webview
```

**1. Start the server on holly** (Unraid: Docker → Add Container, or compose):

```yaml
services:
  redline:
    image: ghcr.io/erikibolya/redline:latest
    ports: ["8788:8788"]
    volumes: ["/mnt/user/appdata/redline:/data"]
    environment:
      REDLINE_TOKEN: "paste-a-long-random-string"   # optional but recommended
    restart: unless-stopped
```

`docker logs redline` prints the token, the human URL and the MCP config.

**1b. No Docker? Run the static binary instead.** Unraid ships a shell; the server is one
self-contained file (no libc, no dependencies):

```bash
wget -O /usr/local/bin/redline \
  https://github.com/erikibolya/redline/releases/download/v0.2.0/redline-linux-x86_64
chmod +x /usr/local/bin/redline
REDLINE_TOKEN="$(head -c 32 /dev/urandom | base64 | tr -d '/+=')" \
  /usr/local/bin/redline serve --host 0.0.0.0 --data-dir /mnt/user/appdata/redline
```

To survive reboots, add those last two lines to `/boot/config/go` (Unraid runs it at array
start). The container is preferred only because updates are a `docker pull`; the binary needs a
`wget` of the new release.

**2. Point the Windows app at it — in the app, not in a file.**

Tray or menu bar → **Servers…** → **Add server**, give it a name (for example *Production*),
paste the address and token, **Save and connect**. The app probes the server with those
credentials before saving anything, so a wrong token is an error message instead of a broken next
launch. **Servers…** lists every profile with its URL, state and version; *Use* switches the window
without signing out of the other servers. The local *This computer* row is always present and
cannot be removed, so the wizard needs no separate "use this computer" action.

Profiles live in `%LOCALAPPDATA%\redline\config.json`:

```json
{
  "servers": [
    { "id": "production", "name": "Production", "url": "https://redline.ibolya.cz", "token": "…" },
    { "id": "dev", "name": "Dev", "url": "http://192.168.68.115:8790", "token": "…" }
  ],
  "active": "production"
}
```

`active: "local"` selects *This computer*. The old `remote_url`/`token` file is migrated as the
first profile on read. The local server keeps running in every mode — that is what serves the
Servers page and the wizard — but in remote mode the window (and every review) comes from the
selected server.

**3. Agents on holly** use `http://127.0.0.1:8788/mcp` — loopback inside the container network,
never exposed to the LAN:

```json
{ "mcpServers": { "redline": {
    "type": "http",
    "url": "http://127.0.0.1:8788/mcp",
    "headers": { "Authorization": "Bearer <token>" } } } }
```

Notes:

- Only the **human-facing** port is reachable from the LAN; agents talk to loopback.
- No TLS: the token protects the port. If the LAN is not trusted, put the container behind your
  existing reverse proxy (Unraid SWAG/NPM) and use `https://…`, or tunnel with WireGuard/Tailscale.
- The Windows app is optional: `http://holly:8788/?t=<token>` works in any browser.
- Sessions live in `/data/sessions` on holly, so the agent can read `annotations.json` directly
  from disk if it prefers files to MCP.

### Traefik / HTTPS on Unraid

The installer can wire the labels for you, so a re-run cannot drop the route:

```bash
REDLINE_DOMAIN=redline.ibolya.cz bash desktop/install-unraid.sh
```

It adds `--network traefik_network` plus the same labels the other Unraid stacks use
(`websecure` entrypoint, `Host(\`…\`)`, `tls.certresolver=cloudflare`, service port 8788).
Override with `REDLINE_TRAEFIK_NETWORK` / `REDLINE_CERTRESOLVER` if yours differ.

The app's Connect form accepts `https://` too. For an https server the credentials are not probed
over plain HTTP (the client has no TLS stack); the webview loads the URL and reports a bad
certificate instead.

### Capture, updates and signing

**`capture_review`** (MCP) screenshots a URL with headless Chromium and opens a review session for
it, so an agent no longer needs a screenshot file on disk:

```json
{"name": "capture_review", "arguments": {"url": "http://localhost:5016/button", "viewport": "1440x900"}}
```

It needs a browser on the server: use the `:browser` image
(`ghcr.io/erikibolya/redline:browser`, ~400 MB with Chromium) or set `REDLINE_CHROME`. Without one
the tool returns an actionable error instead of failing silently.

**Auto-update** — the app checks `releases/latest/download/latest.json` once at launch, and only
restarts when an update actually downloaded and installed. The manual check lives in
**Help → Check for updates…** (the tray menu keeps its own copy, since a tray has no Help menu). Artifacts are signed with the updater
key (`TAURI_SIGNING_PRIVATE_KEY` secret); the public half lives in `tauri.conf.json`. The private
key is at `/mnt/user/appdata/redline-secrets/updater.key` — **losing it means existing installs
can never update again**.

**Windows code signing** — the release workflow signs automatically when these repository secrets
exist; without them the build is unsigned (SmartScreen warns):

| Secret | Contents |
|--------|----------|
| `WINDOWS_CERT_PFX` | base64 of the `.pfx` (OV/EV code-signing certificate) |
| `WINDOWS_CERT_PASSWORD` | its password |

#### Free signing — what actually exists

There is no free, instantly-trusted certificate: Windows trusts a publisher only after a CA has
verified an identity. The two realistic zero-cost routes:

1. **Certum Open Source Code Signing** — free for public OSS, valid 1 year, personal identity
   check (passport/ID + video call), certificate exported as `.pfx`. Fits the pipeline as-is:

   ```bash
   desktop/scripts/add-signing-cert.sh /path/to/cert.pfx 'password'
   ```

2. **SignPath Foundation** — free EV-grade signing for qualifying OSS, but signing happens on
   their servers; nothing lands on this machine. Requires an approved project and four values
   (`SIGNPATH_API_TOKEN`, organisation id, project slug, signing policy). The workflow needs one
   extra job that signs the installer before upload — ask and it will be wired.

**Self-signed certificates are pointless here**: the file is signed, Windows still warns, and
SmartScreen reputation never accrues. They are only useful for internal distribution with a
GPO-deployed root.

What is *already* signed regardless: the update packages, with our own minisign key
(`TAURI_SIGNING_PRIVATE_KEY`). That protects the updater from tampered downloads; it has nothing
to do with the SmartScreen prompt.

A certificate must be bought or granted to a person, not to a repository — that identity step is
the one part that cannot be automated here. Everything after it is: secrets in, signed build out.

Nothing else is needed — the workflow writes the Tauri signing config and signs `redline-app.exe`
plus the installer.

**One front door** — when `REDLINE_DOMAIN` is set the installer no longer publishes port 8788, and
passes `--public-url https://$DOMAIN` so review links and `Copy MCP config` use the HTTPS address.
`REDLINE_PUBLISH_PORT=1` keeps the LAN port if you want both.

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
| P2 | Tauri shell: tray, hidden window, embedded UI, single instance | App runs on Windows, agent connects over MCP (compiles in CI; desktop run pending) |
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
- **P2 — done, unverified on a real desktop.** `desktop/app` is a Tauri v2 shell: tray icon with
  *Open · Start at login · Copy MCP config · listening address · Quit*, window closes to the tray,
  single-instance, embedded UI, server on a free loopback port. Compiles and links on
  `windows-latest` (CI). Nobody has clicked it on a physical Windows desktop yet — that is the
  next check, not a known defect.
- **Download.** Installers are published as GitHub **pre-releases** by
  `.github/workflows/release.yml`: push a `v*-desktop.*` tag and the workflow builds the NSIS
  installer and attaches it to the release page (no artifact hunting). Builds are unsigned, so
  Windows SmartScreen asks for *More info → Run anyway*.
- **Port is pinned to 8788** (`REDLINE_PORT` overrides, ephemeral fallback if it is taken). This is
  what makes `redline open` and agent MCP configs work without knowing a random port.
- **P3 — partial.** NSIS per-user installer builds in CI (`redline_<version>_x64-setup.exe`,
  ~1.9 MB) and is uploaded as an artifact. Autostart and single-instance landed. Still open:
  code signing, the updater plugin, and macOS/Linux bundles.

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

## Compose deployment (server + agent)

`desktop/install-unraid.sh` is the one-liner for a single container. If a compose project owns the
deployment (Compose Manager, docker compose), put the server and the agent in the same stack so the
agent survives server updates:

```yaml
services:
  redline:
    image: ghcr.io/erikibolya/redline:browser
    container_name: redline
    restart: unless-stopped
    networks: [traefik_network]
    entrypoint: ["/redline"]
    command: ["serve", "--host", "0.0.0.0", "--data-dir", "/data",
              "--public-url", "https://redline.example"]
    volumes: ["/mnt/user/appdata/redline:/data"]
    # …the same Traefik labels the installer writes…

  redline-agent:
    build:
      context: /path/to/redline
      dockerfile: desktop/crates/redline-agent/Dockerfile
    image: redline-agent:0.19.0
    container_name: redline-agent
    restart: unless-stopped
    networks: [traefik_network]
    depends_on: [redline]
    working_dir: /repos/app            # must exist inside the OpenCode server too
    env_file: /path/to/opencode/.env   # OPENCODE_PASSWORD
    environment:
      REDLINE_URL: http://redline:8788
      REDLINE_AGENT_REPO: /repos/app                 # fallback for legacy projects
      REDLINE_AGENT_REPO_ROOT: /repos                # keys resolve here
      REDLINE_AGENT_OPENCODE_ROOT: /srv/repos        # the same dir as OpenCode sees it
      REDLINE_AGENT_ID: holly
      REDLINE_AGENT_OPENCODE_SERVER: http://opencode:4096
      REDLINE_AGENT_MODEL: deepseek/deepseek-flash
    volumes:
      - /mnt/user/appdata/redline/token:/data/token:ro
      - /srv/repos:/repos
      - /path/to/opencode:/usr/local/bin/opencode:ro
```

Three things that bite:

- **Auth.** A shared OpenCode server with `OPENCODE_PASSWORD` refuses `opencode run --server`
  without that variable; give the agent the same environment.
- **Paths.** `opencode run` sends its working directory to the server, so `REDLINE_AGENT_REPO`
  must exist *inside the OpenCode container*, not just in the agent. The repo sync stores **keys
  relative to `REDLINE_AGENT_REPO_ROOT`**; `REDLINE_AGENT_OPENCODE_ROOT` is the prefix OpenCode
  uses for the same directory, so both mounts collapse to `redline`, `ActiUnit/Au.Net`, ….
- **Two owners.** Once compose owns the `redline` container name, do not run
  `install-unraid.sh` again; update with `docker compose pull && docker compose up -d`.

The agent refreshes the repo list every `REDLINE_AGENT_SYNC` seconds (default 60) and at startup.
It never deletes a project with reviews; a repo that disappears becomes an error in the review
instead of silently running somewhere else.

The agent container only needs the network, the token and the OpenCode CLI; the OpenCode server
does the actual editing. Because it is a separate service, updating OpenCode no longer stops the
loop — the agent reconnects by service name on the next submission.

## Dev channel and revert

`dev` is the integration branch. Stable releases still come from `v*` tags on `main`; every push
to `dev` publishes a rolling prerelease under the moving **`dev`** tag
(`.github/workflows/dev.yml`), versioned `<next-minor>-dev.<run number>` (bump `DEV_BASE_VERSION` in the
workflow when a new stable minor ships).

The desktop app's menu (RedLine/Help and the tray):

| Item | Manifest | Behaviour |
|---|---|---|
| **Check for updates…** | `releases/latest/download/latest.json` | stable channel, upgrades only |
| **Install latest dev build…** | `releases/download/dev/latest.json` | dev channel, upgrades only |
| **Revert to last stable version…** | `releases/latest/download/latest.json` | same manifest, but the updater's `version_comparator` accepts an older version, so a downgrade actually installs |

Both installers are signed with the same updater key, so the dev channel needs no extra secret.
Dev builds are unsigned for Windows SmartScreen (no Authenticode), exactly like stable.

Why a separate comparator for revert: the updater refuses to install anything that is not newer
than the running app. Revert flips that check to "any different version" and lets the NSIS
installer replace the dev build in place. Data (`%APPDATA%\redline`) is untouched.
