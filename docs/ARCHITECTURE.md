# Architecture

## Goals

- One screenshot → one annotated review, with no external services.
- The agent can create a session and read the result without the server running.
- The human edits in a normal browser; no build step, no dependencies.
- Cross-platform (Windows browser + WSL agent via a shared folder).

## Non-goals

Cloud hosting, accounts, real-time collaboration, issue-tracker sync, automatic visual diffing.
Those are roadmap items (`PLAN.md`), not assumptions in the current code.

## Components

```
┌─────────────┐      CLI: open/list/get/wait      ┌──────────────────────┐
│  AI agent   │ ─────────────────────────────────▶ │  sessions/<id>/      │
│  (any host) │ ◀───────────────────────────────── │  meta.json + images  │
└─────────────┘      reads JSON/PNG/MD             └──────────────────────┘
        │                                                    ▲
        │ browser URL / API                                  │ HTTP submit
        ▼                                                    │
┌──────────────────────────────┐                  ┌──────────┴───────────┐
│ redline.py serve              │  static HTML/JS  │  Human browser       │
│ stdlib ThreadingHTTPServer   │ ───────────────▶ │  canvas editor       │
└──────────────────────────────┘                  └──────────────────────┘
```

- **`redline.py`** is the only Python module: storage, CLI and HTTP in one file. Keeping it single-file
  makes it copy-paste portable.
- **`static/list.html`** fetches `/api/sessions` and renders links.
- **`static/index.html`** is the editor. It loads meta/annotations, draws on a canvas over the
  screenshot, and POSTs the result.
- **`sessions/`** is the database. The agent can read it directly; the server is optional for reads.

## Session lifecycle

```
open ──▶ status=open ──(human submit)──▶ status=submitted ──(resubmit)──▶ status=submitted
                                                        │
                                                        └─ files updated, submitted_at refreshed
```

- `open` (CLI) copies the screenshot and writes `meta.json` + an empty `annotations.json`.
- A video session stores `video.mp4` (or `.webm`), `poster.png`, and `media` metadata in
  `meta.json`; `GET /sessions/<id>/media` serves the clip with HTTP `Range`, while `/image` serves
  the poster so the rail/gallery cards keep working.
- The browser loads the session; annotations are kept only in the page until **Submit review**.
- Submit writes `annotations.json`, `annotated.png`, `review.md` and flips `meta.status`.
  Video annotations may carry `at`/`until` seconds; `review.md` prints timecodes.
- `wait` polls `meta.status`; no push/websocket.

## Data flow

1. Agent captures a screenshot and runs `redline.py open`.
2. Session folder is created; `meta.json` stores title/url/note/status.
3. Agent opens `http://127.0.0.1:8788/s/<id>` (browser tool or human).
4. Editor fetches `/api/sessions/<id>`, then loads `/sessions/<id>/image` (a poster for a video
   session; the clip itself comes from `/sessions/<id>/media` with `Range`).
5. Human draws; canvas keeps an in-memory annotation array. A video frame is drawn into the same
   canvas and drawing pauses playback, so each annotation carries `at` (and optional `until`).
6. Submit sends `{annotations, width, height, png}` to the server.
7. Server validates the session id, writes the three output files, returns the updated meta.
8. Agent polls/reads the folder; turns text notes into changes.

## Coordinate model

- Screenshot is drawn into a canvas at its **natural pixel dimensions**.
- CSS may scale the canvas down (`max-width:100%`); pointer coordinates are multiplied by
  `canvas.width / getBoundingClientRect().width`.
- Therefore annotation coordinates are always relative to the original image, origin top-left.
- If the screenshot is a full-page capture, coordinates are absolute in that image; if it is a
  viewport capture, they are relative to the viewport. The session title/note should say which.

## Storage schema

`meta.json`:

```json
{
  "id": "20261028-111113-button-default-1a2b",
  "title": "Button / default",
  "url": "http://localhost:5016/button",
  "note": "Check hover and disabled",
  "status": "open | submitted | resolved",
  "created_at": "2026-10-28T11:11:13+00:00",
  "submitted_at": null,
  "resolved_at": null,
  "feature_confirmed_at": null,
  "feature_cancelled_at": null,
  "ship": null,
  "branch": "feat/button-hover",
  "image": "screenshot.png",
  "image_width": 1440,
  "image_height": 900
}
```

