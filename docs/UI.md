# UI — editor shell (v0.3)

Status: in progress. Replaces the v0.2 single-page canvas and session list.

## Goal

Make redline feel like a drawing tool rather than a form: sessions on the left, image in the
middle, properties on the right, and the agent-facing prose out of the way until you submit.

Reference points: Photoshop / Illustrator / Figma shell — persistent left navigator, floating tool
options, right inspector, status bar. We borrow the layout, not the feature set.

## Layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ header:   redline · <session title>          ⚙ ⌨ ☀ │ ▤ ▥   [review actions]   │
├──────────────┬───────────────────────────────────────────────┬───────────────┤
│ left rail    │  options bar (floating, top-centre)           │ right panel   │
│ 264px        │  [V][R][A][P][T]   colour   width   zoom      │ 300px         │
│ ┌──────────┐ │                                               │ ┌───────────┐ │
│ │ + New    │ │                                               │ │Annotns (3)│ │
│ │ search   │ │              image (zoom / pan)               │ │ 1 ▭ …     │ │
│ │ chips    │ │                                               │ │ 2 ↗ …     │ │
│ │          │ │                                               │ │ 3 T …     │ │
│ │ session  │ │                                               │ └───────────┘ │
│ │ session  │ │                                               │ ▸ Message to  │
│ │ session  │ │                                               │   agent       │
│ └──────────┘ │                                               │               │
├──────────────┴───────────────────────────────────────────────┴───────────────┤
│ status bar:  zoom 87% · fit · 100%      1440×900      482, 260      id       │
└──────────────────────────────────────────────────────────────────────────────┘
```

Columns collapse: `[` toggles the left rail, `]` the right panel. Both states persist in
`localStorage`. The canvas always fills what is left — no fixed page width, no centred 1000 px
column.

### The header is icons, not badges (unreleased)

- The header's right side is one row of inline-SVG icon buttons with a tooltip and an `aria-label`
  each: **agent settings** (a solid gear), **keyboard shortcuts** (a keyboard, not a `?`), the
  **theme** toggle (sun/moon, showing the theme it switches to), then a hairline, then the two
  **panel** toggles (a frame with the left or right column marked). The `☰`/`⌥`/`?`/`⚙` text
  glyphs depended on the platform's emoji font and read as decoration; a gear that renders as an
  outline at 17 px does not say "settings".
- The panel toggles carry `aria-pressed` and a title that names the next action (*Hide sessions* /
  *Show sessions*), and a hidden panel's icon is muted.
- **The status pills are gone.** `#session-status` and `#agent-state` put `submitted`/`replied`/
  `working` in the header as GitHub-style chips, which competed with the review title and
  duplicated state that is already on screen: the rail card's badge says *working* / *replied* /
  *waiting for agent* / *confirmed* / *cancelled*, the stage toast says what the agent is doing,
  and the composer/STOP button says whose turn it is. The e2e asserts the header has no `.pill`
  and that those states still render in the toast and the rail card.

The three vertical/horizontal borders are **draggable**: the rail's right edge, the
inspector's left edge, and the line above the conversation. Each is a 9 px transparent strip over
the hairline that highlights in the accent colour on hover or focus; drag it (or focus it and use
the arrow keys, Shift for bigger steps) and the panel follows while the canvas is re-measured
mid-drag. Sizes live in CSS variables (`--rail`, `--inspector`, `--conv`) and are remembered per
browser (`redline:railWidth`, `redline:inspectorWidth`, `redline:convHeight`). Limits keep the
stage usable (≥ 360 px) and leave room for the annotation list above the conversation. The rail
divider is present on the dashboard too; both pages read the same `redline:railWidth` and
`--rail`, so moving between `/` and `/s/<id>` keeps the width instead of jumping back to 272 px.

Header icon buttons (`?`, theme, rail, inspector) are a fixed **32 × 32 px square**: the glyph
inside changes width (`?` is narrower than `☀`), the button must not.

## Left rail — sessions as folders

- **+ New review** at the top (drop a screenshot anywhere in the window also works). It is the
  same accent-blue button in the same place on the dashboard and in a review; the header carries
  icons only, so the primary action never moves or changes colour between pages.
