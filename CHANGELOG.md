# Changelog

## 0.12.0 — 2026-10-29

Silence after Submit is the bug this release fixes.

- **Waiting states in the conversation**: "Waiting for an agent" (nobody has read the submission
  yet, with its age and the MCP calls that would read it), "Agent received this" with the time,
  then the live activity line while the agent works.
- **`REDLINE_WEBHOOK_URL`**: the server POSTs a small JSON body on every submission so a host can
  wake an agent or a relay. Plain HTTP, fire-and-forget, never blocks the submission.
- Fixed: `[hidden]` was overridden by component `display` rules, so the ghost chip (and any future
  hidden button) showed up with an empty label.

## 0.11.0 — 2026-10-29

Small daily-use wins, plus the comparison the review loop was missing.

- **Ctrl+V** opens the New review dialog with the clipboard image attached; no file on disk.
- **Gallery on `/`**: the last twelve reviews as cards with screenshot, project, time and status.
- **Diff overlay**: the History panel's `◫` ghosts a previous round's annotations behind the
  current ones (35 % opacity) with a `Ghost · <stamp>` chip in the header to clear it.
- Fixed: the New review dialog was never wired on the editor page, so it did nothing there.

## 0.10.0 — 2026-10-29

The rail becomes a list of projects, so a conversation has a home and a history.

- **Projects are folders**: `sessions/projects/<slug>/project.json` with their sessions one level
  deeper. `GET`/`POST /api/projects` (counts, waiting, last activity), a `project` argument on
  `review_screenshot` / `capture_review`, a filter on `list_reviews`, a new `list_projects` MCP
  tool and `redline open --project`.
- **Inbox, not migration.** Sessions created without a project stay flat and are labelled Inbox on
  read; no data is moved.
- **Rail grouping**: collapsible project headers with waiting/total counts, filters and search
  still apply inside each project, day separators only for long lists.
- The New review dialog gains a project field with autocomplete.

## 0.9.1 — 2026-10-29

- Hide the agent-state pill when there is no state (author `display` was overriding `[hidden]`).

## 0.9.0 — 2026-10-29

The review becomes a conversation: the agent announces itself, and you submit from the chat.

- **Agent state you can see.** `get_feedback` stamps `seen_by_agent_at` and sets `received`; the new
  MCP `progress` tool narrates each step (`working`, with a message and an optional tool name in
  `agent-events.jsonl`); `reply` sets `replied`. The header shows the state, the conversation shows
  the activity feed.
- **Conversation + composer.** The inspector's lower half is a timeline — agent request, your
  prompt, activity events, replies — with a prompt box and **Submit review** at the bottom. The
  top-right Submit button is gone.
- **The prompt travels with the submission** (`meta.prompt`, shown in `review.md` and returned by
  `get_feedback`).
- `GET /api/sessions/{id}/events` exposes the activity feed.

## 0.8.0 — 2026-10-29

The review becomes a conversation: text moves off the canvas and onto the marks.

- **Notes on shapes.** Every rectangle, ellipse, arrow and freehand mark carries an optional note,
  written in the inspector right after drawing. The text tool is gone; legacy text annotations
  still render, read-only.
- **Numbers everywhere.** Shapes get a numbered badge on the canvas, in the annotation list, on
  `annotated.png` and in `review.md`, so the agent can match a paragraph to a region.
- **Ellipse tool** (`E`) — the missing "circle that" gesture.
- **RedLine branding** in the UI, CLI banner, desktop menus and docs (display only: binary, image,
  container and paths keep `redline`).
- `docs/UX-PLAN.md`: projects, chat flow and agent status — the rest of the 1.0 rework.

## 0.7.0 — 2026-10-29

Found by pointing redline at itself: an agent reply had nowhere to appear, and the start page
printed the access token.

- **Agent replies are visible in the editor.** The MCP `reply` tool wrote `agent-thread.jsonl`,
  but no route and no screen read it — a human who submitted a review saw nothing. New
  `GET /api/sessions/{id}/thread` plus an "Agent replies" panel in the inspector, polled every
  5 s, so an answer appears without reloading.
- **The token is masked on the start page.** `GET /api/config` returns the bearer token and the
  page rendered it, so a screenshot of `/` (which redline's own capture produces) leaked it. The
  block now shows `Bearer ••••••`; "Copy config" still copies the real JSON.
- Tests: `agent_replies_are_readable_by_the_human` (20 API tests; 50 total).

## 0.1.0 — 2026-10-28

Initial MVP. Renamed from the working title `au-visual-review` to **redline**; published publicly at
https://github.com/erikibolya/redline under the MIT license.

- Stdlib Python CLI: `serve`, `open`, `list`, `get`, `wait`, `remove`.
- File-based sessions under `sessions/<id>/` (gitignored).
- Browser session list and canvas editor.
- Rectangle, arrow, pen and text annotations; undo/clear; submit/resubmit.
- Output: `meta.json`, `annotations.json`, `annotated.png`, `review.md`.
- Shared `--sessions` directory for Windows browser + WSL/agent split.
- Unit tests for session creation, submission output and ID validation.
- Documentation: README, AGENTS.md, PLAN.md, architecture/API/workflow docs.
- MIT license.

## Unreleased

See `PLAN.md` M2–M5: zoom/pan, editable annotations, session auto-refresh, screenshot capture,
atomic writes, packaging, auth.
