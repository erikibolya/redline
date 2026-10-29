# RedLine 1.0 — UX rework

Written after using redline to review redline itself (v0.7.0). The loop works, but it reads like a
queue of requests, not like a conversation with an agent. This is the plan that changes that.

Ordered by dependency, not by wish size. Each workstream ships on its own so it can be reviewed in
the tool it changes.

---

## Principles

1. **A project is a conversation.** The rail lists projects (agent + work), not individual requests.
2. **The image is the message.** Text is metadata attached to a mark, not a drawing.
3. **The agent's state must be visible.** "Submitted" without "received / working / replied" is
   silence with a badge.
4. **No invisible writes.** Every state an agent writes has a route and a screen (the `reply`
   lesson from v0.7.0).

---

## W1 — Brand: `redline` → `RedLine` (shipped in 0.8.0)

**Goal:** the product is called RedLine in every human-facing place.

Scope (safe, display only):

- UI: `<title>`, header brand, `document.title`, empty states, help sheet, setup page.
- Outputs: `review.md` heading, CLI help text, README/docs headings.
- Desktop app: window title, tray tooltip, menu labels, installer display name.

Deliberately **unchanged** (technical identifiers, changing them breaks installs and CI):

- binary `redline`, crates `redline-core` / `redline-server` / `redline-app`
- image `ghcr.io/erikibolya/redline`, container name `redline`, data dir `/mnt/user/appdata/redline`
- MCP server key `redline`, resource scheme `redline://session/...`
- env vars `REDLINE_*`, API paths

**Decision needed:** display-only (recommended) or a full technical rename with a migration period
(image alias, data-dir move, MCP key rename in every client config).

---

## W2 — Notes instead of a text tool (shipped in 0.8.0)

**Goal:** draw a mark, then write what is wrong there. Text never floats on the canvas.

- Every annotation gets an optional `note` string. The model is a free-form map, so this is
  additive: no migration, old files stay readable.
