# PLAN — redline

## Vision

A zero-friction visual feedback loop: the agent produces a screenshot, the human redlines it, and
the agent receives a precise, machine-readable description of what to change. Offline, no
accounts, runs where the agents run.

**Shipped and in production:** `v0.22.0` on holly — `https://redline.ibolya.cz`, container
`ghcr.io/erikibolya/redline:browser`, MCP at `/mcp`, data in `/mnt/user/appdata/redline`.

## What exists today

| Area | State |
|---|---|
| Agent API | MCP over HTTP (`review_screenshot`, `capture_review`, `get_feedback`, `wait_for_feedback`, `list_reviews`, `list_projects`, `reply`, `progress`, `post_round`, `capture_round`, `resolve`) + plain JSON API + CLI; branch claims, leases and global/per-project run limits |
| Capture | Headless Chromium server-side, from MCP or the UI (`/browser` image) |
| Editor | 3-column shell: resizable session rail, zoom/pan canvas, inspector with a conversation + composer (v0.9); rect/ellipse/arrow/pen; numbered, per-shape notes (v0.8); round history with a read-only viewer and diff (v0.17); submitted lock + STOP; move, nudge, delete, undo/redo, grouping, z-order, align/distribute; resizable panels |
| Sessions | open → submitted → resolved; repository-backed projects in the rail (v0.10, repo keys v0.19); search, filters, badges, "new" dots; feature confirm/cancel stamps; branch per feature; delete / mark done / reopen / purge cancelled; start-page gallery |
| History | every resubmission archives the previous round; diff endpoint (added/removed/changed) |
| Desktop | Tauri v2 tray app: standalone or thin client, server profiles, menu bar, setup wizard, auto-update, port pinned to 8788; local agent toggle with the parallel bridge (`redline-agent`, v0.13/v0.21) |
| Distribution | GitHub Releases (installer, CLI, static Linux binary), GHCR images (`latest`, `browser`), rolling `dev` channel and `browser-dev` image |
| Tests | 91 Rust tests, 165-check browser E2E in CI on Linux + Windows |

## Milestones

### M1 — Python MVP (done, frozen at `v0.1.0`)
CLI + localhost editor + file sessions. Kept in the repo for reference; no new features.

### M2 — Desktop shell + agent surface (done, v0.2–v0.3)
Rust core (store/API/MCP), CLI parity, Tauri shell, auth, container deployment, Traefik.

### M3 — Editing depth (done, v0.3–v0.5)
Zoom/pan stage, selection model, inline text, undo/redo, rail with filters, layers, groups,
alignment, per-note text size, history + diff.

### M4 — Session hygiene (done, v0.6)
Resolve / reopen / delete, `Active` vs `Done` views, purge finished sessions, rail row actions.

### M5 — v1.0 hardening (next)
- [ ] **Code signing.** Pipeline is ready (`WINDOWS_CERT_PFX` / `WINDOWS_CERT_PASSWORD`). Needs a
      certificate: Certum OSS (free, PFX) or SignPath (free, server-side). Identity verification is
      the only manual step.
- [ ] **Updater hardening**: keep the signing key backed up outside the array; verify a real
      0.6 → 0.7 upgrade on Windows; add an "update available" tray badge.
- [ ] **macOS + Linux desktop bundles** (CI matrix; macOS needs an Apple developer id for a clean
      Gatekeeper pass — otherwise document the right-click-open dance).
- [ ] **Accessibility pass**: focus rings, ARIA on the rail/menus, keyboard-only review flow.
- [ ] **Docs**: screenshots in the README, a 60-second "capture → annotate → read" GIF.

### M6 — Beyond 1.0 (ideas, unordered)
- [ ] **Diff overlay in the editor**: ghost the previous round's annotations behind the current
      ones, instead of reading the JSON diff.
- [ ] **Session gallery** on `/` with thumbnails (currently a start screen).
- [ ] **Text plates**: semi-transparent background behind text notes for legibility over busy UIs.
- [ ] **Touch toolbar**: larger targets, no hover-only affordances (the rail actions currently
      appear on hover).
- [ ] **Bulk operations from the rail**: multi-select sessions, bulk resolve/delete.
- [ ] Optional **TLS upstream**: let the app talk to an HTTPS server with its own client (today the
      desktop *webview* handles TLS; the CLI/setup probe is HTTP-only).

## Decisions