- Search field (title, note, id).
- Filter chips with counts: `All` · `Active` (default) · `Open` · `Submitted` · `Done`. `All`
  keeps finished sessions visible; `Active` hides `resolved`. The old `Mine` chip is gone.
- Each row: agent-provided **title**, note excerpt, relative time (`2h ago`), the feature
  **branch** when known (accent monospace), status dot, and a badge that says who has the ball:
  **working** while the agent runs, **waiting for agent** for a
  submitted round nobody has picked up, **confirmed** / **cancelled** after a feature decision,
  otherwise a **new** marker for anything this browser has not looked at (tracked per session in
  `localStorage`) or the annotation count.
- Selected row is highlighted; clicking loads that session into the canvas without a page reload.
- Groups by day (`Today`, `Yesterday`, older) when more than ~15 sessions.

## Middle — canvas

- Renders at viewport size with a transform (crisp at every zoom, unlike CSS scaling a
  fixed-resolution canvas).
- **Zoom**: wheel with Ctrl/Cmd at the pointer, `+`/`-`, `0` fit, `1` 100%, double-click a tool
  button to reset. Range 10 %–800 %. Fit-to-window on load.
- **Pan**: space+drag, middle-drag, or plain wheel (shift for horizontal). Cursor changes. In a
  historical round (read-only viewer) the main button also drags the picture.
- Image is centred; the stage has a dark checkerboard so transparent PNGs read correctly.

## Tools and interaction

| Key | Tool | Notes |
|-----|------|-------|
| `V` | Select | click to select, drag to move, `Delete` to remove, arrows nudge 1 px (shift 10) |
| `R` | Rectangle | drag; shift = square |
| `E` | Ellipse | drag |
| `A` | Arrow | drag |
| `P` | Pen | freehand |
| — | Text | gone since v0.8: notes live in the inspector; legacy text annotations are read-only |

- Every shape is numbered on the canvas and in the inspector; freehand rows read
  `freehand · N points`. A pen stores `points`, not `start`/`end`.
- `Ctrl/Cmd+Z` undo, `Ctrl/Cmd+Shift+Z` redo (full history, not just "remove last").
- `Esc` cancels the in-progress shape or clears selection.
- `Ctrl/Cmd+Enter` submits the review.
- Hit testing covers rect/ellipse bounds+interior, arrow segment, pen polyline, legacy text box.

## Right panel

- **Annotations** list: numbered badge (same number as on the canvas), type, note preview or
  coordinates; click selects (canvas highlights), bin icon deletes. Drag on canvas keeps this list
  in sync.
- **Message to agent** collapsed by default — this is the session `note` (what the agent asked).
  It is context, not the main event, so it hides behind a disclosure.
- **Agent replies** (v0.7.0): the other half of the loop. The MCP `reply` tool appends to
  `agent-thread.jsonl`; `GET /api/sessions/<id>/thread` exposes it and the panel polls every 5 s,
  so an answer appears while the review is open. Before this the file was written and never read.
- Below it, a small `review.md` preview toggle (what the agent will read).

## Notes on shapes, not text on the canvas (v0.8)

The text tool is gone. Every mark — rectangle, ellipse, arrow, freehand — owns an optional `note`
string, and the note is the *only* place the human writes prose.

- Draw a shape, the inspector focuses **Note for the agent** for that shape.
- The canvas shows a numbered badge at the shape's start point; the same number appears in the
  annotation list, in `annotated.png` and in `review.md`, so the agent can match a paragraph to a
  region without guessing.
- Selecting a shape shows its note; the list previews it under the coordinates.
- Notes are not painted on the image (only the number is), so they never cover the UI under review.
- Legacy `text` annotations still render and still export; no new ones can be created.
- `annotations.json` carries `note` per shape (free-form map, additive), and `review.md` appends
  `— “note”` to the numbered region line.

## Thumbnail in the New review dialog (v0.13.2)

- The drop zone previews the image once something is attached — dropped, chosen, or pasted —
  instead of printing only `name — size KB`. The caption keeps the name and size and adds
  *click to replace*. The preview is an `objectURL` (revoked when it is replaced), so a pasted
  clipboard image never makes a round-trip to the server before Create.