- Tools: rect, ellipse (**new** — the plan's "kruh"), arrow, free draw. No text tool in the toolbar.
- After drawing, the inspector focuses the note field for the new shape
  ("What is wrong here / what should change?").
- Selecting a shape shows its note in the inspector; on hover/selection the shape can show a small
  numbered badge (`①`) on the canvas so the human can match it to the list. The note itself is not
  painted on the image.
- Agent output: `annotations.json` carries `note` per shape; `review.md` lists
  `1. rectangle (1147,104)→(1421,736) — "alignment of icons…"`.
- Legacy `text` annotations keep rendering as they do today (read-only path) and gain a "convert to
  note on the nearest shape" action.

**Decisions needed:** badge on the canvas vs nothing at all; whether `annotated.png` gets badges
(recommended: yes, numbers only), and what to do with old text notes on export.

---

## W3 — Agent status you can see (shipped in 0.9.0)

**Goal:** after Submit, the human knows the agent picked it up.

States (stored in `meta.json`, additive):

| State | Who sets it | Meaning |
|---|---|---|
| `open` | human creates | nothing submitted yet |
| `submitted` | human submits | waiting for the agent |
| `received` | agent reads feedback | `seen_by_agent_at`, set automatically by `get_feedback` / `wait_for_feedback` |
| `working` | agent says so | explicit, with a live activity line |
| `replied` | agent writes | `reply` lands |
| `resolved` | agent finishes | existing `resolve` |

`working` is **explicit and detailed** (decision 2026-10-29): the agent reports what it is doing,
not just that it is busy.

- New MCP tool `progress` (name to settle): `{ session_id, state, message, tool? }` where `state` is
  `thinking` / `tool` / `step`, `message` is one short human-readable line ("Reading app.js",
  "Running the browser e2e"), and `tool` is the tool name when relevant.
- Stored in `agent-events.jsonl` beside the thread (append-only, same tolerant reader), so the
  history survives a restart and the timeline can replay it.
- UI: status pill in the header, an activity line under it ("Agent is working — Running tests…"),
  and a collapsible "Activity" section in the timeline with the last N events and relative times.
- Throttle on the agent side: one event per meaningful step, never per token. The UI shows the last
  event prominently and the rest folded.
- No push channel: the editor already polls the thread every 5 s; extend that to meta + events.
- `get_feedback` returns the same state so the agent can stay idempotent.

**Decision needed:** nothing — the format above is the proposal; the exact tool name can wait for
implementation.

---

## W4 — Chat flow: composer + timeline (shipped in 0.9.0)

**Goal:** the right panel becomes a conversation, not a form.

- **Timeline** per session: your prompt → the image card (thumbnail + annotation count) → agent
  replies → status changes → resolved. Newest at the bottom, chat-style.
- **Composer** at the bottom of the inspector: textarea ("What should change?") + **Submit
  review** button. `Ctrl+Enter` submits. The prompt is stored as the session note.
- **Top-right menu loses Submit.** It keeps: status pill, agent state, help, theme, rail/inspector
  toggles, `+ New review`.
- The image card in the timeline opens the canvas; the canvas is the workspace, the chat is the
  record.

**Decisions needed:** composer at the bottom of the right panel (recommended) or a full-width bottom
bar under the canvas; whether a submission with zero annotations but a prompt is allowed (it should
be — that is just a message).

---

## W5 — Projects in the rail (shipped in 0.10.0)

**Goal:** the rail is a list of projects/conversations, not of requests.

- New entity `Project`: id/slug, name, agent label, created_at, optional colour.
- Store: `projects/<slug>/project.json`; sessions move under `projects/<slug>/sessions/<id>/`.
  Alternative (cheaper): keep sessions flat and add `project_id` to `meta.json`. Decide before
  writing the migration.
- Migration: existing flat sessions land in an `Inbox` project; no data loss, no rewrite of
  `annotations.json`.
- Rail: project row (name, last activity, counters: waiting on agent / unread replies / done).
  Expanding a project shows its threads; clicking one opens the conversation + canvas.
- Agent API: `project` argument on `review_screenshot` / `capture_review` (default `Inbox`),
  `list_reviews` accepts a project filter and returns project grouping.
- Home page `/` becomes a project gallery (this absorbs the old "session gallery" idea).

**Decision needed:** is a project *one agent conversation* or *a work area that can hold several
agents*? The answer decides whether `project` is a label or a first-class object.

---

## W6 — Chrome cleanup (shipped in 0.9.0)

- Top-right audit: status, agent state, `?`, `⌘` shortcut hint, theme, rail/inspector toggles,
  `+ New review`. Nothing that duplicates the composer.
- Fix from the 2026-10-29 review: annotation rows in the inspector are not aligned — icons should
  sit on the first line, rows need clearer separation.
- Keyboard: `Ctrl+Enter` = submit from the composer; `T` no longer opens a text tool.
- Empty states: "No project selected" instead of a blank canvas.

---

## Order and releases

| Release | Workstreams | Why this order |
|---|---|---|
| **0.8.0** | W1 + W2 | Brand and the note model are visible immediately and independent of the data model. |
| **0.9.0** | W3 + W4 + W6 | Status and the chat flow are what "it doesn't feel like talking to an agent" is about. |
| **0.10.0** | W5 | Projects are the only change that touches storage and the agent API; do it once the flow is settled. |
| **1.0** | signing | Still blocked on a certificate, unchanged. |

Each release: Rust tests, browser e2e extended for the new interaction, CHANGELOG entry, tag,
deploy to holly, then a self-capture review in the new UI.

---

## Decisions (2026-10-29)

| # | Question | Answer |
|---|---|---|
| 1 | Rename scope | **Display only.** Binary, image, container, paths, MCP key and env vars keep `redline`. |
| 2 | Notes on `annotated.png` | **Numbered badges on every shape**, the same number in the inspector, the canvas and `annotated.png`; the note text is expandable in the panel and never painted on the image. |
| 3 | `working` | **Explicit, with detail** — what the agent is doing (tool, step, thinking line), not just a spinner. See W3. |
| 4 | Composer | **Bottom of the right panel**: textarea + Submit. The top-right Submit goes away. Future: draggable/snapping panels for a custom layout. |
| 5 | Project model | **First-class folder with its own tree** (`projects/<slug>/…`), not a label on a session. |

## Non-goals

- Real-time push (SSE/WebSocket) — 5 s polling is enough at this scale.
- Accounts, multi-user, sharing.
- Rich text in notes; plain text only.
- Drag & drop of existing annotations between projects.
