# Agent instructions — redline

This repository is a local visual-review queue. The human annotates screenshots; the agent creates
sessions, opens them, and reads the submitted JSON. Read this file before changing the tool.

## Current state

- **MVP complete** (tag `v0.1.0`, commit `301eefd`): stdlib Python server/CLI, session store,
  browser editor, session list, 3 unit tests. No third-party dependencies.
- Separate git repository on `main`, published to **https://github.com/erikibolya/redline**
  (public, MIT). Project name: **redline** ("Redline the screenshot. The agent writes the fix.").
- The tool is used by the Au.Net component review workflow (`COMPONENT-REVIEW.md` there), but this
  repo has no dependency on Au.Net.

## Publishing (personal GitHub)

The remote is already configured; publishing is a normal `git push`:

```bash
git remote -v                    # origin -> https://github.com/erikibolya/redline.git
git status -sb                   # confirm branch is main and clean-ish
git push origin main             # or `git push` once upstream is set
```

Auth details (do not redo this setup unless it breaks):

- **`gh` is not on `PATH` by default.** It lives at `~/.local/bin/gh`; use
  `export PATH="$HOME/.local/bin:$PATH"` first, or the absolute path.
- Logged in as **`erikibolya`** over **HTTPS**; `gh auth setup-git` already installed the credential
  helper, so `git push` works without prompts. Verify with `gh auth status`.
- The token is stored in plain text in `~/.config/gh/hosts.yml`. **Never copy it into the repo,
  a commit, or chat.** Run `gh auth logout` on shared machines.
- To re-authenticate without a browser on the host: `gh auth login --web --git-protocol https`,
  then complete the device code at https://github.com/login/device. On a headless box the browser
  cannot be opened automatically — read the one-time code from the output and enter it manually.
- Useful repo commands: `gh repo view --web`, `gh run list`, `gh release create vX.Y.Z`,
  `gh issue list`. Pushing tags (`git push --tags`) publishes releases.

Repository settings already applied: description "Redline the screenshot. The agent writes the
fix. A local, dependency-free visual review queue for human ↔ AI collaboration." and topics
`ai-agents, human-in-the-loop, screenshot, annotation, visual-review, feedback-loop,
developer-tools`.

**Before pushing, always check** that `sessions/`, screenshots, customer data and `__pycache__/`
are not staged (`git status`); they are gitignored — keep it that way.

## Issue tracking

Work is mirrored from `PLAN.md` onto GitHub so progress is visible publicly:

- Milestone **`v0.2.0 — Editing ergonomics`** (M2) holds nine open issues, `#1`–`#9`, one per M2
  checklist item. Close an issue only when its "Done when" criteria are met on Windows **and** WSL.
- Use `gh issue list --milestone "v0.2.0 — Editing ergonomics"` to see what is left;
  `gh issue close <n> --comment "Shipped in <commit>"` when a change lands.
- When starting a later milestone (M3–M5), create the matching milestone and issues first so
  `PLAN.md` and GitHub stay in sync; keep the same one-issue-per-checklist-item shape.
- Releases are cut from tags: `gh release create vX.Y.Z --title … --notes-file … --verify-tag`,
  and `CHANGELOG.md` gets the entry in the same commit as the version bump.

## Environment

- Python 3.10+ on Windows/WSL/Linux. Use `python3` on WSL/Linux and `python` on Windows.
- Browser: Chrome/Edge/Firefox; HTTP submit works everywhere.
- Default server: `127.0.0.1:8788`; default sessions: `./sessions`.

## Desktop app work (branch `desktop`, v1.0)

`docs/DESKTOP.md` is the design record. The Rust backend lives in `desktop/`:

| Path | Responsibility |
|------|----------------|
| `desktop/crates/redline-core/` | Store, HTTP API, token auth, MCP. No GUI dependencies. |
| `desktop/crates/redline-server/` | Headless `redline serve` / `token` / `mcp-config` binary. |
| `desktop/ui/` | Editor assets copied from `static/` (single source until v1.0). |
| `desktop/app/` | Tauri v2 shell (Windows build; not yet scaffolded). |

Build and test (this box has no `cc`; see DESKTOP.md for the one-time Zig/musl setup):

```bash
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
cd desktop && cargo test --target x86_64-unknown-linux-musl
cargo run --target x86_64-unknown-linux-musl -p redline -- serve --port 8899 --ui ui
```

Smoke test the MCP path without a GUI:

```bash
TOKEN=$(./target/x86_64-unknown-linux-musl/debug/redline token)
curl -s -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' localhost:8899/mcp
```

