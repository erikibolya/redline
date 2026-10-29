# Agent instructions — redline

Read this before touching anything. It is the handoff document: what redline is, where it runs,
how to change and ship it, and the traps that have already cost time.

---

## 1. What this is, and where it stands

`redline` is a local visual review queue for human ↔ AI collaboration: an agent produces a
screenshot, a human draws on it in a browser or the desktop app, the agent reads the result as
JSON + Markdown + an annotated PNG.

**Current release: v0.6.0.** Live in production on holly:

| Thing | Value |
|---|---|
| Human UI | `https://redline.ibolya.cz` (Traefik → container, Let's Encrypt via cloudflare resolver) |
| Agent endpoint | `https://redline.ibolya.cz/mcp` (or `http://127.0.0.1:8788/mcp` on holly) |
| Container | `redline`, image `ghcr.io/erikibolya/redline:browser`, no published port |
| Data | `/mnt/user/appdata/redline` → `/data` (sessions in `/data/sessions`) |
| Token | `/mnt/user/appdata/redline/token` (also printed by `docker logs redline | grep -i token`) |
| Repo | `https://github.com/erikibolya/redline` (public, MIT), branch `main` |

The Windows desktop app (`desktop/app`) is a Tauri v2 shell: tray + menu bar + a webview pointed at
the server. It is a *client*; all logic is server-side. It auto-updates from GitHub Releases.

## 2. Environment you are actually in

You are in a **container on holly (Unraid)** with, by explicit decision of the owner:

- `/var/run/docker.sock` mounted → full control of every container;
- `/mnt/user` mounted read-write → the whole array.

That is **root-equivalent**. The global rules in `~/.config/opencode/AGENTS.md` require asking the
human before anything destructive (deleting data, touching containers other than `redline`,
changing other services, `/boot`, rewriting git history). Read them; they are loaded automatically.

Environment traps that have already bitten:

- **Docker runs on the host, not in this container.** Bind-mount sources must be *host* paths
  (`/mnt/user/...`). Mounting `/tmp/opencode/...` silently creates an empty directory on the host.
- **The sandbox home is ephemeral.** `~`, `~/.cargo`, `/tmp` vanish when the container is
  recreated. Tooling lives on the array:
  - `gh` → `/mnt/user/appdata/agent-tools/bin/gh`
  - gh credentials → `GH_CONFIG_DIR=/mnt/user/appdata/agent-tools/gh`
  - git credential helper is configured globally to use that path.
  Export both before pushing: `export GH_CONFIG_DIR=/mnt/user/appdata/agent-tools/gh PATH=/mnt/user/appdata/agent-tools/bin:$PATH`
- **No Rust toolchain on the host.** Build with Docker:
  `docker run --rm -v /mnt/user/Development/repos/redline:/src -w /src/desktop rust:1-bookworm cargo test --workspace`
- **Signing/updater keys**: `/mnt/user/appdata/redline-secrets/updater.key` (private, 600) and
  `.pub`. The private key is also a GitHub secret. Losing it means existing installs can never
  update again.

## 3. Repository layout

```
desktop/
  Cargo.toml              workspace: crates + no app (the app is its own workspace, needs a GUI)
  crates/redline-core/    store, api, auth, mcp, capture, diff, client, paths — no GUI deps
  crates/redline-server/  `redline` binary: serve / open / list / get / wait / remove /
                          token / mcp-config / health
  app/                    Tauri v2 shell (tray, menu, updater, setup wizard)
  ui/                     app.css, app.js, index.html (editor), list.html, setup.html
  scripts/                e2e.mjs (browser test), e2e.sh, add-signing-cert.sh
  Dockerfile              scratch + static musl binary (~4 MB)
  Dockerfile.browser      debian-slim + chromium (~768 MB) — needed for capture
  install-unraid.sh       idempotent install/upgrade of the container
  docker-compose.example.yml
docs/                     DESKTOP.md (ADR), UI.md (UI plan/status), API.md, ARCHITECTURE.md,
                          WORKFLOW.md, PLAN.md at the root
static/, redline.py       v0.1 Python implementation, frozen; do not add features to it
```

## 4. Build, test, ship

```bash
# Rust (host has no toolchain: use the rust image)
docker run --rm -v /mnt/user/Development/repos/redline:/src -w /src/desktop rust:1-bookworm \
  sh -c 'rustup component add rustfmt; cargo fmt --all; cargo test --workspace'

# Browser end-to-end (32 checks: drawing, zoom mapping, move, groups, align, history, diff, rail actions)
docker build -q -f desktop/Dockerfile.browser -t redline:local .
REDLINE_IMAGE=redline:local bash desktop/scripts/e2e.sh     # prints ok/FAIL per check

# Ship
git push origin main            # CI: core+cli on Linux & Windows, tauri shell, browser e2e
git tag -a vX.Y.Z -m "..." && git push origin vX.Y.Z   # release: installer, CLI, linux binary, images
REDLINE_IMAGE=ghcr.io/erikibolya/redline:browser REDLINE_DOMAIN=redline.ibolya.cz \
  bash desktop/install-unraid.sh                        # deploy to holly
```

Version bumps touch three files: `desktop/Cargo.toml`, `desktop/app/Cargo.toml`,
`desktop/app/tauri.conf.json`.

CI secrets: `TAURI_SIGNING_PRIVATE_KEY` (updater), optional `WINDOWS_CERT_PFX` +
`WINDOWS_CERT_PASSWORD` (Authenticode — not set; builds are unsigned and SmartScreen warns).

## 5. Deployment shape

The installer adds Traefik labels (copied from the other stacks on this host):

```
--network traefik_network
traefik.http.routers.redline.rule=Host(`redline.ibolya.cz`)
traefik.http.routers.redline.entrypoints=websecure
traefik.http.routers.redline.tls.certresolver=cloudflare
traefik.http.services.redline.loadbalancer.server.port=8788
```

When `REDLINE_DOMAIN` is set the container publishes **no** port and runs with
`--public-url https://$DOMAIN`. `REDLINE_PUBLISH_PORT=1` keeps a LAN port (don't, unless needed).

Env knobs: `REDLINE_DOMAIN`, `REDLINE_IMAGE`, `REDLINE_PORT`, `REDLINE_TOKEN`, `REDLINE_DATA`,
`REDLINE_PUBLISH_PORT`, `REDLINE_CHROME` (browser binary for capture).
Rust side: `REDLINE_PORT`, `REDLINE_DATA_DIR`, `REDLINE_TOKEN`, `REDLINE_CHROME`, `REDLINE_HOST`.

## 6. Architecture in one screen

- `redline-core` — session folder store (`meta.json`, `annotations.json`, `review.md`,
  `annotated.png`, `history/`), project folders (`sessions/projects/<slug>/`, v0.10), axum HTTP
  API, bearer/cookie auth, MCP JSON-RPC, headless capture (shells out to Chromium), annotation
  diffing, loopback HTTP client.
- Auth: token from `/data/token`; `Authorization: Bearer`, `redline_token` cookie (set by
  `/s/<id>?t=…` → 307), or `?t=`. `Sec-Fetch-Site: cross-site` is refused.
- MCP tools: `review_screenshot`, `capture_review`, `get_feedback`, `wait_for_feedback`,
  `list_reviews`, `list_projects`, `reply`, `progress`, `resolve`. Resources: `redline://session/<id>/{annotated.png,review.md}`.
- Agent state: `get_feedback` stamps `seen_by_agent_at` and sets `agent_state=received`;
  `progress` appends to `agent-events.jsonl` and sets `working`; `reply` sets `replied`.
  The editor polls meta + thread + events every 5 s and renders them as a conversation.
- UI: no framework, no build step. `app.js` holds the shell (rail, canvas engine, inspector);
  `app.css` the design tokens incl. light theme. Served from the binary at `/static/*`.
- Editor model: annotations = paint order; selection is a `Set`; viewport is a transform
  (`scale/x/y`) applied in `render()`, so zoom is crisp and pointer mapping is `(screen - view)/scale`.

## 7. Lessons already paid for (do not relearn)

- **Pointer capture in the stage swallows clicks** on children (options bar). Guard:
  `if (event.target.closest(".options, .text-editor")) return;`.
- **Text entry**: `pointerdown` must `preventDefault()` (else mousedown focus-steals and the empty
  editor commits), focus synchronously, and `commit()` must be idempotent (removal fires `blur`).
- **`RunEvent::ExitRequested` is vetoed** to keep the tray app alive; a quit/restart must set a
  flag first or it silently does nothing.
- **Bundle builds need the updater key**: `createUpdaterArtifacts` fails without
  `TAURI_SIGNING_PRIVATE_KEY`, in `desktop.yml` too.
- **Release job ordering**: the Linux-binary job needs `needs: windows` (the release is created by
  the Windows job).
- **Chromium binds its debugger to loopback**: run the puppeteer container with
  `--network container:redline-e2e`.
- **In tests, poll; never `sleep`** for a submit to land (flaked twice).
- **`--public-url` was silently ignored** once because the flag was missing from `VALUE_FLAGS`;
  unknown `--flags` now error instead of being swallowed.
- **`docker run --rm` containers exit** when the test closes the browser; each e2e run needs a
  fresh container (the script does this).
- **Test the UI by clicking**, not only via keyboard: dead tool buttons shipped because the e2e
  used shortcuts exclusively.
- **A file nobody reads is not a feature.** `reply` appended `agent-thread.jsonl` and no route or
  screen read it, so a human who submitted a review heard silence for three releases. The agent →
  human half of the loop needs a route *and* a UI, just like the human → agent half.
- **The start page printed the token** (`/api/config` returns it verbatim). Any screenshot of `/`
  leaked it — including one made by redline's own capture. Mask secrets at render time, keep the
  copy button working.

## 8. Working rules

- Ask before destructive actions (see global AGENTS.md). Creating/updating the `redline` container
  is normal work; say what is lost first if a review is in flight.
- Do not hand-edit `annotations.json`, `meta.json`, `review.md` or `annotated.png` in a session.
- Never commit screenshots, sessions, tokens or keys. `sessions/` and `target/` are gitignored.
- Keep the UI dependency-free (vanilla JS/CSS, served from the binary).
- Keep the API additive; breaking changes need a migration note in `CHANGELOG.md`.
- Update `docs/UI.md` (UI), `docs/DESKTOP.md` (desktop/deploy), `PLAN.md` when behaviour changes.
- Run Rust tests **and** the browser e2e before tagging.

## 9. Common recipes

```bash
# health / version
curl -s https://redline.ibolya.cz/health; docker exec redline /redline --version

# create a review from a URL (agent path, server-side capture)
TOKEN=$(cat /mnt/user/appdata/redline/token)
curl -s -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"url":"http://localhost:5016/button","title":"Button / default","note":"hover + disabled"}' \
  https://redline.ibolya.cz/api/capture

# read feedback (what an agent sees)
curl -s -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_feedback","arguments":{"session_id":"<id>"}}}' \
  https://redline.ibolya.cz/mcp

# session lifecycle
curl -X POST -H "Authorization: Bearer $TOKEN" https://redline.ibolya.cz/api/sessions/<id>/resolve
curl -X POST -H "Authorization: Bearer $TOKEN" https://redline.ibolya.cz/api/sessions/<id>/reopen
curl -X DELETE -H "Authorization: Bearer $TOKEN" https://redline.ibolya.cz/api/sessions/<id>

# rebuild/redeploy
REDLINE_IMAGE=ghcr.io/erikibolya/redline:browser REDLINE_DOMAIN=redline.ibolya.cz bash desktop/install-unraid.sh
```

## 10. Known limitations / next

No code signing (SmartScreen warns), no macOS/Linux desktop bundles, no touch toolbar, no
multi-select across groups beyond membership, and the `/` page is still a start screen rather than
a gallery. Since v0.8 text notes live in the inspector, not on the canvas — there is nothing to
size or style, and legacy text annotations are read-only history.
See `PLAN.md` § "Next" for the ordered list.

## 11. If you are the next session

1. `git pull`, `docker exec redline /redline --version` — confirm what is actually deployed.
2. Read `PLAN.md` § Next and `docs/UI.md` for the UI state.
3. Reproduce the bug or implement the feature **with the e2e open in another window**; then ship
   with a version bump, a tag, and a redeploy.
4. Leave this file better than you found it — especially § 7.
