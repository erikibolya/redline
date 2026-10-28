# redline

**Redline the screenshot. The agent writes the fix.**

A local, dependency-free visual review queue for human ↔ AI collaboration.

The AI agent creates a **session** with a screenshot and context. You open it in a browser, draw
rectangles, arrows, freehand marks and text notes, then click **Submit review**. The agent reads
`annotations.json`, `annotated.png` and `review.md`, turns every note into a code change, and asks
for confirmation with a follow-up session.

This repository is intentionally separate from the product repository. It can be pushed to a
personal GitHub account and has no company-specific code or paths.

> **Desktop app (preview).** Installers live on the
> [releases page](https://github.com/erikibolya/redline/releases): a tray application that embeds
> this server plus an MCP endpoint, so agents connect directly and no terminal stays open. It runs
> standalone on your desktop or as a thin client of a server next to your agents (Unraid/Docker).
> See [`docs/DESKTOP.md`](docs/DESKTOP.md).
>
> Server image (Unraid/NAS/Docker): `docker pull ghcr.io/erikibolya/redline:latest` — bind
> `/data`, publish `8788`, set `REDLINE_TOKEN`. The Windows app then connects to it from the tray.
> Everything below describes v0.1, which still works.

## Requirements

- Python **3.10+** (stdlib only — no `pip install` needed); use `python3` on WSL/Linux and `python` on Windows
- A modern browser
- Optional: a shared checkout if the browser and agent run on different machines

## Quick start

```bash
python3 redline.py serve --open          # http://127.0.0.1:8788
```

Create a session (normally the agent does this):

```bash
python3 redline.py open shot.png \
  --title "Button / default" \
  --url "http://localhost:5016/button" \
  --note "Check hover, focus and disabled" \
  --json
```

The command prints the session id and review URL. Open the URL (the session list is at `/`) and
annotate. Sessions live in `sessions/<id>/` and are gitignored.

## Agent workflow

```bash
# 1. create a review session from a screenshot
python3 redline.py open review/14-button/default.png \
  --title "Button / default" \
  --url "http://localhost:5016/button" \
  --note "Variants + hover/disabled" --json

# 2. open the printed review_url in the browser / Review pane

# 3. wait for the human to submit (blocks until submitted or timeout)
python3 redline.py wait <id> --timeout 600 --json

# 4. read the result
python3 redline.py get <id> --json
```

The result is deliberately redundant:

| File | Purpose |
|------|---------|
| `annotations.json` | Machine-readable source of truth: annotation types, coordinates, colors, text notes. |
| `annotated.png` | The screenshot with the drawn annotations burned in — visual reference. |
| `review.md` | Human/agent-readable summary with page URL, request note and text notes. |

A session becomes `submitted` only when the human clicks **Submit review**. Re-submitting updates
the files and timestamp.

## Human workflow

1. Open the review URL.
2. Use **Rectangle**, **Arrow**, **Pen** or **Text**.
   - Text: click a spot and type the note in the prompt.
   - `Ctrl/Cmd+Z` removes the last annotation.
3. Click **Submit review**. The page confirms the submission; the agent can now read it.
4. You can add more notes and submit again; the agent sees the latest submission.

## CLI reference

```text
python3 redline.py serve [--host 127.0.0.1] [--port 8788] [--sessions DIR] [--open]
python3 redline.py open IMAGE [--title T] [--url URL] [--note TEXT] [--id ID]
                             [--sessions DIR] [--host H] [--port P]
                             [--json] [--open-browser]
python3 redline.py list [--sessions DIR] [--json]
python3 redline.py get ID [--sessions DIR] [--json]
python3 redline.py wait ID [--sessions DIR] [--timeout 600] [--interval 2] [--json]
python3 redline.py remove ID [--sessions DIR] [--yes]
```

- Default sessions directory: `./sessions` next to `redline.py`.
- Default server: `127.0.0.1:8788`.
- `open` creates the folder and copies the image; `--id` must be unique.
- `wait` exits **2** on timeout; `get` exits **1** if the session does not exist.
- `--json` makes CLI output machine-readable.

## HTTP API

The server is intentionally small. All responses are JSON except the static pages/images.

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/` | Session list page. |
| `GET` | `/s/<id>` | Annotation editor for a session. |
| `GET` | `/api/sessions` | `{ "sessions": [meta, …] }`, newest first. |
| `GET` | `/api/sessions/<id>` | `{ "meta": …, "annotations": … }`. |
| `GET` | `/sessions/<id>/image` | Original screenshot bytes. |
| `GET` | `/sessions/<id>/annotated.png` | Last submitted annotated PNG. |
| `POST` | `/api/sessions/<id>/annotations` | Submit annotations (JSON body, see below). |
| `GET` | `/health` | `{ "ok": true }`. |

Submit body:

```json
{
  "annotations": [ … ],
  "width": 1440,
  "height": 900,
  "png": "data:image/png;base64,…"
}
```

`png` is optional; when present the server writes `annotated.png`. `width`/`height` are the canvas
dimensions and are stored in `meta.json`/`annotations.json`.

## Session format

```text
sessions/<id>/
  meta.json          # id, title, url, note, status, created_at, submitted_at, image, dimensions
  screenshot.png     # original screenshot (extension preserved)
  annotations.json   # { session, image: {file,width,height}, annotations: […] }
  annotated.png      # rendered annotations over the screenshot
  review.md          # markdown summary for the agent
```

Annotation objects (canvas pixels, origin top-left):

```json
{ "type": "rect",  "start": {"x":10,"y":20}, "end": {"x":100,"y":60}, "color": "#e11d48", "width": 3 }
{ "type": "arrow", "start": {"x":10,"y":20}, "end": {"x":100,"y":60}, "color": "#e11d48", "width": 3 }
{ "type": "pen",   "points": [{"x":10,"y":20}], "color": "#e11d48", "width": 3 }
{ "type": "text",  "x":10, "y":20, "text": "Move this up", "color": "#e11d48" }
```

## Cross-machine setup (Windows browser + WSL/agent)

If the browser and agent do not share a host, point both at the same checkout or shared folder:

```bash
# agent side
python3 redline.py open ... --sessions /mnt/user/Development/repos/redline/sessions

# browser side (Windows)
python redline.py serve --sessions Z:\repos\redline\sessions
```

The browser edits files in the shared folder; the agent reads them directly. The server does not
need to be reachable from the agent in this mode.

## Personal GitHub

Published at **https://github.com/erikibolya/redline** (public, MIT). The remote is already set up:

```bash
git push origin main            # publish changes
gh repo view --web              # open the repo in a browser
```

`gh` lives at `~/.local/bin/gh` and is authenticated over HTTPS, so `git push` needs no prompts.
If credentials ever expire, run `gh auth login --web --git-protocol https` and enter the device
code at https://github.com/login/device.

## Testing

```bash
python3 -m unittest discover -s tests -t . -v
```

The tests cover session creation/listing, submission output and session-id validation.

## Security & limitations

- Binds `127.0.0.1` by default; no authentication. Do not expose publicly.
- Single-user, file-based queue; concurrent submissions are last-write-wins.
- Screenshots are copied into `sessions/`; keep them out of git (already ignored).
- No zoom/pan, annotation selection, or inline text editing yet (see `PLAN.md`).
- No screenshot capture yet: the agent must supply an image; a Playwright capture command is planned.
- Annotated coordinates are canvas pixels; if the screenshot is scaled on screen, the editor maps
  pointer coordinates back to the original canvas.

## Repository layout

```text
redline.py                 # CLI + stdlib HTTP server + session store
static/list.html          # session list
static/index.html         # annotation editor
tests/test_redline.py      # unit tests
docs/ARCHITECTURE.md      # design and data flow
docs/API.md               # endpoint + payload reference
docs/WORKFLOW.md          # human/agent review protocol
AGENTS.md                 # instructions for the next agent session
PLAN.md                   # roadmap, decisions, known work
sessions/                 # local sessions (gitignored)
```

## License

MIT — see `LICENSE`.
