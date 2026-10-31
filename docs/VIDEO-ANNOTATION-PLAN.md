# Video annotation feedback — plan (2026-10-31)

Status: **implemented** on `feat/video-annotation-fixes`; browser e2e 229/229 and the Rust
workspace suite are green. Docs updated in `docs/UI.md`, `docs/VIDEO.md`, `docs/TESTING.md` and
`CHANGELOG.md`.

Three reports from using video reviews in v0.23.0. Reproduced against a disposable 0.23.0
instance before planning.

## 1. Annotations vanish when leaving and returning to a video review

**Report.** "When I create annotations for the video and click to a different review, all my work
was gone when I came back. There was a lot of annotation."

**Reproduction.**

- Switching reviews through the editor rail (`Editor.openSession`) keeps the work: the in-memory
  `drafts` Map is restored.
- Leaving through the dashboard, a browser reload, a desktop-app navigation or the back/forward
  cache starts a new document; the `Editor` is new and `drafts` is empty. The server has no
  annotations because the review was never submitted — the work is gone.

**Root cause.** Drafts are memory-only (`this.drafts = new Map()`), exactly as decided for v0.16
(`docs/ROUNDS-PLAN.md` § 5/§ 8: "localStorage is a follow-up if it proves necessary"). Video
reviews produce many more annotations per round, so the loss is now expensive.

**Fix.**

- Back the draft store with `localStorage`, one key per session
  (`redline:draft:<session-id>`), while keeping the same `get`/`set`/`delete`/`size` interface.
- A draft holds `annotations`, `prompt`, `savedAt` and a media fingerprint. On load, discard a
  draft whose media fingerprint no longer matches (the agent posted a new round) or whose review
  is already submitted, and show the existing "Draft discarded" notice.
- Save on a short debounce after every mutation/text change, and flush on `pagehide` and
  `visibilitychange` (hidden). Clear the stored draft on submit and on a new video round.
- Keep at most 20 drafts, drop entries older than 30 days, and evict the oldest on quota errors;
  the current session's annotations stay in memory regardless.

**Tests.** Browser e2e: draw two timecoded annotations, reload the page, assert both are restored
with their `at` values; a full dashboard round-trip restores them too; submit removes the stored
draft; a new round discards a stale draft.

## 2. Group annotations by timestamp in the inspector

**Report.** "In the annotations pane, annotations should be grouped by the timestamp (somehow
separated, choose your visual style)."

**Design.**

- Video sessions only. Image sessions keep today's flat list.
- Groups are chronological sections, one per distinct annotation time (millisecond precision),
  with a header: a small colour-neutral dot, the timecode in monospace, and the group's annotation
  count. Rows under the header keep their global number, colour, note/label, range button and
  delete button.
- The group header is a button: clicking it seeks the video to that timestamp and clears the
  selection, so it is a navigation control rather than a label.
- The group that contains the playhead gets an accent marker; ranges count as active between
  `at` and `until`.
- Annotations without a numeric `at` (legacy/edge cases) go last under "No timecode".
- Visual separation: a 10 px gap and a hairline above each group after the first; no heavy cards,
  so the rows still read as one list. Light and dark themes use the existing tokens.

**Tests.** e2e: two annotations at one moment and one at another produce two group headers with
the right timecodes and counts; the header seeks; a range group is marked active; read-only
history groups use the same shape; image sessions have no group headers.

## 3. Old annotations stay visible when the playhead moves

**Report.** "When I draw an annotation and move on the timeline, the old annotations (from a
previous timestamp) are still visible and disappear only after I draw a new one."

**Reproduction.** After drawing at 00:00.500 and seeking to 00:01.200, the freshly drawn (still
selected) annotation has opacity `1` at the far time. Once another annotation is drawn, selection
moves and the old one drops to the 12 % ghost. So the old annotation was kept visible by
*selection*, not by its time.

**Root cause.** `Editor.annotationOpacity` starts with `if (this.selected.has(index)) return 1;`,
and a new drawing stays selected. Selection is a highlight, not a statement that the annotation
belongs to the current frame.

**Fix.**

- Selection no longer overrides time visibility; it only draws the dashed outline.
- Ghosts become opt-in: the default is *off*, so other moments are completely hidden and the
  canvas shows exactly the current frame's annotations. The existing **Ghosts** toggle shows them
  at the designed low opacity (with the fade near their time); the preference is remembered.
- Timeline markers always show every annotation, so nothing becomes undiscoverable.
- The active group in the inspector follows the same rule.

**Tests.** e2e: draw, keep it selected, seek far — opacity must be `0`; turn Ghosts on — the same
annotation is a faint ghost; near its time it fades to full; the rendered canvas contains no
non-current annotation pixels with Ghosts off; the toggle state survives a reload.

## Order

1. Draft persistence (localStorage, flush, stale detection, tests).
2. Time visibility (selection + Ghosts default, tests).
3. Inspector groups (render helper, CSS, seek, active state, tests).
4. Docs: `docs/UI.md`, `docs/VIDEO.md`, `docs/TESTING.md`, `CHANGELOG.md`, `PLAN.md` counts.
5. Full Rust suite + browser e2e, then the normal feature-ready handoff.

## Out of scope

- Server-side drafts or autosave: unsent work remains local to the browser profile/origin.
- Conflict resolution between two browsers editing the same draft.
- Filtering annotations at submit: every annotation in the session is still sent, regardless of
  the playhead or Ghosts setting.
