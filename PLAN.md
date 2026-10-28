# PLAN — redline

## Vision

A zero-friction visual feedback loop: the AI agent produces a screenshot, the human annotates it
in the browser, and the agent receives a precise, machine-readable description of what to change.
It should work offline, on Windows/WSL, without accounts or external services.

## Scope

- Local single-user review queue (human + agent).
- Screenshot + drawing + text notes → JSON/PNG/Markdown.
- Separate from any product repository; portable to a personal GitHub account.
- No third-party Python dependencies.

Out of scope for now: multi-user collaboration, cloud hosting, issue-tracker sync, real-time
co-editing.

## Current state (MVP, commit 301eefd)

- `redline.py`: CLI (`serve/open/list/get/wait/remove`), file-based session store, stdlib HTTP
  server, annotation submission.
- Browser UI: session list + canvas editor with rectangle, arrow, pen, text, undo, clear,
  submit/resubmit.
- Output: `meta.json`, `annotations.json`, `annotated.png`, `review.md`.
- Tested: 3 unit tests (create/list, submission format, ID validation).
- Verified end-to-end: create session → open editor → canvas loads → submit → read JSON/Markdown.

## Architecture in one paragraph

A session is a folder under `sessions/<id>/`. The CLI writes `meta.json` and copies the screenshot;
the HTTP server serves the folder and a static editor. The browser POSTs annotations + a composited
PNG; the server writes the files and flips `status` to `submitted`. The agent polls the folder (or
API) and reads JSON. No database, no background process needed for the agent.

See `docs/ARCHITECTURE.md` for details.

## Milestones

### M1 — MVP (done)

- [x] CLI session creation, listing, reading, waiting, removal.
- [x] Static editor and session list.
- [x] Rect/arrow/pen/text annotations; undo/clear; submit.
- [x] JSON + annotated PNG + review.md output.
- [x] Shared `--sessions` folder.
- [x] Unit tests.

### M2 — Editing ergonomics

- [ ] Zoom/pan (fit width, 100%, wheel zoom) with correct coordinate mapping.
- [ ] Select/move/delete individual annotations; edit text notes inline instead of `prompt()`.
- [ ] Annotation list click-to-highlight; numbered markers.
- [ ] Color presets + line-width presets; remember last used.
- [ ] Keyboard shortcuts help panel; `Esc` cancels the current drawing.
- [ ] Session list auto-refresh (poll); filter by status; show annotation count.
- [ ] Copy review URL button; “open image in new tab”.
- [ ] Responsive toolbar on narrow windows.

### M3 — Agent integration

- [ ] `redline.py capture --url URL [--selector] [--viewport 1440x900] [--out shot.png]`
      (Playwright/Chromium; optional dependency, opt-in).
- [ ] `redline.py open --url URL` capture shortcut.
- [ ] `redline.py wait` JSON output already works; add `--fail-on-timeout` and progress.
- [ ] Before/after comparison session: original + follow-up screenshots side by side/diff.
- [ ] `redline.py export <id>` to a single Markdown review (for issue trackers).
- [ ] Optional checklist file per session (`checklist.json`) that the agent ticks off.

### M4 — Reliability & distribution

- [ ] Atomic writes (write temp + rename) and a per-session lock to avoid concurrent submit races.
- [ ] Validate `Content-Type`, max annotation count/text length, and malformed JSON gracefully.
- [ ] `pyproject.toml` + console entry point (`pipx install .`), versioning, changelog.
- [ ] Windows launcher (`review.cmd`) and a one-command `scripts/serve.cmd`.
- [ ] Optional token auth + `--host` warning when binding non-loopback.
- [ ] Golden/API tests using `http.client` or `urllib` against a live test server.
- [ ] Test on Windows Python 3.12 and WSL.

### M5 — Review workflow polish

- [ ] Session archive/cleanup command (`prune --older-than 14d`).
- [ ] Export annotated PNG into docs/issues; copy Markdown controls.
- [ ] Save/restore viewport/zoom per session.
- [ ] Theme (light/dark) for the editor.
- [ ] Integration recipe for Au.Net component reviews (link `COMPONENT-REVIEW.md`).

