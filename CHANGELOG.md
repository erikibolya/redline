# Changelog

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