Rules that keep the migration honest: **Python is frozen** (no new features on `redline.py`),
the session folder format is unchanged, and no v1.0 release happens before CLI + UI parity.


## Standard workflow

1. **Prepare a screenshot.** Capture the page/state at a stable viewport. Save it wherever the
   review pack lives (e.g. `review/14-button/default.png`).
2. **Create a session:**
   ```bash
   python3 redline.py open review/14-button/default.png \
     --title "Button / default" \
     --url "http://localhost:5016/button" \
     --note "Check hover, focus and disabled" --json
   ```
   The JSON contains `id`, `review_url` and `dir`. Use `--sessions DIR` if the browser runs on a
   different machine.
3. **Open the URL** in the browser/Review pane. If browser tools are available, open the printed
   `review_url`; otherwise ask the human to open it. Keep the page server running.
4. **Wait** for submission:
   ```bash
   python3 redline.py wait <id> --timeout 600 --json
   ```
   Exit code 0 = submitted; 2 = timeout; 1 = not found.
5. **Read the result:**
   - `sessions/<id>/annotations.json` — source of truth (types, coordinates, text).
   - `sessions/<id>/annotated.png` — visual confirmation.
   - `sessions/<id>/review.md` — summary; text notes include coordinates.
6. **Implement** each text note as a checklist item. Use marked regions (`rect`/`arrow`/`pen`) to
   locate the affected UI. Never edit the user's annotations.
7. **Confirm** with a fresh screenshot/session; then optionally
   `python3 redline.py remove <id> --yes` for obsolete sessions.

## Screenshot conventions

- Use the full page or a focused component region; include the surrounding layout when spacing is
  being reviewed.
- Keep one state per session (default, hover, disabled, etc.) unless the note explicitly covers a
  matrix.
- For Au.Net docs pages, capture at a fixed viewport (e.g. 1440×900) and mention the URL and state
  in `--title`/`--note`.
- Canvas coordinates are in the screenshot's natural pixels. If the browser scales the image on
  screen, the editor already maps pointer coordinates back to natural pixels.

## Code map

| File | Responsibility |
|------|----------------|
| `redline.py` | CLI (`serve/open/list/get/wait/remove`), session storage, annotation submission, HTTP server. |
| `static/list.html` | Session list page; fetches `/api/sessions`. |
| `static/index.html` | Canvas editor; loads session, drawing tools, submits JSON + PNG. |
| `tests/test_redline.py` | Unit tests for session creation/submission/ID validation. |
| `docs/API.md` | HTTP + annotation schema. |
| `docs/ARCHITECTURE.md` | Design, data flow, limitations. |
| `docs/WORKFLOW.md` | Human/agent review protocol. |
| `PLAN.md` | Roadmap, decisions, known issues. |

## Rules

- **Do not hand-edit** `annotations.json`, `meta.json`, `annotated.png` or `review.md`; the human's
  submission is authoritative.
- Keep `sessions/` out of git (already ignored). Never commit screenshots or customer data.
- Do not expose the server publicly; add auth/TLS before any remote use.
- Keep the tool dependency-free (stdlib only) unless the human explicitly approves a dependency.
- Keep the JSON/API stable; additive fields are fine, breaking changes need a migration note.
- Update `README.md`, `docs/API.md` and `PLAN.md` when behavior or the schema changes.
- Run `python3 -m unittest discover -s tests -t . -v` after code changes; add tests for new
  endpoints/CLI behavior.

## Common recipes

```bash
python3 redline.py serve --open
python3 redline.py serve --port 8790
python3 redline.py list --json
python3 redline.py get <id> --json
python3 redline.py remove <id> --yes
python3 redline.py open shot.png --sessions ./shared-sessions --json
```

Troubleshooting:

- **Address already in use** — another server is running; stop it or use `--port`.
- **Session not found** — check `--sessions` points at the same folder as the server.
- **Screenshot does not load** — confirm the file copied into `sessions/<id>/` and the server is
  running; the editor shows an error banner on failure.
- **Old standalone tool** — the previous static file was removed from Au.Net; this repo replaces it.

## Known limitations (see PLAN.md)

No zoom/pan or per-annotation editing; text is entered via prompt; no auto-refresh of the session
list; no screenshot capture command; single-user local queue; no auth. These are planned, not bugs
to “fix” silently.

## Host access (holly)

This checkout is on the Unraid host, and the agent container has the Docker socket plus `/mnt/user`.
That is root-equivalent: read the global rules in `~/.config/opencode/AGENTS.md` and **ask before any
destructive action** (deleting data, touching containers other than `redline`, changing Traefik or
other services, `/boot`, force-pushing history).