`ship` is present only after the human confirms a feature (v0.23). It records the ship run:
`status` (`requested`, `running`, `opened`, `merged`, `failed`, `closed`), the PR URL/number,
the merge commit and the agent's error. The review run never opens a PR; confirmation queues this
record and the bridge's ship job fills it in.

`annotations.json` (written on submission):

```json
{
  "session": "…",
  "title": "…",
  "url": "…",
  "image": { "file": "screenshot.png", "width": 1440, "height": 900 },
  "media": { "kind": "image", "file": "screenshot.png", "width": 1440, "height": 900 },
  "annotations": [],
  "submitted_at": "…"
}
```

`review.md` is generated from meta + annotations. Text notes include their coordinates; rectangles
include start/end/color/width; other shapes point to the JSON.

### Agent concurrency state

Next to `sessions/` the data directory holds `agent-settings.json` (global max, default
per-project max, lease) and `agent-claims.json` (one claim per active session). Defaults are 2
global runs, 1 per project and a 120 s lease. A claim is `running` or `waiting`; `waiting` keeps
the branch lock but does not consume an execution slot.

The server is the authority. The bridge asks `POST /api/agent/next`, heartbeats, and marks
`waiting` when the human has the ball. The branch key is `Meta::work_key()`: `meta.branch`, or
the derived `feat/<title-slug>-<id-tail>` for a v0.21 `session/<id>` fallback; RedLine never
touches git. The bridge advertises `jobs: ["review","ship"]` on `next`; the response's `job`
selects the review prompt or the ship prompt. A ship claim reuses the waiting review claim's
`work_key` (the branch the agent actually worked on) and marks `meta.ship.status=running`.
`redline-agent` runs `REDLINE_AGENT_WORKERS` claim workers (default 4), each with its own worktree under
`<repo_root>/.redline-worktrees/<repo>-<hash>/<branch>-<hash>`, and running heartbeats every
`lease/3`. A resolved round releases the claim and removes a clean worktree; a dirty one is left
for inspection. The OpenCode server must see the worktree path just like the repository itself.
The feature branch starts from `refs/heads/dev`, then `refs/heads/main`, then their `origin/`
twins, and finally `HEAD`; the candidates are **fully qualified refs**, because a short name such
as `dev` becomes ambiguous as soon as a tag of that name exists (and git then either refuses it or
silently branches off the tag).

## HTTP server

- `ThreadingHTTPServer` with a single handler class; class-level `sessions` path.
- Static files are served from `static/` with a path-traversal check.
- Session image route resolves the image name from `meta.json` (extension preserved).
- JSON bodies are capped at 60 MB (`MAX_BODY`), enough for a large annotated screenshot.
- No authentication; loopback by default. Binding non-loopback is an explicit human choice.

## Cross-machine

The server only needs filesystem access to `sessions/`. Two supported modes:

1. **Same host:** agent starts the server; browser opens `127.0.0.1:8788`.
2. **Split:** agent writes sessions in a shared checkout; the human runs `redline.py serve
   --sessions <shared>` on Windows. The agent reads the same files directly; no network route is
   required between agent and browser.

## Tradeoffs

- **File-based over DB:** trivial portability and offline reads; no transactions/ordering guarantees.
- **Canvas-only annotations:** simple and fast; annotation objects are not editable after drawing
  (undo/clear only) until M2.
- **No screenshot capture:** keeps dependencies zero; Playwright capture is an opt-in M3 feature.
- **Prompt-based text:** minimal UI code; inline editor is M2.

## Extension points

- Add CLI commands in `build_parser()` + `cmd_*` functions.
- Add routes in `Handler.do_GET`/`do_POST`; keep JSON additive.
- Add annotation kinds in `static/index.html` (`drawAnnotation`, pointer handlers) and document in
  `docs/API.md`; `review.md` should summarize each kind.
- Add tests under `tests/`; run with `python3 -m unittest discover -s tests -t .`.
