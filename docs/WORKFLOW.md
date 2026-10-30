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

Each session is one feature (v0.21+). When the human is satisfied, they click the rail's
**✓ Confirm feature** and accept the dialog. RedLine records `feature_confirmed_at`, sets
`status=resolved` and closes the review; it never touches git.

The agent reads the decision as `feature_state == "confirmed"` from MCP (`get_feedback`,
`wait_for_feedback` or `list_reviews`) and applies its own workflow. In this repository that is:
keep the work on a feature branch, open a PR into `dev`, wait for the required checks and merge;
the dev build is the testable result.

- **✕ Cancel feature** closes the review without merging and keeps the screenshot/annotations.
- **↺ Reopen** clears either decision and returns the session to the queue.
- A new submission or agent round clears the decision, so more work makes the feature pending.
- `resolve` only closes an agent round; it is **not** the human's feature confirmation.
- Data deletion is separate: bulk **Delete** or **Delete done**. The ✕ row action no longer
  deletes files.

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