- The preview is capped at `min(240px, 30vh)` and the modal scrolls (`max-height: 94vh`), so a
  tall screenshot cannot push *Create review* off the screen.

## Clocks in the audience's timezone (v0.13.1)

The server stores and serves RFC 3339 UTC (`+00:00`) everywhere. The UI formats those with `Date`
and the browser's locale, so a review submitted at 13:03 in Prague reads 13:03, not 12:03. Never
slice a clock out of the ISO string (`slice(11, 16)`): that silently renders UTC. `relativeTime`
and `dayLabel` were already local; the conversation timeline, agent activity lines and the history
round labels now are too (history folder names are flattened UTC stamps, `stampTime` un-flattens
and converts them).

## Feature confirmation (v0.21)

Each session is one feature. The rail's row actions carry the human's decision; agents map
`confirmed` to their own workflow (open a PR, merge into the integration branch) and treat
`cancelled` as "do not ship".

- **✓ Confirm feature** opens a dialog that explains the consequence and only then calls
  `POST /api/sessions/<id>/confirm-feature`. The session becomes `resolved` and keeps
  `feature_confirmed_at`; the rail shows a green `confirmed` badge and dot.
- **✕ Feature actions** opens one dialog with **Keep working**, **Cancel feature** and
  **Delete files…**. Cancel calls `POST /api/sessions/<id>/cancel-feature`: the review closes
  with `feature_cancelled_at`, nothing is merged and the screenshot/annotations stay in **Done**.
  Delete files… removes them permanently. A decided feature gets **Reopen** instead of Cancel in
  the same menu. The row itself never deletes on a single click.
- **↺ Reopen** clears both feature stamps and returns the session to `open`.
- **A decision closes the screen that showed the review.** Confirming, cancelling or deleting the
  review that is currently open navigates back to `/` with no selection; a decision on another
  row leaves the editor where it is. This is the same rule as a filter or search that no longer
  contains the open review: the rail deselects it and the dashboard appears instead of a review
  the chosen view says is gone.
- **Bulk Select** now offers **Confirm** (one dialog for the selection), **Delete** and **Close**;
  single-session delete is also one dialog away from the row's ✕.
- **Delete cancelled** in the rail head purges only features with `feature_cancelled_at`;
  confirmed features stay as the feature log. It is hidden when no feature is cancelled.
- The decision is additive server state (`feature_confirmed_at` / `feature_cancelled_at`); a new
  submission or agent round clears it, so a feature that gets more work is pending again.

## Agent settings and branches (v0.22)

- The header gear opens **Agent settings**: global max concurrent runs (default 2), default
  per-project limit (default 1), claim lease, per-project overrides, and live claims.
- Settings live on the server (`/data/agent-settings.json`) and are shared by the desktop and web
  UI. Changing them applies to new claims; the bridge's worker count is
  `REDLINE_AGENT_WORKERS` (default 4), and the server's limits are the real cap.
- A session can carry a `branch`. Every review has one from creation: the New review dialog's
  optional *Feature branch* field, or the derived `feat/<title-slug>-<id-tail>`
  (`feat/wrong-branches-52e0`). The rail shows it after the relative time in accent monospace
  (`2h ago · feat/parallel-runs`) and the review header shows it beside the session id; clicking
  it renames the branch (`PUT /api/sessions/{id}/branch`) — refused while the round is submitted,
  because the agent's worktree already exists for the old name. v0.21's `session/<id>` fallback is
  not produced any more and is read back as the `feat/…` name.
- A running claim consumes a slot; a claim waiting for the human frees the slot but keeps the
  branch lock. One branch has one owner. The bridge runs up to four claim workers
  (`REDLINE_AGENT_WORKERS`), each in its own git worktree; the server enforces the limits.

## Agent presence (unreleased)

- The header carries an agent chip next to the settings gear: green **N agents connected** or
  amber **No agent connected**, with the agent ids in its tooltip. Clicking it opens the agent
  panel.
