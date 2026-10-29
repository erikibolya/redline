# Agent instructions — redline

Read this before touching anything. It is the handoff document: what redline is, where it runs,
how to change and ship it, and the traps that have already cost time.

---

## 1. What this is, and where it stands

`redline` is a local visual review queue for human ↔ AI collaboration: an agent produces a
screenshot, a human draws on it in a browser or the desktop app, the agent reads the result as
JSON + Markdown + an annotated PNG.

**Current release: v0.19.0.** Live in production on holly:

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

# Ship a feature (dev is protected; never push it directly)
git push origin feat/my-feature
feature-ready "what changed and why"
# draft PR -> ready -> required CI -> auto-merge -> dev build

# Release (only after an explicit yes; main is protected)
gh pr create --base main --head dev --title "release: vX.Y.Z" --body "Release notes"
# wait for the required checks, merge the PR, then:
git checkout main && git pull --ff-only
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

On holly the container is owned by a Compose project (`/boot/config/plugins/compose.manager/projects/redline`)
together with `redline-agent`; `install-unraid.sh` must not be run there. See `docs/DESKTOP.md` §
"Compose deployment".

Env knobs: `REDLINE_DOMAIN`, `REDLINE_IMAGE`, `REDLINE_PORT`, `REDLINE_TOKEN`, `REDLINE_DATA`,
`REDLINE_PUBLISH_PORT`, `REDLINE_CHROME` (browser binary for capture).
Rust side: `REDLINE_PORT`, `REDLINE_DATA_DIR`, `REDLINE_TOKEN`, `REDLINE_CHROME`, `REDLINE_HOST`.

## 6. Architecture in one screen

- `redline-agent` — unattended bridge: watch submitted reviews → resolve the project's repo →
  `opencode run` → progress/reply/resolve. It syncs the git repos it can see (`POST
  /api/agent/repos`) and runs each review in its project's repo. Also embedded in the desktop app
  (tray → *Start local agent*); see `docs/LOCAL-AGENT.md`.
- `redline-core` — session folder store (`meta.json`, `annotations.json`, `review.md`,
  `annotated.png`, `history/`), project folders (`sessions/projects/<slug>/`, v0.10; optional
  `repo` key + `agent`), axum HTTP API, bearer/cookie auth, MCP JSON-RPC, headless capture
  (shells out to Chromium), annotation diffing, loopback HTTP client.
- Auth: token from `/data/token`; `Authorization: Bearer`, `redline_token` cookie (set by
  `/s/<id>?t=…` → 307), or `?t=`. `Sec-Fetch-Site: cross-site` is refused.
- MCP tools: `review_screenshot`, `capture_review`, `get_feedback`, `wait_for_feedback`,
  `list_reviews`, `list_projects`, `reply`, `progress`, `post_round`, `capture_round`, `resolve`.
  Resources: `redline://session/<id>/{annotated.png,review.md}`.
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
- **A filename is not a preview.** The New review drop zone said `image.png — 285 KB` for a pasted
  screenshot, so there was no way to tell *which* screen you had on the clipboard. Attach the
  picture itself (`objectURL`, revoked on replace); the name and size become a caption. Cap the
  height and let the modal scroll — a taller preview must not push *Create review* off-screen.
- **Never slice a clock out of an ISO stamp.** The inspector did `iso.slice(11, 16)`, which is UTC:
  a review submitted at 13:03 in Prague was shown to every reader as 12:03. Parse (`new Date`) and
  format with the browser's locale — `clockTime()` / `stampTime()` in `app.js`. Capture screenshots
  inherit the *container's* timezone (usually UTC), so a "fixed clocks" screenshot looks unfixed
  unless you capture from a browser started with `TZ=Europe/Prague`.
- **A project is not a repository.** RedLine projects began as labels; a label does not tell the
  agent where to work. Repo projects now store a **relative** key (`redline`, `ActiUnit/Au.Net`)
  plus an agent id. Never store an absolute mount path: the OpenCode server, the agent container
  and the Windows app can see the same repo through different roots. Resolve keys under
  `REDLINE_AGENT_REPO_ROOT`, canonicalize and refuse escapes/missing paths — a silent fallback to
  the default repo is worse than a visible error.
