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

## W1 — Brand: `redline` → `RedLine`

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

## W2 — Notes instead of a text tool

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

## W3 — Agent status you can see

**Goal:** after Submit, the human knows the agent picked it up.

States (stored in `meta.json`, additive):

| State | Who sets it | Meaning |
|---|---|---|
| `open` | human creates | nothing submitted yet |
| `submitted` | human submits | waiting for the agent |
| `received` | agent reads feedback | `seen_by_agent_at`, set automatically by `get_feedback` / `wait_for_feedback` |
| `working` | agent says so | new MCP tool or `reply` with `state: "working"` |
| `replied` | agent writes | `reply` lands |
| `resolved` | agent finishes | existing `resolve` |

- UI: status pill in the header + a timeline entry ("Agent received this 2 min ago"), and a subtle
  "agent is working…" indicator while `working`.
- No push channel: the editor already polls the thread every 5 s; extend that to the session meta.
- CLI/MCP: `get_feedback` returns the same state so the agent can be idempotent.

**Decision needed:** is `working` explicit (agent must say it) or inferred from "received and no
reply within N minutes"? Explicit is honest; inferred is zero-effort for the agent.

---

## W4 — Chat flow: composer + timeline

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

## W5 — Projects in the rail

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

## W6 — Chrome cleanup (ships with W4)

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

## Open decisions (answer before 0.8.0)

1. Rename: display-only, or technical too?
2. Notes: numbered badges on `annotated.png`, or clean image + notes only in the panel?
3. `working`: explicit agent signal, or inferred timeout?
4. Composer: bottom of the inspector, or a full-width bar?
5. Project: a label on a session, or a first-class folder with its own tree?

## Non-goals

- Real-time push (SSE/WebSocket) — 5 s polling is enough at this scale.
- Accounts, multi-user, sharing.
- Rich text in notes; plain text only.
- Drag & drop of existing annotations between projects.