- When nobody is online the rail head shows **No agent connected. Reviews will wait until an
  agent connects.** The warning is informational: creating, annotating and submitting a review
  stays enabled, so a bridge can be connected afterwards without losing work.
- Presence is the bridge's own heartbeat: repo sync every 60 s (and every `next`/claim call)
  touches `last_seen`; an agent stays online for 180 s. The dashboard and the editor share the
  same chip, fed by `GET /api/agent/status` every 10 s.
- **Connect an agent** is a dialog, not a permanent dashboard block. The header chip opens it on
  both pages; it shows the live status, the MCP JSON with the token masked (Copy config copies the
  real one), and sits in the shared fixed dialog shell. The dashboard only shows the amber
  invitation card — with a **Connect an agent** button — while no bridge is online; once one
  polls, both the card and the rail warning disappear.

## Dialog frame (unreleased)

Every popup is one shell: a fixed header with the title and a 32 × 32 px **X** at its right
edge, a fixed footer with the action buttons, and only the middle body scrolls (`.dialog-head`,
`.dialog-body`, `.dialog-foot` inside `.dialog-frame`). This applies to **Agent settings**, **New
review**, **Keyboard** and the feature/branch decision dialogs. The e2e opens settings in a
380 px-high window, scrolls the body past the project list, and asserts the head/foot rectangles
do not move; it also clicks every close X.

## Touch, bulk and accessibility (unreleased)

- **Touch**: `body.touch` (set from `pointer: coarse` / `maxTouchPoints`) raises every tool, chip
  and button to at least 40 px and makes the rail's feature actions always visible — no
  hover-only affordances.
- **Bulk**: the rail's *Select* mode turns rows into toggles (`aria-pressed`) with a bottom bar:
  *Confirm*, *Delete*, *Close*; Escape exits. The actions reuse the single-session endpoints.
- **Accessibility**: `:focus-visible` rings on every interactive element; `role="img"` + label on
  the canvas; `aria-label` on the composer, session rows and history rows; the conversation log is
  `aria-live="polite"`.

## Submitted lock and STOP (v0.17)

- While `status == submitted` the canvas is locked for editing. Every mutation funnels through
  `Editor.mutate` (plus `undo`/`redo` and `startText`), and those refuse while locked or while a
  history round is being viewed — the canvas pointer guard alone was not enough. The drawing tool
  buttons are `disabled` (dimmed), the inspector's align/layer/group bar and the note field are
  hidden, and the composer textarea is disabled. The annotation list becomes a flat read-only list
  (`.ann-row.readonly`: no cursor affordance, no delete button) whose rows are separated by a
  full-width hairline — a `border-top` on a rounded row paints only the segment between the two
  rounded corners, which reads as a broken frame. There is **no modal** — a single status toast in
  the middle of the stage says *"Agent is working — …"* (or *"Waiting for the agent…"*) above a
  light scrim. While locked, wheel, zoom buttons, keyboard zoom and pan are frozen; STOP removes
  the scrim and restores the view. Reading other panels is still allowed.
- The composer's primary button becomes a red **STOP**; it asks *"Stop the agent and edit this
  review?"* and calls `POST /api/sessions/<id>/cancel`. The canvas unlocks with the annotations
  intact and the button returns to *Submit review*.
- The server withdraws the round from history when no agent has read it yet; if
  `seen_by_agent_at` is set the round stays. The agent bridge polls the session every 2 s during a
  run and kills the child process when the status leaves `submitted`.

## Round history, read-only viewer and drafts (v0.16)

- A round exists the moment it is submitted (archived with its source screenshot), so History is
  never empty after the first submit.
- **History is navigation, not actions**: `Current` is always present; round rows open the
  archived source screenshot + annotations **read-only**; the header bar
  `Viewing round N — drag to pan, Ctrl+scroll to zoom · back to current` returns to the live
  canvas. Only `Current` is editable.
- **A historical round is a viewer, not a dead image**: left-drag pans (the canvas shows a grab
  cursor), the wheel scrolls (Shift for horizontal), Ctrl/Cmd+wheel zooms at the pointer, and the
  zoom buttons plus `+`/`-`/`0`/`1` keep working. Space+drag works even when a history row still
  has keyboard focus; the pointer being over the stage is what arms space. The live "agent is
  working" toast is hidden while an archived round is on screen.
