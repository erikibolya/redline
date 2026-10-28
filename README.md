# au-visual-review

A tiny, dependency-free visual review queue for human ↔ AI collaboration.

The agent drops a screenshot into a session; you open the local page, draw rectangles, arrows,
freehand marks and text notes, and click **Submit review**. The agent reads `annotations.json`,
`annotated.png` and `review.md` and turns the notes into changes.

## Quick start

```bash
python3 review.py serve --open          # http://127.0.0.1:8788
python3 review.py open shot.png --title "Button / default" --url "http://localhost:5016/button" --note "Check hover and disabled"
```

`open` prints the session id and URL. Open the URL (the list page also shows every session) and
annotate. Sessions live in `sessions/<id>/` and are gitignored.

No third-party packages. Tested with Python 3.10+ (Windows, WSL/Linux).

## Agent workflow

```bash
python3 review.py open review/14-button/default.png \
  --title "Button / default" \
  --url "http://localhost:5016/button" \
  --note "Variants + hover/disabled" --json
# -> {"id": "...", "review_url": "http://127.0.0.1:8788/s/...", "dir": "..."}

python3 review.py wait <id> --timeout 600 --json
python3 review.py get <id> --json
```

Then read:

- `sessions/<id>/annotations.json` — normalized coordinates (`canvas` pixels), colors, text notes.
- `sessions/<id>/annotated.png` — visual reference.
- `sessions/<id>/review.md` — human-readable summary.

The agent should keep the docs app running and use the same screenshot dimensions as the page.

## Session format

```
sessions/<id>/
  meta.json          # title, url, note, status, timestamps, image file
  screenshot.png     # original screenshot
  annotations.json   # { image: {width,height}, annotations: [...] }
  annotated.png      # rendered annotations over the screenshot
  review.md          # markdown summary for the agent
```

Annotation kinds:

```json
{ "type": "rect",  "start": {"x":10,"y":20}, "end": {"x":100,"y":60}, "color": "#e11d48", "width": 3 }
{ "type": "arrow", "start": {"x":10,"y":20}, "end": {"x":100,"y":60}, "color": "#e11d48", "width": 3 }
{ "type": "pen",   "points": [{"x":10,"y":20}], "color": "#e11d48", "width": 3 }
{ "type": "text",  "x":10, "y":20, "text": "Move this up", "color": "#f59e0b" }
```

## Separate repository

This directory is its own git repository and can be pushed to a personal GitHub account:

```bash
git remote add origin git@github.com:<you>/au-visual-review.git
git push -u origin main
```

No company-specific code or paths are required. The optional `--sessions` flag points at any
shared folder if the agent and browser run on different machines (e.g. a Windows browser reading
the same checkout from WSL).

## Notes

- The server binds `127.0.0.1` by default; do not expose it publicly without adding auth.
- Screenshots and annotations stay local; `sessions/` is ignored by git.
- The browser needs the File System Access API only for the old standalone tool; this version
  submits through HTTP, so any modern browser works.
