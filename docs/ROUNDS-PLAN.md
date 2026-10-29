# Rounds, history and review state — plan

Seven requests from using 0.15.0. This document is the agreed shape before implementation; it
assumes one task = one session (v0.14) and the conversation panel (v0.9).

## 1. The conversation stops showing the round screenshot

Current: `renderConversation()` draws `.conv-img` for thread messages that carry `image`.

Change: drop the thumbnail and its CSS. The round picture belongs on the canvas and in History;
the conversation keeps only text (agent note, activity, prompt). The `image` field stays in
`agent-thread.jsonl` and `/rounds/<file>` keeps serving, so agents/tools are unaffected.

## 2. History becomes round navigation

Current: each row is a submission with three tiny buttons — `◫` ghost, `Δ` text diff, `↗` open the
archived PNG. The ghost chip appears in the header; `↗` opens a new tab, which the desktop webview
blocks (it looks dead). The rows are not clickable as a whole.

Change:

- **One row per round, clickable.** `Round N · date · X annotations` and a `Current` row at the
  top. Rows are read-only views, not actions.
- **Clicking a round swaps the canvas to that round**: the archived source screenshot plus that
  round's annotations, rendered read-only (no tools, no selection, no editing). A bar at the top
  says `Viewing round N — Back to current`.
- **Current** returns to the live session.
- The inspector shows the archived annotations while viewing (clicking one highlights it on the
  canvas but cannot edit it).
- Remove `◫` (ghost), `Δ` (text diff) and `↗` (new tab). The diff endpoint stays for agents; the
  ghost feature is deleted (chip, `setGhost`/`toggleGhost`, CSS).
- Numbering counts *submissions*, oldest = 1; the live round is `Current`.

**Data needed (backend):**

- `archive_previous()` currently copies `annotations.json` and `annotated.png` only when the round
  has annotations, and never the screenshot. It must also copy the round's source screenshot to
  `history/<stamp>-source.png` (the file name is already served by `/sessions/<id>/history/<file>`).
- `HistoryEntry` gains `source: bool` and `round: usize` (1-based, computed oldest→newest).
- Archiving should happen even for an empty round (a submission with a prompt but no annotations
  is still a round), while `annotated.png` may be absent.
- `prune_history` must delete `-source.png` with its stamp and keep the last 20 rounds.

## 3. Ghost and diff leave the UI

Ghosting was a workaround for not being able to see the previous round; round viewing replaces it.
Delete the header chip, the `ghost` state in the editor, the `◫` handler and the `?v` ghost
interaction. Keep `/api/sessions/<id>/history/<stamp>/diff` (agents and the CLI may use it) but no
button.

## 4. Submitted means locked, and STOP unlocks

Current: while `submitted`, the canvas stays editable; a second submission is possible and the
agent's work races the human's edits.

Change:

- **Dark overlay** over the canvas while `status == submitted` (≈45 % black, lock icon, "Submitted —
  waiting for the agent"). It swallows pointer events; tools and the note field are disabled. The
  image stays visible underneath.
- The composer's primary button becomes **red STOP** while submitted. Clicking asks
  *"Stop the agent and edit this review?"*; confirmed → `POST /api/sessions/<id>/cancel`:
  sets `status = open`, stamps `cancelled_at`, keeps the annotations and the round untouched.
  The canvas unlocks, the button returns to *Submit review*.
- **Bridge cooperates:** during `opencode run` it watches the session every ~2 s; if the status is
  no longer `submitted` (cancelled or otherwise), it kills the child process and logs it. A late
  `reply`/`post_round` from the model after the kill is ignored by the UI (it belongs to a
  cancelled run) — the bridge must not post anything itself.
- `cancel` is idempotent and only valid while submitted; it never touches `annotations.json`.

## 5. Unsent annotations survive a session switch

Current: `Editor.load(id)` replaces `this.annotations` with the server copy; switching away throws
the local edits away.

Change:

- Editor keeps `drafts: Map<sessionId, {annotations, prompt, selected, image}>`.
- Before loading another session, save the current annotations + composer text + selection **if
  they differ from the loaded server state**.
- `load(id)` restores a draft instead of the server annotations and leaves the composer text.
- Submit success deletes the draft; a new round from the agent clears it (the image changed) and
  tells the human in the conversation ("Draft discarded — the agent posted a new round").
- In-memory only (survives switching sessions, not an app restart). localStorage is a follow-up if
  it proves necessary.

## 6. Backend summary

| Change | Where |
|---|---|
| archive source screenshot per round, `HistoryEntry.source`/`round`, archive empty rounds | `store.rs` (`archive_previous`, `history`) |
| `POST /api/sessions/{id}/cancel` | `api.rs` + `Store::cancel` |
| bridge watches status during the run and kills the child | `redline-agent/src/lib.rs` |
| remove `/rounds` thumbnail usage (route stays) | `app.js`, `app.css` |

## 7. Order

1. **Rounds data** (archive source, numbering, empty rounds) — backend + tests.
2. **History UI** (clickable rounds, read-only view, remove ghost/diff/↗) — depends on 1.
3. **Drafts** (independent, quick).
4. **Lock + STOP** (cancel endpoint, overlay, bridge watcher).
5. Conversation thumbnail removal (with 2).

Suggested releases: **0.16.0** = 1–3 + 5; **0.17.0** = 4. Both on `dev` first.

## 8. Decisions to confirm

1. Round view renders the **source screenshot + archived annotations** (zoomable, crisp) rather
   than the pre-baked `annotated.png` — ok?
2. History order: newest first with `Current` at the top (recommended), or oldest first?
3. STOP kills the running agent (bridge watcher) or only unlocks the canvas and lets the agent
   finish? Killing is what the button says; confirm.
4. Drafts in memory only (recommended) or also `localStorage` (survives app restart, can go stale
   against a new round)?
5. Do agent rounds get a separator in History (`— agent replied —`), or is History strictly one row
   per human submission?