- The ghost (`◫`), diff (`Δ`) and open-PNG (`↗`) buttons and the `Ghost · …` chip were removed;
  the diff endpoint remains for agents.
- The conversation no longer renders the round screenshot (`.conv-img`); the canvas and History
  own the pictures.
- **Drafts** live in memory per session: unsent annotations and composer text survive switching
  sessions. A new agent round discards the draft (wrong coordinates) and says so in the
  conversation. The window title is prefixed with `•` while drafts exist, and the desktop shell
  asks before quitting.

## Rounds: one task, one session (v0.14)

An agent that answers with a new screenshot must post it **into the same review**, not create a
new one.

- `post_round` (upload) and `capture_round` (server-side capture) archive the previous round,
  write a uniquely named screenshot (`screenshot-<stamp>.png`), clear the annotations and set the
  status back to `open` — the human's turn.
- The editor polls the session meta; when `meta.image` changes it reloads the canvas and keeps the
  conversation scrolled where the reader is.
- The round appears in the conversation with a thumbnail (`/sessions/<id>/rounds/<file>`), next to
  the agent's note. History keeps the archived annotated rounds.
- A small toast at the top of the stage says "Waiting for the agent…" (or the latest activity)
  while `submitted`; it is `pointer-events: none`, so annotating is never blocked.

## Waiting states and the webhook (v0.12)

- After Submit, the conversation says what is happening: **Waiting for an agent** (nobody has read
  it yet, with the age of the submission and the two MCP calls that would read it), **Agent
  received this** (with the time), the live activity line while `working`, and the reply itself.
- The canvas toast repeats that line while `submitted`. It sits **under** the tool bar — the tools
  are at the top of the stage and the toast must never cover them — and ignores the pointer.
- `REDLINE_WEBHOOK_URL=http://host:port/path` makes the server POST a small JSON body
  (`event`, `session_id`, `title`, `project`, `prompt`, `annotations`, `review_url`) when the human
  submits. It is a plain HTTP POST with a 5 s timeout, fire-and-forget: a dead endpoint never
  fails the submission. Use it to wake an agent or a relay; RedLine still has no agent push of its
  own.

## Paste, gallery and diff overlay (v0.11)

- **Ctrl+V** anywhere in the app opens the New review dialog with the clipboard image already
  attached (title defaults to "Pasted screenshot"). No file on disk, no upload round-trip.
  Works on the editor and on the start page; drops still work the same way.
- **Gallery on `/`**: the last twelve *active* reviews as cards with their screenshot, project,
  relative time and status. Confirmed and cancelled features are finished work and stay in the
  rail's All/Done filters, not on the dashboard. The start screen below it is unchanged.
- **Diff overlay**: in the History disclosure, the `◫` button ghosts that round's annotations
  behind the current ones at 35 % opacity; a `Ghost · <stamp>` chip in the header clears it.
  The `Δ` button still prints the textual diff.

## Projects in the rail (v0.10)

The rail lists projects (conversation areas), not a flat stream of requests.

- A project is a folder: `sessions/projects/<slug>/project.json` plus its sessions one level
  deeper. It can carry a `repo` key relative to the agent's repo root; that is what makes the
  agent run the review in the right repository.
- The agent syncs git repositories it discovers (`GET /api/project` from OpenCode, plus a scan of
  its repo root) through `POST /api/agent/repos`. Repo keys are portable (`redline`,
  `ActiUnit/Au.Net`), never absolute paths.
- Sessions created without a project live flat and are shown as **Inbox** — there is no migration,
  the label is added on read. **Inbox** and legacy label-only projects still work; the agent uses
  its default `REDLINE_AGENT_REPO` for them.
- Each project header shows its name and the total number of sessions — one count, not a waiting
  count beside the total (the per-row badge says which rounds are waiting). Click to collapse; the
  state persists per browser.
