# Changelog

## Unreleased

_(nothing yet)_

## 0.21.0 — 2026-10-30

- **The header is icons, and the status pills are gone.** `submitted`/`replied`/`working` sat in
  the header as GitHub-style pills and competed with the review title, although the rail card, the
  stage toast and the composer/STOP button already say whose turn it is; they are removed. Agent
  settings is now a solid gear instead of a `⚙` glyph, keyboard shortcuts is a keyboard instead of
  `?`, the theme button is a sun/moon SVG, and the two panel toggles are left/right frame icons
  with `aria-pressed` and a *Hide/Show* title, separated from the app tools by a hairline. Every
  icon is inline SVG, so none of it depends on the platform's emoji font. The e2e clicks both panel
  toggles, asserts the canvas bitmap follows the grid, swaps the theme icon, and checks the header
  has no `.pill` while the state still renders in the toast and the rail card.

- **The dev server follows the dev build.** Every push to `dev` now publishes
  `ghcr.io/erikibolya/redline:browser-dev` (plus a sha tag) from the same workflow as the Windows
  dev build. The `redline-dev` stack can pull that tag instead of building from the working tree,
  and a User Script or scoped Watchtower updates it every few minutes. The image includes
  Chromium, so capture works on dev too.
- **Agent concurrency control (foundation).** Sessions carry an optional `branch` declared by the
  agent (fallback `session/<id>`), the rail shows it after the relative time, and the server has
  branch ownership leases plus global/per-project run limits — defaults 2 global and 1 per
  project, editable in the new **Agent settings** dialog. A waiting claim keeps the branch lock
  but frees the execution slot. The bridge runs parallel claim workers against this API.
- **The agent bridge runs reviews in parallel.** `redline-agent` now runs up to four claim workers
  (`REDLINE_AGENT_WORKERS`, default 4) against `/api/agent/next`; each claim gets its own git
  worktree on the declared branch, heartbeats its lease, moves to `waiting` when the human has the
  ball (freeing the run slot but keeping the branch), and releases/removes a clean worktree when
  the round is resolved. The server's global/per-project limits and the one-owner-per-branch rule
  still apply; a dirty worktree is left for inspection.
- **Freehand strokes are numbered and listed again.** `renderInspector` read
  `annotation.start.x` for every non-text shape, but a pen stores `points`; the resulting
  `TypeError` aborted `mutate` before `render()`, so the live stroke stayed on screen with no
  badge and no inspector row (and could only be removed with Ctrl+Z). Shapes now get explicit
  labels per type, and the e2e draws a pen, asserts its row/number and badge pixel, submits it
  and checks the points in the server JSON.
- **Regression gate.** `docs/TESTING.md` defines the tool matrix, the bug-fix protocol (failing
  test first), the feature protocol and the release checklist. The e2e now draws every drawing
  tool (rectangle, ellipse, arrow, pen) instead of only clicking its button.
- **A reply is not the finish line.** The run card treated `agent_state=replied` as the end of the
  run, so it said **Run finished** while the review was still submitted, the canvas was frozen and
  the red STOP button was armed — and then the agent kept working for minutes. `reply` now only
  marks an answer: while the round is still submitted the card reads **Agent replied — waiting for
  it to finish** (amber, with the age of the last step), a later `progress` line turns it back into
  **Working**, and **Run finished** is reserved for a round that was handed back (`post_round`) or
  resolved. The server no longer freezes `working` after a reply on an open round; the browser e2e
  covers the answered-but-still-running state and the resume.
- **The rail says who has the ball.** A submitted round that no agent has picked up used to carry
  the **new** badge (which only meant "this browser has not looked at it yet"), so a review
  sitting in the queue read as the human's turn. It now reads **waiting for agent**; `new` is
  reserved for rounds that are back in the human's hands, and `working` still wins while an agent
  runs; a submitted round the agent has answered but not resolved now reads **replied**. The
  project header keeps a single count instead of a green waiting count beside the same total. The
  browser e2e asserts the badge and the missing header count.
- **The Servers page Remove button is readable again.** `.server-actions .btn.danger` kept the
  filled `.btn.danger` background and only overrode the label to `--danger`, so *Remove* was red
  text on a red button. It is now an outline in both themes (label contrast 9.2:1 dark, 6.3:1
  light), and the e2e measures that contrast instead of trusting the stylesheet.
- **Adding a server lives on the Servers page only.** The app menu and the tray no longer carry
  **Add a server…**; **Servers…** stays and its **Add server** button opens the same wizard. The
  wizard's **Use this computer** button is gone (the *This computer* row in the list is always
  present and cannot be removed) and is replaced by **Back**, which returns to the server list.