- **The OpenCode project API is experimental.** `/api/project` is useful for display names and
  active projects, but discovery must still scan the repo root for `.git`; dedupe the same repo
  seen through two mounts (`/mnt/user/...` and `/mnt/c/...`).
- **A push is not a done signal.** A feature branch becomes a PR only when the agent runs
  `feature-ready`; auto-merge then waits for the required checks. Merging on
  every push would ship checkpoints and half-finished work. Do not open a non-draft PR while the
  feature is still being worked on.
- **An empty-state class outlives the empty state.** `#history-list` was declared once in
  `index.html` as `inspector-empty` (22 px padding, centred text) and never lost the class when
  rows were injected, so a populated list still carried the empty state's dead space. Toggle the
  class with the content — a populated list needs its own layout class.
- **A lock flag is not a lock.** The submitted lock set `Editor.locked` and guarded the canvas
  pointer plus `setTool`, but Delete/arrow keys, undo/redo, the inspector's align/layer/group
  buttons and the note field wrote anyway — the e2e only asserted `window.redline.locked`, so it
  passed while the human could still edit a submitted round. Enforce a lock at the single
  mutation choke point (`mutate`, and the few paths that bypass it), and test the *refusal*: draw,
  delete, nudge and click the write tools, then assert nothing changed. A control that looks armed
  but silently refuses clicks reads as a broken lock; disable it visibly.
- **A spinner is a promise.** `progress` set `agent_state=working` unconditionally, so the agent's
  final narration line — written after `post_round` had handed the review back — re-armed the
  indicator. The card pulsed "Working" on a review the human already had, with a green Submit
  button beside it, and stayed like that until the next submit. A state flag must be derived from
  the round (`status == submitted`), not set by the last writer; the server refuses to revive a
  finished round, the UI checks both fields, and the card always shows the age of the last step so
  "is it really working?" has an answer that is not animation.

## 8. Working rules

- Ask before destructive actions (see global AGENTS.md). Creating/updating the `redline` container
  is normal work; say what is lost first if a review is in flight.
- **`dev` and `main` are protected.** Do not merge or push either directly. Push a feature
  branch; when the work is done run `feature-ready "summary"` (global agent tool; the repo copy
  still works). It creates the ready PR and auto-merge lands it in `dev`. A release is a PR from
  `dev` to `main`, merged after the required checks and an explicit human yes; tag afterwards.
  See `docs/AUTO-MERGE.md`.
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

# agent repo sync (what redline-agent does on startup and every 60 s)
curl -s -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"agent_id":"holly","repos":[{"key":"redline","name":"redline"}]}' \
  https://redline.ibolya.cz/api/agent/repos

# session lifecycle
curl -X POST -H "Authorization: Bearer $TOKEN" https://redline.ibolya.cz/api/sessions/<id>/resolve
curl -X POST -H "Authorization: Bearer $TOKEN" https://redline.ibolya.cz/api/sessions/<id>/reopen
curl -X DELETE -H "Authorization: Bearer $TOKEN" https://redline.ibolya.cz/api/sessions/<id>

# mark a feature ready (from its branch; no dirty files)
feature-ready "what changed and why"

# rebuild/redeploy
REDLINE_IMAGE=ghcr.io/erikibolya/redline:browser REDLINE_DOMAIN=redline.ibolya.cz bash desktop/install-unraid.sh
```

## 10. Known limitations / next

No code signing (SmartScreen warns), no macOS/Linux desktop bundles, no touch toolbar, no
multi-select across groups beyond membership, and the `/` page is still a start screen rather than
a gallery. Since v0.8 text notes live in the inspector, not on the canvas — there is nothing to
size or style, and legacy text annotations are read-only history.

Repo routing is single-agent: all agents watch the same queue, so run one `redline-agent` per
RedLine server for now. Project records already carry `agent`, and repo keys are portable, so a
claim/filter router can be added without migrating data.

See `PLAN.md` § "Next" for the ordered list.

## 11. If you are the next session

1. `git pull`, `docker exec redline /redline --version` — confirm what is actually deployed.
2. Read `PLAN.md` § Next and `docs/UI.md` for the UI state.
3. Reproduce the bug or implement the feature **with the e2e open in another window**; then ship
   with a version bump, a tag, and a redeploy.
4. Leave this file better than you found it — especially § 7.