- **A running agent is visible in the rail.** A review the agent is currently working on carries a
  spinner and a **working** badge instead of its annotation count, its dot pulses, and the project
  header shows the same spinner and label (with a count when several reviews run at once) — even
  while the project is collapsed. The rail re-reads `/api/sessions` every 8 s and redraws only when
  the set of running reviews changes, so the received → working → replied transition appears
  without a reload. `agent_state` comes from `progress`/`reply`; STOP clears it, and the rail only
  treats a review as running while it is still `submitted`. A submitted round the agent has
  answered but not resolved reads **replied** (amber) instead of **waiting for agent**, because
  the agent already picked it up and may still be working.
- Filters and search still apply inside every project; day separators appear only in long lists.
- The New review dialog picks the project from a **select** grouped into **Repositories** and
  **Other projects**; it is required. Empty legacy projects are hidden. **+ New project…** reveals
  a name field and creates a label without a repo (the agent falls back to its default repo).
- The dialog **remembers the project you last worked in**: opening a review, or choosing a project
  for a new one, preselects it next time (Ctrl+V included) on the editor and the start page. It is
  a browser preference (`redline:project` in localStorage), so it survives reloads and does not
  follow you to another machine. A project missing from the picker falls back to the placeholder.

## Conversation and composer (v0.9)

The inspector's lower half is the conversation, so the tool stops feeling like a queue of requests.

- **Composer** at the bottom: a prompt ("What should change?") plus **Submit review**. The
  top-right Submit button is gone; `Ctrl+Enter` still sends. The prompt is stored as
  `meta.prompt` and appears in the timeline as a "you" bubble.
- **Timeline**: the agent's request (grey), your prompt (blue), activity events (small lines with
  a dot and a time) and agent replies (green), merged and sorted by timestamp.
- **Agent state** in the stage toast and the rail card: `received` (the agent read the
  submission), `working` (it is doing something, with the latest activity line beside the
  conversation title) and `replied`. `seen_by_agent_at` records the first read.
- **Run summary** at the tail of the conversation (v0.19): a card that reads **Working** with a
  pulsing dot while `agent_state` is `working`, and **Used N tools: Edit and Shell** — the distinct
  `tool` names the agent reported through `progress`, counted for the current round only. **Run
  finished** appears only when the round has left the agent's hands (a new round was posted or the
  review was resolved); a round with no activity shows no card. A `reply` does **not** finish the
  run: the agent can answer and keep working for minutes, so while the review is still `submitted`
  the card reads **Agent replied — waiting for it to finish** in amber, keeps the age of the last
  step, and turns back into **Working** as soon as the next `progress` line arrives. This is the
  interactive-run view an agent's own terminal chat shows, so the human does not have to read the
  step lines to know whether anything is still happening — and STOP, which is still armed, never
  contradicts the card.
- **"Working" means the round is still submitted.** The run card also shows *last step X ago*, and
  the stage toast never claims `working` on a review that already went back to the human — even if
  a late narration line arrives after `post_round` (the server refuses to revive a finished round,
  and the UI derives the state from `status` as well as `agent_state`). A spinner that never stops
  is indistinguishable from a dead agent, so the age of the last step is always on screen. The
  exception is a `reply` on a still-submitted round: that is not a finished round, and a later
  `progress` is proof the run resumed, so it may set `working` again.
- Polling stays at 5 s for the session meta, the thread and the events; no websockets yet.
- `progress` is the MCP tool the agent uses to narrate: one short line per meaningful step
  (`state` = thinking/tool/step, optional `tool` name). It appends to `agent-events.jsonl`.

## Status bar

Zoom level, image dimensions, live cursor position in image pixels, session id. Cheap to add and
it makes the canvas feel instrumented.

## Server changes

- `GET /api/sessions` gains `annotation_count` per session (computed by reading each
  `annotations.json`) so the rail can show badges without N extra requests.
- UI assets split into `app.css`, `app.js`, `editor.js`, `list.js`, all inlined into the binary via
  `include_str!` and served from `/static/*`.

## Staging

1. **Shell** — done in v0.3.0: 3 columns, zoom/pan, tools on a proper stage, status bar, inspector.
2. **Sessions rail** — done in v0.3.0: filters, search, badges, session switching without reload.
3. **Editing depth** — done in v0.3.1–0.3.3: undo/redo, inline text (create, edit, delete by
   clearing), nudge, delete, drag-to-move, pinch zoom.
