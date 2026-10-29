# Changelog

## 0.14.1 — 2026-10-29

Fixes found by using rounds for real.

- **The canvas actually switches to the new round.** `/sessions/<id>/image` is a stable URL while
  the file behind it changes, so the browser kept showing the previous screenshot; it is now
  cache-busted with the image name.
- **History updates.** The count is loaded with the session and refreshed when a round arrives,
  instead of staying at 0 until the disclosure is opened.
- **No invented messages from the bridge.** It no longer posts "check `git status`" or resolves on
  the agent's behalf; it logs what happened and leaves the session in the state the agent left it.
  A short grace period covers the CLI exiting just before the last tool call lands.
- **Rounds keep running.** The bridge keys its "already handled" set by submission, not by session,
  so the same review can be submitted again after a new round.
- `post_round` clears `resolved_at` when it reopens a session.

## Unreleased

- **History rows are compact and bounded.** Each round was rendered as a full annotation card
  inside the empty-state wrapper, whose 22 px padding and centred text left most of the section
  white. The list is now a stack of small bordered rows (stamp + count, ghost / diff / screenshot
  buttons) and drops the empty-state class once it has content.
- **The canvas status pill moved below the toolbar.** It sat at `top: 14px`, the exact place the
  tool bar floats, so while a review was `submitted` the "Agent is working…" pill covered the
  tools. `--stage-toolbar-top` / `--stage-toolbar-height` now describe the bar once and the pill
  is placed under it; the e2e checks the two rectangles do not intersect.
- **A submitted review opens again.** `renderHeader()` read `this.events.length` while
  `this.events` was still `undefined` (the conversation poll had not answered yet), so a fresh
  load of a submitted session threw and the canvas said "Could not load this session". The lists
  start empty in the constructor; the e2e reloads the page while submitted to keep it that way.
- **Header icon buttons are square.** `.btn.icon` is a fixed 32 × 32 px box, so `?`, `☀` / `☾`,
  `☰` and `⌥` no longer produce buttons of different widths that read as squashed rectangles.

## 0.14.0 — 2026-10-29

One task is one session: the agent's follow-up no longer opens a new review.

- **`post_round` / `capture_round`** (MCP): post the next screenshot (uploaded or captured) into an
  existing review. The previous round is archived, the canvas image is replaced, annotations are
  cleared and the session returns to the human (`open`), so the rail keeps one item and History
  fills up.
- **The editor follows the round**: when the poll sees a new image, the canvas reloads it and the
  conversation shows the round with its thumbnail.
- **Waiting state on the canvas**: while `submitted`, a small pill at the top of the stage says
  "Waiting for the agent…" or the latest activity line; it never blocks annotation.
- The bridge prompt now forbids creating a new review for a follow-up round and tells the agent to
  hand the session back instead of resolving when it posts a screenshot.

## 0.13.4 — 2026-10-29

- **New-message indicator in the conversation.** When you scroll up and a reply or activity line
  arrives, the header shows `↓ N new messages`; click it (or scroll back down) to clear. The log
  no longer jumps while you read.

## 0.13.3 — 2026-10-29

- **Conversation keeps your place.** The 5 s poll only re-renders when something changed and only
  follows the tail when you are already at the bottom; scrolling up to read history no longer
  yanks you back.
- **No more stdout dumps in the conversation.** When the agent forgets to report, the bridge posts
  one short note (changes are in the working tree) and resolves the review; the transcript stays
  in the agent's server log. Failure messages are a single line, not a pasted stderr.
- The bridge prompt now makes the closing `reply` + `resolve` explicit.

## 0.13.2 — 2026-10-29

- **New review** — the drop zone shows a thumbnail of the attached image (dropped, chosen or
  pasted) instead of only `image.png — 285 KB`. You can now see that you pasted the right screen
  before creating the review. The caption keeps the file name and size plus *click to replace*;
  the preview is capped in height and the dialog scrolls on short screens.

## 0.13.1 — 2026-10-29

- **Desktop app** — *Check for updates…* moved from the **RedLine** menu to **Help**, where people
  look for it. The tray menu keeps its own entry (a tray has no Help submenu).
- **Editor** — conversation timestamps are shown in the reader's timezone. The API sends RFC 3339
  UTC and the UI sliced the clock out of the string, so a submission made at 13:03 in Prague read
  “12:03”. Relative times (“5m ago”) and day labels were already local and are unchanged.

## 0.13.0 — 2026-10-29

The agent side becomes part of the product.

- **`redline-agent`** — a Rust bridge (binary + library) that turns a submission into an OpenCode
  run: polls submitted reviews, reads the feedback, runs `opencode run` in the repository
  (optionally against an already running OpenCode), and lets the run report `progress`, `reply`
  and `resolve`. Replaces the Python prototype.
- **Desktop app** — tray menu: *Choose agent repository…*, *Start local agent*, *Stop local agent*.
  Settings live in `agent.json`; the agent talks to the embedded server in standalone mode or to
  the configured remote server.
- **`docs/LOCAL-AGENT.md`** — server-on-a-NAS vs everything-on-one-PC, headless and container
  recipes.
- Release assets: `redline-agent` for Windows and Linux.

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