- **Each session is one feature.** The rail's ✓ now asks for confirmation and records
  `feature_confirmed_at`. The ✕ opens a feature-actions dialog: **Cancel feature** records
  `feature_cancelled_at` and keeps the screenshot/annotations, while **Delete files…** removes
  them permanently. A decided feature offers **Reopen** in the same menu. Both decisions close
  the session (`resolved`) and ↺ Reopen undoes them. Agents read the decision as `feature_state`
  (`pending`/`confirmed`/`cancelled`) from `GET /api/sessions`, MCP `get_feedback`,
  `wait_for_feedback` and `list_reviews`; RedLine itself never touches git. A new submission or
  agent round makes the feature pending again. Bulk mode gained **Confirm**; multi-delete stays
  in bulk **Delete**, and **Delete cancelled** purges only cancelled features; confirmed features
  stay as the log.

## 0.20.0 — 2026-10-30

- **A locked canvas also freezes the view.** While a submitted review waits for the agent, the
  wheel, zoom buttons, keyboard zoom and pan no longer move the screenshot, and a light scrim
  covers the stage; STOP removes both. The toolbar and status pill stay above the scrim.
- **Multiple server profiles in the desktop app.** The local **Servers** page (tray → *Servers…*)
  lists Production, Dev and *This computer*, shows each server's version/state and switches the
  window with one click. Profiles and tokens are stored side by side in `config.json`; switching
  only changes the active profile, so other servers stay signed in. Adding a server reuses the
  existing wizard, and the old `remote_url`/`token` config is read as the first profile.
- **The annotation list is a list, not a stack of broken frames.** In a submitted (read-only) round
  the separator between rows was a `border-top` on a rounded row, so it painted only the middle
  segment between the two rounded corners and every row looked like an open frame. The separator is
  now a full-width hairline and read-only rows are a plain `.ann-row.readonly` (no pointer cursor,
  no hover highlight, no delete button). The browser e2e measures the separator and the class.

- **Panels are resizable.** The two column borders and the line above the conversation in the
  editor are now drag handles: the session rail, the annotation inspector and the conversation
  each remember the size you drag them to (`localStorage`), the canvas is re-measured mid-drag, and
  the handles are keyboard-operable (`←/→` or `↑/↓`, Shift for bigger steps). Limits keep the stage
  at least ~360 px wide and stop the conversation from swallowing the annotation list. The browser
  e2e covers all three dividers, including a reload.
- **A finished round can no longer keep saying "working".** The agent's last narration line often
  arrives *after* `post_round` (review handed back) or `reply` (answer posted); `progress` used to
  set `agent_state=working` unconditionally, so the review kept pulsing a spinner with a green
  **Submit review** button and the human could not tell a live agent from a dead one. The server
  now only marks a round working while it is still submitted, the UI derives the state from
  `status` as well, and the run card shows how long ago the last step was ("Used 1 tool · last
  step 6m ago"). The narration event itself is still recorded.

- **The rail shows which review and which project an agent is working on.** While an agent runs a
  round the review row carries a spinner and a **working** badge (in place of the annotation
  count), its dot pulses, and its project header shows the same spinner and label — with a count
  when more than one review runs at once — even when the project is collapsed. The rail polls the
  session list every 8 s and redraws only when the set of running reviews changes; the indicator
  clears as soon as the agent replies or the human presses STOP. The browser e2e asserts both the
  appearance and the removal.

- **A new review starts in the project you were just working in.** Opening a review (or picking a
  project in the dialog) records it in the browser, and the next **Ctrl+V** or **New review** opens
  with that project already selected instead of the placeholder — on the editor and on the start
  page, and across reloads. The memory is per browser and per server origin; a project that no
  longer exists or is hidden from the picker falls back to the placeholder. The browser e2e covers
  both the preselect and its survival across a reload.

- **The submitted lock actually holds.** After a submit, Delete/Backspace, arrow-key nudging,
  undo/redo, the inspector's align/layer/group buttons, the note field and the keyboard tool
  shortcuts could still change annotations while the stage pill claimed the canvas was locked.
  All annotation writes now funnel through one `Editor.mutate` guard that refuses while the round
  waits for the agent (and while a history round is open), the drawing tools are `disabled` and
  dimmed instead of merely ignoring clicks, and the inspector's write toolbar hides with the note
  field. The browser e2e proves the refusal, not just the flag.

## 0.19.0 — 2026-10-29

- **Projects are repositories now.** The agent discovers git repos from OpenCode
  (`/api/project`) and by scanning its repo root, then syncs them to `POST /api/agent/repos`.
  Projects store a **portable relative key** (`redline`, `ActiUnit/Au.Net`) and an agent id, never
  an absolute mount path. The New review select is grouped into **Repositories** and **Other
  projects**, requires a choice, and hides empty legacy leftovers. `+ New project…` still creates a
  label without a repo.
- **Reviews run in their project's repo.** Before each run the agent resolves the project key
  under `REDLINE_AGENT_REPO_ROOT`, canonicalizes it and refuses a missing or escaping path instead
  of silently using the default repo. Legacy projects without a key keep using
  `REDLINE_AGENT_REPO`; error reviews get a reply and are resolved so they do not spin.
- **New review picks a project from the server's list.** The free-text project field (and its
  datalist, which only the editor had) is now a `<select>` fed by `GET /api/projects` on both the
  editor and the start page, so projects created by agents are selectable without typing.
  **+ New project…** reveals a name field; the project is created on first use.
