# HTTP API & data reference

The agent-facing contract (confirmation, optional handoff, `jobs` capability) is described in
[`INTEGRATIONS.md`](INTEGRATIONS.md). This file is the HTTP/data reference.

Base URL: `http://127.0.0.1:8788` by default. All API responses are JSON; static pages are HTML;
session images are served with the original content type. Authenticated data (everything under
`/api/*` plus session images/history) is sent with `Cache-Control: no-store`: a rail poll or a
dashboard load must never replay a list from before a feature was confirmed, cancelled or deleted.

## Endpoints

### `GET /`

Session list page (HTML).

### `GET /s/<id>`

Editor page (HTML). The page reads the id from the path and calls the JSON API below.

### `GET /api/sessions`

```json
{
  "sessions": [
    {
      "id": "20261028-111113-button-default-1a2b",
      "title": "Button / default",
      "url": "http://localhost:5016/button",
      "note": "Check hover",
      "status": "open",
      "feature_state": "pending",
      "feature_confirmed_at": null,
      "feature_cancelled_at": null,
      "handoff": null,
      "branch": "feat/button-hover",
      "created_at": "2026-10-28T11:11:13+00:00",
      "submitted_at": null,
      "image": "screenshot.png"
    }
  ]
}
```

Newest first by `created_at`. `feature_state` is derived: `confirmed` when
`feature_confirmed_at` is set, `cancelled` when `feature_cancelled_at` is set, otherwise
`pending`. A confirmed feature may also carry `handoff` (v0.23): the optional post-confirmation
work's state (`requested`, `running`, `done`, `failed`) plus the integration's `label`, `url` and
`message`. The server does not know what the work means; see
[Handoff a confirmed feature](#post-apisessionsidhandoff).

### `GET /api/sessions/<id>`

```json
{
  "meta": { "…": "as above" },
  "annotations": {
    "session": "…",
    "title": "…",
    "url": "…",
    "image": { "file": "screenshot.png", "width": 1440, "height": 900 },
    "annotations": [ … ]
  }
}
```

`annotations` is the last submitted payload (or `{session, annotations: []}` before submission).
The `meta` object carries `feature_confirmed_at` / `feature_cancelled_at` when the human decided.

### `PUT /api/sessions/<id>/branch`

Name the feature branch the review runs on.

```json
{ "branch": "feat/wrong-branches" }
```

Returns `{ "meta": { … } }`. `{"branch": null}` (or an empty string) restores the derived
default. `409` while the session is `submitted` (the agent's worktree was created for the old
name) or has a handoff record (the link points at the handed-off branch). A review created without
a name already has a branch — `feat/<title-slug>-<id-tail>` — so `session/<id>` is no longer
produced; sessions that v0.21 claimed under that fallback are read back under their `feat/…` name.

### `POST /api/sessions/<id>/confirm-feature`

The human says the feature is complete. Sets `status=resolved`, `resolved_at` (if unset) and
`feature_confirmed_at`, clears `feature_cancelled_at`. The optional body asks for a handoff:

```json
{ "handoff": true }
```

Without it (or with `false`) the server records only the decision and clears a stale failed/queued
handoff. With `true` it queues `handoff.status=requested` unless the handoff is already done.
RedLine does not run git or deploy anything: the integration claims the handoff job, performs its
own action, and reports it through the `handoff_status` MCP tool. Repeating the request retries a
failed handoff.

### `POST /api/sessions/<id>/handoff`

Queue (or retry) a handoff for a feature that is already confirmed. Returns `{ "meta": { … } }`.
`400` when the feature is not confirmed; a done handoff is returned unchanged. This is the retry
path behind the rail's `handoff failed ↻` badge and the way a pre-v0.23 confirmed feature can be
handed off. (The pre-release `/ship` spelling is kept as an alias.)

```json
{
  "handoff": {
    "status": "requested",
    "requested_at": "2026-10-31T10:00:00+00:00",
    "started_at": null,
    "finished_at": null,
    "label": null,
    "url": null,
    "message": null
  }
}
```

### `POST /api/sessions/<id>/cancel-feature`

The human abandons the feature. Sets `status=resolved`, `resolved_at` (if unset) and
`feature_cancelled_at`, clears `feature_confirmed_at` and `handoff`. Nothing is handed off and the
review files are kept.

### `POST /api/sessions/<id>/reopen`

Undo: `status=open`, clears `resolved_at`, `feature_confirmed_at`, `feature_cancelled_at` and
`handoff`. A new submission or agent round also clears the feature stamps and the handoff, so more
work makes the feature `pending` again.

### `GET /sessions/<id>/image`

Original screenshot bytes. Content type comes from the file extension. For a video session this
is the **poster** (`poster.png`), so the dashboard rail and gallery `<img>` cards keep working.

### `GET /sessions/<id>/media`

The video bytes for a video session (video sessions only). Supports HTTP `Range` and returns
`206 Partial Content` with `Content-Range`, so `<video>` can seek without downloading the whole
clip. `404` for image sessions or a missing file. Authenticated like every session route.

### `POST /api/sessions` (video)

Video sessions use the same endpoint with `video_base64` or `video_path` instead of the image
fields; `poster_base64`, `duration_ms`, `width`, `height` and `fps` are optional. The response meta
contains:

```json
{
  "image": "video.mp4",
  "media": {
    "kind": "video",
    "file": "video.mp4",
    "mime": "video/mp4",
    "width": 1280,
    "height": 720,
    "duration_ms": 5000,
    "fps": 30
  },
  "poster": "poster.png"
}
```

`image` still names the media file for older clients; `media.kind` is the authoritative switch.
Annotations may carry `at` (seconds) and optional `until`; `review.md` prints `mm:ss.mmm` timecodes.
MCP clients can use `review_video` instead of `review_screenshot`; `get_feedback` includes
`media` and `video_url`.

### `POST /api/sessions/video` (browser multipart)

The New review dialog uploads a video as `multipart/form-data`:

| Field | Required | Meaning |
|---|---|---|
| `video` (or `file`) | yes | MP4/WebM bytes |
| `poster` | no | first-frame PNG/JPEG for the rail/gallery card |
| `title`, `note`, `url`, `project`, `branch`, `agent` | no | same as `POST /api/sessions` |
| `duration_ms`, `width`, `height`, `fps` | no | metadata read by the browser |

Limits: 250 MB per video, 5 minutes when `duration_ms` is sent, 8 MB poster. Magic bytes decide
the container (HEIC/AVIF image brands are rejected); the uploaded filename never becomes the path.
Response is the same `{meta, review_url, dir}` shape as `POST /api/sessions`.

### `POST /api/sessions/<id>/video` (next round)

Same multipart fields as `POST /api/sessions/video` (only `video`, `poster`, `duration_ms`,
`width`, `height`, `fps` are used). The current round is archived - annotations, contact sheet,
frames and the clip under `history/<stamp>/video.*` - and the session reopens (`status=open`,
empty annotations) on the new clip. Used by the editor's **New video** button and MCP
`post_round`.

### `GET /sessions/<id>/frames/<file>`

A full-resolution still captured at an annotated time (`0.500.png`, `12.340.jpg`). `404` when the
round has no such frame. Frames are referenced from `annotations.json` (`frames[]`) and
`get_feedback`.

### `GET /sessions/<id>/history/<stamp>/video`

The archived clip of a submitted video round, with the same `Range`/`206` behaviour as the live
`/media` route. `/sessions/<id>/history/<stamp>/frames/<file>` serves that round's stills.

### `GET /sessions/<id>/annotated.png`

The last submitted annotated PNG. `404` before the first submission (or when the client did not
send a PNG).

### `POST /api/sessions/<id>/annotations`

Request:

```json
{
  "annotations": [ … ],
  "width": 1440,
  "height": 900,
  "png": "data:image/png;base64,…"
}
```

- `annotations`: array; may be empty.
- `width`/`height`: canvas dimensions; stored in meta + payload.
- `png`: optional; when present, decoded and written to `annotated.png`.

For a video submit use `multipart/form-data` instead of JSON:

| Field | Meaning |
|---|---|
| `annotations` | the JSON above **without** `png` |
| `png` | the composed contact sheet (PNG) |
| `frames` | repeated files named `<seconds>.png`/`.jpg`; at most 20 unique times / 60 MB |

The server writes `frames/`, stores their timecodes in `annotations.json`, archives them with the
round and lists them in `review.md` (`### Frames`). A JSON video submit without a sheet/frames is
still accepted.

Response `200`:

```json
{
  "ok": true,
  "meta": { "…": "status=submitted, submitted_at=…" },
  "annotations": { "…": "stored payload" }
}
```

`GET /api/sessions/<id>` also returns `disk_bytes`, the recursive size of the session folder
(including videos and history) for the 2 GB warning.

### `GET /api/sessions/<id>/history`

```json
{
  "submissions": [
    {
      "stamp": "2026-10-31T17-56-35-00-00",
      "round": 3,
      "current": false,
      "annotations": 1,
      "png": true,
      "source": true,
      "frames": 0,
      "started_at": "2026-10-31T17:56:35+00:00",
      "finished_at": "2026-10-31T18:04:12+00:00",
      "duration_secs": 457
    }
  ]
}
```

Newest first; `round` numbers oldest = 1. `started_at` is the submission stamp and `finished_at`
the moment the round left the agent's hands — the agent's next picture (`post_round`), the next
human submission, or `resolve` — so `duration_secs` is what the round actually cost. Both are
omitted while a round is still being worked on, and for v0.1 archives that carry no
`submitted_at`. The fields are additive: older clients ignore them.

Errors:

- `400` — invalid JSON / body too large (`{"error": "…"}`).
- `404` — unknown session.
- `500` — unhandled storage error (check server log).

### `GET /api/projects`

Projects are conversation areas; repository projects also carry a portable `repo` key (relative
to the agent's repo root).

```json
{
  "projects": [
    {
      "slug": "redline",
      "name": "redline",
      "agent": "holly",
      "repo": "redline",
      "max_parallel": null,
      "effective_max_parallel": 1,
      "created_at": "2026-10-29T18:20:00+00:00",
      "sessions": 4,
      "waiting": 1,
      "done": 3,
      "last_activity": "2026-10-29T18:30:00+00:00"
    }
  ]
}
```

`repo` is `null` for legacy label-only projects.

### `POST /api/agent/repos`

An agent reports the repositories it can see. The endpoint is idempotent and never deletes a
project that has reviews. `agent_id` is `[a-z0-9_-]{1,64}`; keys are relative paths without `.` or
`..`, at most 200 characters.

```http
POST /api/agent/repos
Authorization: Bearer <token>
Content-Type: application/json

{"agent_id":"holly","repos":[{"key":"redline","name":"redline"},{"key":"ActiUnit/Au.Net","name":"Au.Net"}]}
```

Returns `{"ok": true, "projects": [ … ]}`. Re-reporting the same key updates its `name`/`agent` and
keeps its slug, so sessions keep their project.

### Agent concurrency (v0.22)

The server is the authority for run limits and branch ownership. Defaults are **2 concurrent
runs** globally and **1 per project**; a branch always has at most one owner. A claim in
`waiting` state keeps the branch lock but does not consume a run slot.

```json
GET /api/agent/settings
{
  "max_parallel": 2,
  "project_max_parallel": 1,
  "lease_seconds": 120,
  "running": 1,
  "waiting": 0,
  "claims": [],
  "projects": [
    {"slug": "redline", "name": "redline", "repo": "redline",
     "max_parallel": null, "effective_max_parallel": 1}
  ]
}
```

`PUT /api/agent/settings` accepts any of `max_parallel`, `project_max_parallel`,
`lease_seconds`, and a `projects` map of `slug -> number|null` (`null` inherits the default).

```http
POST /api/agent/next       {"agent_id": "holly", "jobs": ["review", "handoff"]}
POST /api/agent/claims     {"session_id": "…", "agent_id": "holly", "work_key": "feat/x"}
POST /api/agent/claims/…/heartbeat   {"agent_id": "holly"}
POST /api/agent/claims/…/state       {"agent_id": "holly", "state": "waiting"}
POST /api/agent/claims/…/release     {"agent_id": "holly"}
```

`next` returns `{"session": {…}, "claim": {…}, "job": "review|handoff", "limits": {…}}` or
`{"session": null, "reason": "empty|at_capacity|branch_locked|claimed"}`. A claim response is
`{"claim": {…}, "session": {…}, "job": "review", "limits": {…}}`. The branch key is
`session.branch`, or the derived `feat/…` when the agent did not declare one.

`jobs` is the bridge's capability list. A bridge that omits it only receives `review` work, so
an old client is never handed a confirmed feature and asked to treat it as an annotation round.
`handoff` jobs are sessions with a claimable `handoff` and `feature_confirmed_at`; the response's
`session.handoff.status` is already `running`. The claim's `work_key` wins over `session.branch`:
it is the branch the review actually worked on, including v0.21's `session/<id>` legacy name.

The agent presence rows in `GET /api/agent/status` include `jobs` (default `["review"]`). A bridge
that advertises `["review","handoff"]` is the one that can be given a handoff job; an old bridge
that sends no `jobs` is review-only, and the UI says so in the status chip's tooltip.

### Agent presence

Every agent-facing call records a `last_seen`: `POST /api/agent/repos` is the periodic heartbeat
(the bridge calls it every 60 s), and `next`/claim calls update it too. `GET /api/agent/status`
adds the presence list to the settings/claims payload:

```json
{
  "online": 1,
  "agents": [
    {"agent_id": "holly", "last_seen": 1793000000, "online": true}
  ]
}
```

An agent is `online` for **180 s** after its last call (three missed repo-sync beats); the
presence file keeps its last-seen row for seven days. Presence is informational: RedLine never
blocks creating or submitting a review when no agent is connected.

### `GET /health`

`{"ok": true}`.

### Static

`GET /static/<file>`, `/`, `/s/<id>` and the editor redirects. Path traversal is rejected.

## Annotation schema

Coordinates are **canvas pixels** relative to the screenshot's natural size, origin top-left.

### Rectangle

```json
{
  "type": "rect",
  "start": { "x": 10, "y": 20 },
  "end": { "x": 100, "y": 60 },
  "color": "#e11d48",
  "width": 3
}
```

### Arrow

```json
{
  "type": "arrow",
  "start": { "x": 10, "y": 20 },
  "end": { "x": 100, "y": 60 },
  "color": "#e11d48",
  "width": 3
}
```

### Freehand pen

```json
{
  "type": "pen",
  "points": [{ "x": 10, "y": 20 }, { "x": 15, "y": 24 }],
  "color": "#e11d48",
  "width": 3
}
```

### Text note

```json
{
  "type": "text",
  "x": 10,
  "y": 20,
  "text": "Move this up",
  "color": "#e11d48"
}
```

Text is the highest-signal annotation; `review.md` lists text notes with coordinates. Shapes are
location hints. Colors/width are chosen in the toolbar; the agent should not infer semantics from
color unless the human states it.

## Meta schema

See `docs/ARCHITECTURE.md` § Storage schema. Fields:

| Field | Meaning |
|-------|---------|
| `id` | Session id (unique, URL-safe). |
| `title` | Human label (component / state). |
| `url` | Page under review. |
| `note` | Agent's request to the human. |
| `status` | `open`, `submitted` or `resolved`. |
| `created_at` / `submitted_at` | UTC ISO-8601 timestamps. |
| `resolved_at` | Present after resolve/confirm/cancel. |
| `feature_confirmed_at` | The human confirmed the feature; agents map this to their own workflow. |
| `feature_cancelled_at` | The human cancelled the feature; nothing should be handed off or merged. |
| `feature_state` | Derived in `GET /api/sessions`: `pending`, `confirmed` or `cancelled`. |
| `handoff` | Optional post-confirmation work (v0.23): `status` (`requested`, `running`, `done` or `failed`) plus the integration's `label`, `url` and `message`. The server never runs git or deploys; `done`/`failed` are the integration's own report. Cleared by a new round, reopen or cancel. |
| `branch` | The feature branch the review runs on. Every review has one: `feat/<title-slug>-<id-tail>` when nobody named it, and the branch-lock key. v0.21's `session/<id>` fallback is read back as that `feat/…` name. |
| `image` | Screenshot filename inside the session. |
| `image_width` / `image_height` | Present after submission; canvas dimensions. |

## curl examples

```bash
curl -s http://127.0.0.1:8788/api/sessions
curl -s http://127.0.0.1:8788/api/sessions/<id>
curl -s -X POST http://127.0.0.1:8788/api/sessions/<id>/annotations \
  -H 'Content-Type: application/json' \
  --data @submission.json
```

## Compatibility

- Additive fields are allowed; consumers should ignore unknown keys.
- Do not rename/remove existing fields without a migration note in `PLAN.md`.
- The CLI is the preferred agent interface; the HTTP API is for the browser and scripts.
- No auth: loopback only.
- Max request body: 60 MB.
