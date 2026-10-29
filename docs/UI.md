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
│ header:   redline · <session title>            [status]      [Submit review] │
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

Header icon buttons (`?`, theme, rail, inspector) are a fixed **32 × 32 px square**: the glyph
inside changes width (`?` is narrower than `☀`), the button must not.

## Left rail — sessions as folders

- **+ New review** at the top (drop a screenshot anywhere in the window also works).
- Search field (title, note, id).
- Filter chips with counts: `All 7` · `Open 5` · `Submitted 2` · `Mine` (created by a human).
- Each row: agent-provided **title**, note excerpt, relative time (`2h ago`), status dot, and a
  badge: annotation count for submitted sessions, a **new** dot for anything created or submitted
  since this browser last looked (tracked per session in `localStorage`).
- Selected row is highlighted; clicking loads that session into the canvas without a page reload.
- Groups by day (`Today`, `Yesterday`, older) when more than ~15 sessions.

## Middle — canvas

- Renders at viewport size with a transform (crisp at every zoom, unlike CSS scaling a
  fixed-resolution canvas).
- **Zoom**: wheel with Ctrl/Cmd at the pointer, `+`/`-`, `0` fit, `1` 100%, double-click a tool
  button to reset. Range 10 %–800 %. Fit-to-window on load.
- **Pan**: space+drag, middle-drag, or plain wheel (shift for horizontal). Cursor changes.
- Image is centred; the stage has a dark checkerboard so transparent PNGs read correctly.

## Tools and interaction

| Key | Tool | Notes |
|-----|------|-------|
| `V` | Select | click to select, drag to move, `Delete` to remove, arrows nudge 1 px (shift 10) |
| `R` | Rectangle | drag; shift = square |
| `A` | Arrow | drag |
| `P` | Pen | freehand |
| `T` | Text | click, type inline at that point (no `prompt()`), `Enter` commits, `Esc` cancels |

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

## Submitted lock and STOP (v0.17)

- While `status == submitted` a dark overlay covers the canvas (`#review-lock`): pointer events are
  swallowed, tools and the note field are disabled, and the composer textarea is disabled.
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
  `Viewing round N — back to current` returns to the live canvas. Only `Current` is editable.
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
- A small pill at the top of the stage says "Waiting for the agent…" (or the latest activity)
  while `submitted`; it is `pointer-events: none`, so annotating is never blocked.

## Waiting states and the webhook (v0.12)

- After Submit, the conversation says what is happening: **Waiting for an agent** (nobody has read
  it yet, with the age of the submission and the two MCP calls that would read it), **Agent
  received this** (with the time), the live activity line while `working`, and the reply itself.
- The canvas pill repeats that line while `submitted`. It sits **under** the tool bar — the tools
  are at the top of the stage and the pill must never cover them — and ignores the pointer.
- `REDLINE_WEBHOOK_URL=http://host:port/path` makes the server POST a small JSON body
  (`event`, `session_id`, `title`, `project`, `prompt`, `annotations`, `review_url`) when the human
  submits. It is a plain HTTP POST with a 5 s timeout, fire-and-forget: a dead endpoint never
  fails the submission. Use it to wake an agent or a relay; RedLine still has no agent push of its
  own.

## Paste, gallery and diff overlay (v0.11)

- **Ctrl+V** anywhere in the app opens the New review dialog with the clipboard image already
  attached (title defaults to "Pasted screenshot"). No file on disk, no upload round-trip.
  Works on the editor and on the start page; drops still work the same way.
- **Gallery on `/`**: the last twelve reviews as cards with their screenshot, project, relative
  time and status. The start screen below it is unchanged.
- **Diff overlay**: in the History disclosure, the `◫` button ghosts that round's annotations
  behind the current ones at 35 % opacity; a `Ghost · <stamp>` chip in the header clears it.
  The `Δ` button still prints the textual diff.

## Projects in the rail (v0.10)

The rail lists projects (conversation areas), not a flat stream of requests.

- A project is a folder: `sessions/projects/<slug>/project.json` plus its sessions one level
  deeper. `POST /api/projects` creates one; agents get it implicitly with `project` on
  `review_screenshot` / `capture_review`.
- Sessions created without a project live flat and are shown as **Inbox** — there is no migration,
  the label is added on read.
- Each project header shows its name, the number of waiting submissions (green) and the total.
  Click to collapse; the state persists per browser.
- Filters and search still apply inside every project; day separators appear only in long lists.
- The New review dialog has a project field with autocomplete from `GET /api/projects`.

## Conversation and composer (v0.9)

The inspector's lower half is the conversation, so the tool stops feeling like a queue of requests.

- **Composer** at the bottom: a prompt ("What should change?") plus **Submit review**. The
  top-right Submit button is gone; `Ctrl+Enter` still sends. The prompt is stored as
  `meta.prompt` and appears in the timeline as a "you" bubble.
- **Timeline**: the agent's request (grey), your prompt (blue), activity events (small lines with
  a dot and a time) and agent replies (green), merged and sorted by timestamp.
- **Agent state** pill in the header: `received` (the agent read the submission), `working` (it is
  doing something, with the latest activity line beside the conversation title) and `replied`.
  `seen_by_agent_at` records the first read.
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

*New review* takes a URL as well as a file: the server screenshots it (`POST /api/capture`, the
same engine as the `capture_review` MCP tool) and opens the session. Requires the `:browser` image
or `REDLINE_CHROME`; otherwise the dialog shows the actionable error.

## Submission history (v0.4)

Each *Submit again* archives the round it replaces into `sessions/<id>/history/<timestamp>.json`
plus `.png` (last 20 kept). `GET /api/sessions/<id>/history` lists them, and the inspector's
**History** section links to the old screenshots — so a re-review can compare what changed instead
of overwriting the evidence. Rounds are compact bordered rows (stamp, annotation count, then
ghost / diff / screenshot actions), not full-size annotation cards, so the list stays scannable.

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
