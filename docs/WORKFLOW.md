# Visual review workflow

## Roles

- **Agent:** captures the screenshot, creates the session, opens the app, waits, reads JSON,
  implements changes, confirms with a follow-up session.
- **Human:** draws notes and writes what should change; submits.

## 1. Prepare the review pack

Capture one screenshot per state you want reviewed:

- default view;
- key variants/color/size matrix;
- hover/focus/disabled/selected;
- narrow width / responsive;
- long text/overflow.

Keep the screenshot at a stable viewport (e.g. 1440×900) and record the URL and state in the
session title/note. Example:

```
title: "Button / hover + disabled"
url:   "http://localhost:5016/button"
note:  "Desktop 1440×900; compare filled/outlined hover, disabled, icon spacing"
```

## 2. Create the session

```bash
python3 redline.py open review/14-button/hover.png \
  --title "Button / hover + disabled" \
  --url "http://localhost:5016/button" \
  --note "Desktop 1440×900; compare hover/disabled" --json
```

Open the printed `review_url` in the browser or the Review pane. The server must be running:

```bash
python3 redline.py serve --open
```

With the current agents, pass `branch` to `review_screenshot` or `POST /api/sessions`. RedLine
uses it as the branch-lock key; if omitted, the first claim falls back to `session/<id>`. The
defaults are 2 concurrent runs globally and 1 per project; waiting for the human frees the run
slot but keeps the branch locked.

The bridge runs up to four claim workers (`REDLINE_AGENT_WORKERS`, default 4). Each claim gets a
separate git worktree on its branch; the agent commits and pushes there. One branch has one owner,
so two rounds of the same feature never edit the same worktree at once. A resolved round releases
the claim and removes a clean worktree; a dirty one is left for inspection.

## 3. Annotate

- **Rectangle** — mark a region that needs a change.
- **Arrow** — point from a note/label to a control.
- **Pen** — freehand underline/outline.
- **Text** — click a spot and type the instruction. This is the most important output; coordinates
  are included in `review.md`.

Suggested colors (a convention, not enforced): red = bug, yellow = spacing/visual, blue =
question/idea. Use the width slider for visibility. `Ctrl/Cmd+Z` undoes the last annotation.

Write notes as actionable instructions:

- Good: “Move the chevron 4px left; it touches the text at 200% zoom.”
- Avoid: “This looks wrong.” (no location/action)

## 4. Submit

Click **Submit review**. The page shows the submission timestamp. You can add more notes and submit
again; the agent reads the latest submission.

## 5. Agent reads the result

```bash
python3 redline.py wait <id> --timeout 600 --json
```

Files:

- `annotations.json` — exact coordinates/types/text.
- `annotated.png` — visual confirmation.
- `review.md` — text notes with coordinates + marked regions.

The agent turns every text note into a checklist item, uses shapes to locate the UI, fixes the
code, and records the change. Never edit the human's annotation files.

## 6. Confirm the feature

Each session is one feature (v0.21+). A review run is **not** a handoff: the agent may push the
feature branch and post a round, but it must not open a pull request, merge or deploy. When the
human is satisfied, they click the rail's **✓ Confirm feature** and choose whether to hand off.
The checkbox defaults to on when an online bridge advertises the `handoff` capability. RedLine
records `feature_confirmed_at`, sets `status=resolved`, closes the review and — only when the
handoff is requested — queues `handoff.status=requested`. It still never touches git.

The bridge advertises `["review","handoff"]` on `POST /api/agent/next`; the confirmed feature comes
back as a `handoff` job. A separate agent run reads the decision and performs the integration's
own action. In this repository's reference bridge that is: open or reuse the PR into `dev`, wait
for the required checks and merge. It reports the result with MCP `handoff_status`, whose fields
are deliberately generic: `state` (`done` or `failed`), `label` ("merged", "deployed", …), `url`
and `message`. The rail shows `handoff queued`, `handing off`, the integration's label with its
link, or `handoff failed ↻`.

A non-git integration can use `feature_state=confirmed` and do nothing, or use the same handoff
job for a deploy/publish and report its own label and URL. The server has no GitHub token and never
runs git; the `redline-agent` bridge is the reference git implementation.

The overnight failure this replaces: agents opened and merged PRs while the human was asleep,
before any confirmation, so every review was already touched by morning. The confirmation must
be the permission, not a status label added after the fact.

- **✕** opens a feature-actions dialog: **Cancel feature** closes the review without merging and
  keeps the screenshot/annotations; **Delete files…** removes them permanently.
- **↺ Reopen** clears either decision and returns the session to the queue. A decided feature also
  gets **Reopen** in the ✕ menu.
- A new submission or agent round clears the decision, so more work makes the feature pending.
- `resolve` only closes an agent round; it is **not** the human's feature confirmation.
- Data deletion is separate: the ✕ menu's **Delete files…**, bulk **Delete**, or **Delete
  cancelled** (only cancelled features; confirmed features stay as the log).

For a fixed-state follow-up, post a new round into the same session (the old flow created a second
session; v0.14 keeps one task per session instead).

## Au.Net component reviews

The product repo has `COMPONENT-REVIEW.md` with one step per component. For each step:

1. Start the docs app and capture the agreed states.
2. Create a session per state (or one session with a combined screenshot).
3. Human annotates → agent fixes → follow-up session.
4. When the human confirms, mark the component row ✅ and move to the next component.

Screenshot sets are kept outside git (`review/`); only code/docs/tests are committed.

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| Session list empty | `--sessions` points at a different folder than the agent used. |
| Address in use | Stop the other server or use `--port 8790`. |
| Screenshot missing | Check `sessions/<id>/meta.json` → `image`; re-open the session. |
| Submit fails | Check the server log; the request body is capped at 60 MB. |
| Coordinates off | Use a screenshot at its natural size; the editor maps CSS scaling to canvas pixels. |

## Do / don't

- **Do** write one state per session, with a clear title and URL.
- **Do** include the viewport size in the note when spacing is under review.
- **Don't** commit `sessions/`, screenshots or customer data.
- **Don't** expose the server beyond localhost.
- **Don't** hand-edit `annotations.json`; use the editor.