- **One URL field, explained.** "Page URL" and "Or screenshot a URL" were two boxes doing almost
  the same thing. There is now one *Page URL*: metadata for an uploaded screenshot, or — when no
  image is attached — the page the server captures. "What should be checked?" is now "What should
  the agent check?" with a hint that the note becomes the first message.
- **The conversation shows the agent's run at a glance.** A card under the step lines reads
  **Working** with a pulsing dot while `agent_state` is `working` and tallies the round:
  **Used 4 tools: Edit and Shell**, from the `tool` names sent with `progress`. It settles to
  **Run finished** when the agent replies, counts only the current round, and stays out of the way
  when nobody has reported anything. The browser e2e covers it.
- **The window follows the configured server.** The tray said *Server: …* from `config.json`
  while the window could still be sitting on the local (or an old) server, so reviews were saved
  somewhere the agent never watches. The label now reads **Configured**, *Open RedLine* navigates
  the window to that server, and focusing a window left on another origin puts it back — except
  the setup page, which is served locally on purpose.
- **The submitted lock is a status pill, not a modal.** While a review was `submitted` a dark
  scrim plus a centred "Submitted — waiting for the agent / STOP unlocks the canvas" box covered
  the screenshot — while the same agent state already floated above the toolbar. The modal is
  gone (`#review-lock` and `.lock-box` removed); the stage status pill is centred where the box
  was, keeps the message short ("Agent is working — …" / "Waiting for the agent…"), carries the
  STOP hint as a tooltip and lets the screenshot show through. The `locked` guards in `app.js` are
  untouched, so editing stays blocked.

## 0.18.0 — 2026-10-29

- **Menu actions no longer run twice.** Tray items use their own `tray:` ids and the dispatcher
  ignores the same action within 400 ms, so update dialogs and the repository picker open once.
- **The connect wizard matches the app and can be left.** It uses the app's tokens and buttons, has
  a **← Back to sessions** link, a proper **Use this computer** button (disconnect, no token
  needed) and a clearer hint where the token comes from.
- **Bulk operations in the rail**: *Select* turns rows into checkboxes with a bar to resolve or
  delete several sessions at once; Escape leaves the mode.
- **Touch toolbar**: on coarse pointers the tools, chips and buttons grow to 40 px targets and the
  rail's per-session actions stop being hover-only.
- **Accessibility**: visible `:focus-visible` rings, accessible names on the canvas, composer,
  session rows and history rows, and the conversation is an `aria-live` region.
- **README rewritten** for 0.18 (desktop, server, agent bridge, the round loop, configuration).

## 0.17.0 — 2026-10-29

- **Submitting locks the canvas**: a dark overlay blocks editing and the composer is disabled
  while `submitted`; the primary button turns into a red **STOP**.
- **STOP** calls `POST /api/sessions/<id>/cancel`, unlocks the canvas and keeps the annotations.
  A round no agent has read yet is withdrawn from history; a round the agent has seen stays.
  The bridge watches the session every 2 s while the agent runs and kills it on cancel.


- **A round is archived when it is submitted**, not when the next one replaces it. History fills
  up immediately, keeps the source screenshot (`history/<stamp>-source.png`) and numbers rounds
  oldest = 1. Two submissions in the same second get unique stamps.
- **History is a round viewer**: `Current` is always the way back, every round is clickable and
  opens the source screenshot plus its annotations read-only. The separate ghost (`◫`), text diff
  (`Δ`) and open-PNG (`↗`) buttons are gone, and so is the ghost chip.
- **The conversation no longer shows the round screenshot**; the canvas and History own it.
- **Drafts**: unsent annotations and the composer text survive switching sessions. A new agent
  round clears the draft (the coordinates belong to the old image) and says so. On quit, the shell
  asks before discarding unsaved drafts (the webview marks the window title with `•`).


## 0.15.0 — 2026-10-29

- **Dev update channel in the desktop app.** Help/tray gains *Install latest dev build…* (the
  rolling `dev` prerelease) and *Revert to last stable version…* (the updater's comparator accepts
  a downgrade, so a dev build can go back). Every push to `dev` publishes a dev build
  (`.github/workflows/dev.yml`).
- **History rows** are compact and bounded; the empty-state class no longer leaks into a populated
  list.
- The browser e2e waits for the canvas to actually swap after a round, not only for the metadata.

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