### M6 — Desktop app (v1.0, in progress)

Turns redline into a tray-resident application with an embedded server and an MCP endpoint, so
agents connect directly instead of talking to a terminal. Design and phase status live in
[`docs/DESKTOP.md`](docs/DESKTOP.md).

- [x] `redline-core`: store + HTTP API + token auth + MCP, 20 tests, no GUI dependencies.
- [ ] CLI parity for `open/list/get/wait/remove` against the running app.
- [ ] Tauri v2 shell: tray, hidden main window, embedded UI, single instance.
- [ ] Windows installer (NSIS/MSI, WebView2 bootstrap), autostart, updater.
- [ ] macOS/Linux packages and the GitHub Actions build matrix.
- [ ] `capture_review` (headless screenshot) and before/after comparison.

## Decisions

| # | Decision | Why |
|---|----------|-----|
| D1 | **Python stdlib only** | Runs on any machine with Python; no `pip`, no node build. |
| D2 | **File-based sessions, no DB** | Shared folders work across WSL/Windows; agent can read without the server. |
| D3 | **Separate repository** | No company coupling; push to personal GitHub. |
| D4 | **JSON + PNG + Markdown output** | JSON for exact coordinates/text; PNG for visual; Markdown for humans/issues. |
| D5 | **Localhost by default** | No auth/TLS; avoid accidental exposure. |
| D6 | **Canvas pixels as coordinates** | Stable across zoom/scaling; editor maps pointer positions. |
| D7 | **Submit over HTTP, not File System Access API** | Works in all modern browsers and across machines. |
| D8 | **No automatic “fix my note”** | The agent turns notes into code changes; the tool only captures intent. |
| D9 | **Tauri v2 for the desktop app** | One binary, ~10 MB installer, tray/autostart/updater plugins, reuses the existing canvas UI. Supersedes the Python-only shell. See `docs/DESKTOP.md`. |
| D10 | **Token auth on every route** | A port agents can reach must not be open to every local process or web page. Supersedes D5's "no auth"; bind stays loopback by default. |

## Known limitations / technical debt

- No zoom/pan; coordinates are natural pixels but editing at 100% can be awkward on large pages.
- Only undo/clear; no per-annotation selection or edits.
- Text notes use `window.prompt`, not an inline editor.
- Session list does not auto-refresh.
- No screenshot capture; the agent must supply a file.
- No auth/TLS; localhost-only.
- Single writer; concurrent submissions last-write-wins.
- `annotated.png` is only written when the client sends `png` (the official UI always does).
- API is minimal; no pagination (fine for local use).
- Some unused imports were removed; keep the stdlib-only rule.

## Testing plan

- Keep `python3 -m unittest discover -s tests -t . -v` green.
- Add tests for: wait timeout, malformed POST, missing session, `--sessions` separation, text-note
  coordinates in `review.md`, static route traversal rejection, annotated route validation.
- Add an HTTP integration test that starts `ThreadingHTTPServer` on an ephemeral port.
- Manual checklist for the UI: load image, each tool, undo/clear, submit, resubmit, mobile width.

## Release plan

- Published: **https://github.com/erikibolya/redline** (public, MIT), default branch `main`.
- Tag `v0.1.0` after M1 (current), `v0.2.0` after M2, etc. (`git tag -a vX.Y.Z -m … && git push --tags`).
- No package publication required; `git clone` + `python redline.py serve` is the install.
- If `pipx` packaging lands (M4), publish to PyPI only if the human wants it.

## Definition of done (feature)

- Works on Windows and WSL with no extra dependencies.
- Documented in README + relevant `docs/` page; AGENTS.md updated if agent workflow changes.
- Tests cover the new behavior; old tests still pass.
- JSON schema remains backward-compatible or a migration note is added.
- No secrets/screenshots committed.