4. **Polish** — done in v0.3.1–0.3.2: light theme, shortcut sheet, agent-view preview, colour cues.
   Remaining: focus rings, empty states in the rail, multi-select.

## Multi-selection and alignment (v0.4)

- Shift/Ctrl-click adds or removes an annotation from the selection; dragging empty canvas draws a
  rubber band (pan is space/middle/right drag, as in Figma).
- Dragging any selected annotation moves the whole selection; arrows nudge it; Delete removes it.
- With 2+ selected, the inspector grows an alignment row: left / centre / right, top / middle /
  bottom. With 3+, distribute horizontally or vertically (equal gaps, not equal centres).
- The annotation count reads `2/3` when a subset is selected, so the selection is never ambiguous.

## Capture from the UI (v0.4)

*New review* takes a URL as well as a file. **Page URL** is metadata for an uploaded screenshot;
with no image attached the server screenshots that URL (`POST /api/capture`, the same engine as the
`capture_review` MCP tool) and opens the session. Requires the `:browser` image
or `REDLINE_CHROME`; otherwise the dialog shows the actionable error.

## Submission history (v0.4)

Each *Submit again* archives the round it replaces into `sessions/<id>/history/<timestamp>.json`
plus `.png` (last 20 kept). `GET /api/sessions/<id>/history` lists them, and the inspector's
**History** section links to the old screenshots — so a re-review can compare what changed instead
of overwriting the evidence. Rounds are compact bordered rows (stamp, annotation count, then
ghost / diff / screenshot actions), not full-size annotation cards, so the list stays scannable.

One round is one row. *STOP* then *Submit again* without touching the canvas re-sends exactly the
annotations and the picture already archived, so the list collapses such a run of identical copies
to its newest entry instead of showing the same round twice (2026-10-30).

The same rule applies to the live canvas: a round that is still what the canvas shows (after
*Submit*, and also after the agent resolves or the review is reopened) is marked `current` and
labelled **Round N (current)**. It is not listed a second time as a *Current* row; *Current* only
appears when the canvas differs from every archived round (the agent posted a new picture, or the
human is drawing the next round). Clicking the live round returns to the canvas.

## Layers, groups, text size (v0.5)

- Paint order is array order. With a selection the inspector offers **bring to front / forward /
  backward / send to back**.
- **G** groups the selection (assigns a shared `group` id), **Shift+G** ungroups. Clicking one
  member selects the whole group, so moving or deleting a group is one gesture. The annotation
  list marks grouped rows with `⧉`.
- The text-size selector was removed with the text tool in v0.8; legacy text keeps the size it
  was saved with.
- New annotations carry a stable `id`, which is what makes history diffing able to say "moved"
  instead of "deleted and added" for older rounds.

## History diffing (v0.5)

`GET /api/sessions/<id>/history/<stamp>/diff` compares an archived round with the current
annotations: `added`, `removed`, `changed` (with before/after JSON). Matching is by annotation
`id`, falling back to canonical JSON for pre-v0.5 annotations. In the inspector's **History**
section, `Δ` shows the summary inline:

```
2 → 4 annotations

+ arrow at 9,9
- rect at 0,0
~ text “tighten this” (edited)
```

## Verification

`desktop/scripts/e2e.mjs` drives Chromium over CDP and asserts behaviour the Rust tests cannot see:
coordinate mapping under a fitted zoom, dragging, undo, inline text (including diacritics), and
submission, multi-select with align, grouping, z-order, text sizing, history archival and diffing — 32 checks. It runs in CI on every push (`browser e2e` job) and locally via
`desktop/scripts/e2e.sh`.

It has already earned its keep twice: it caught the text tool being broken by mousedown's focus
change, and duplicate text annotations from a double `blur` commit.

Each stage is a tagged release; the old page keeps working until the new one covers it.

## Non-goals

Layers, blending modes, shapes beyond rect/ellipse/arrow/pen, colour pickers beyond presets,
multi-select, exporting to other formats. This is a review tool, not an editor.