| # | Decision | Why |
|---|----------|-----|
| D1 | Stdlib Python for v0.1 | Zero install friction; still the frozen legacy build |
| D2 | File-based sessions, no DB | Portable, inspectable, agent can read without the server |
| D3 | Separate repository, MIT | No company coupling |
| D4 | JSON + PNG + Markdown output | Exact data for agents, visual for humans, prose for issues |
| D5 | ~~Localhost only, no auth~~ | Superseded by D10 |
| D6 | Canvas pixels as coordinates | Stable across zoom and scaling |
| D7 | Submit over HTTP | Works everywhere, no File System Access API |
| D8 | No automatic "fix my note" | The tool captures intent; the agent implements |
| D9 | **Tauri v2 for the desktop app** | ~10 MB installer, tray/autostart/updater plugins, reuses the web UI |
| D10 | **Token auth on every route** | A port agents can reach must not be open to every local process |
| D11 | **Server runs next to the agents** (container on holly) | Reviews can be created while the desktop is off; MCP stays on loopback |
| D12 | **No JS build step** | The whole client is `app.js` + `app.css`, served from the binary |
| D13 | **Browser E2E via CDP in CI** | Three real bugs (text tool, duplicate notes, dead tool buttons) were invisible to Rust tests |
| D14 | **Archive on resubmit** | A re-review must be able to compare rounds instead of overwriting them |

## Next (ordered)

0. **Video reviews** — phases 1–3 are in (`docs/VIDEO.md`): media core, browser upload, canvas
   playback, timecoded annotations and ranges, ghosts, zoomable timeline, contact sheet + full
   frames, new video rounds, history playback and `post_round` video. Phase 4 is the release
   pass.
1. **UX rework for 1.0** — see `docs/UX-PLAN.md`: projects instead of requests, a chat flow with a
   composer and visible agent status, notes attached to shapes instead of a text tool, RedLine
   branding. Ships as 0.8 → 0.10, before the rest of this list.
1. Code signing (blocked on a certificate — the pipeline is ready).
2. Diff overlay in the editor; it makes re-reviews much faster than reading JSON.
3. Text plates for legibility.
4. Session gallery on `/`.
5. Touch toolbar + hover-free rail actions.
6. macOS/Linux bundles.
7. Multi-agent repo routing: `agent_id` and portable repo keys are already stored.

## Known limitations

- No code signing: SmartScreen warns on the installer.
- Capture needs the `:browser` image (~768 MB); the slim image returns an actionable error instead.
- No TLS client in the CLI/wizard (HTTP only); the webview does TLS.
- No multi-user model: one token, one queue. Concurrent submissions are last-write-wins.
- Repo routing is single-agent: all agents watch the same queue; a claim/filter router is next.
- **Feature confirmation is a signal, not a git action.** RedLine records `feature_state` and
  queues a separate **ship job** (v0.23); the bridge claims it only when it advertises the job,
  and the agent runs the repository's own PR/merge workflow and reports through `ship_status`.
  RedLine still has no server-side GitHub token and never runs git itself. A review run is not a
  shipment: it pushes the branch and replies, it must not open a PR.
- No server-side pagination; the rail renders everything it gets.
- Text notes have a fixed font family, one size per note.

## Testing plan

- `cargo test --workspace` — 110 tests: store layout/atomics, HTTP auth/CSRF/traversal, MCP tools,
  capture error paths, history archival, diff semantics, feature confirmation/cancel, agent
  settings/claims/leases/limits/next, parallel bridge claims in separate worktrees, path/config
  helpers, video media/limits/frames/rounds/timecodes, CLI end-to-end.
- `desktop/scripts/e2e.sh` — 204 checks in real Chromium: every drawing tool is actually drawn
  (rectangle, ellipse, arrow, pen) and checked for its inspector row/number and badge pixel; the
  pen is also moved and undone through its `points` path; coordinate mapping at 46 % zoom, drag,
  undo, notes, submit, multi-select, align, grouping, z-order, text size, history, diff, the
  replied-but-still-running state, feature confirm/cancel/delete menus, the confirm → ship queue,
  MCP `ship_status` merged/failed, the rail's PR link and `ship failed ↻` retry, row delete,
  cancelled-only purge, the All filter, agent settings/branch display, rail reopen and bulk
  confirm, and the whole video path (recorded WebM upload, playback, timecoded draw, ghost
  opacity, range, timeline zoom, contact sheet + two frames, new video round, archived history
  playback).
- CI runs both on every push; releases are cut from tags only after green. The tool matrix,
  feature/bug protocol and release checklist live in `docs/TESTING.md`.

## Definition of done

- Works on Windows and WSL/Linux with no extra dependencies.
- Rust tests and the browser E2E are green; the e2e covers the interaction you changed and the
  release checklist in `docs/TESTING.md` is complete.
- Documented in `AGENTS.md`/`docs/` + `CHANGELOG.md`; API changes are additive or have a note.
- Tagged, released, and deployed to holly with the version verified in `docker exec redline /redline --version`.
