# HTTP API & data reference

Base URL: `http://127.0.0.1:8788` by default. All API responses are JSON; static pages are HTML;
session images are served with the original content type.

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
`pending`.

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
default. `409` while the session is `submitted`: the agent's worktree was created for the old
name, so stop the round first. A review created without a name already has a branch —
`feat/<title-slug>-<id-tail>` — so `session/<id>` is no longer produced; sessions that v0.21
claimed under that fallback are read back under their `feat/…` name.

### `POST /api/sessions/<id>/confirm-feature`
The human says the feature is complete. Sets `status=resolved`, `resolved_at` (if unset) and
`feature_confirmed_at`, clears `feature_cancelled_at`. RedLine does not touch git; agents read
`feature_state == "confirmed"` and apply their own workflow (for this repo: open a PR into `dev`).

### `POST /api/sessions/<id>/cancel-feature`

The human abandons the feature. Sets `status=resolved`, `resolved_at` (if unset) and
`feature_cancelled_at`, clears `feature_confirmed_at`. Nothing is merged and the review files are
kept.

### `POST /api/sessions/<id>/reopen`

Undo: `status=open`, clears `resolved_at`, `feature_confirmed_at` and `feature_cancelled_at`.
A new submission or agent round also clears both feature stamps, so more work makes the feature
`pending` again.

### `GET /sessions/<id>/image`

Original screenshot bytes. Content type comes from the file extension.

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

Response `200`:

```json
{
  "ok": true,
  "meta": { "…": "status=submitted, submitted_at=…" },
  "annotations": { "…": "stored payload" }
}
```

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
POST /api/agent/next       {"agent_id": "holly"}
POST /api/agent/claims     {"session_id": "…", "agent_id": "holly", "work_key": "feat/x"}
POST /api/agent/claims/…/heartbeat   {"agent_id": "holly"}
POST /api/agent/claims/…/state       {"agent_id": "holly", "state": "waiting"}
POST /api/agent/claims/…/release     {"agent_id": "holly"}
```

`next` returns `{"session": {…}, "claim": {…}, "limits": {…}}` or
`{"session": null, "reason": "empty|at_capacity|branch_locked|claimed"}`. A claim response is
`{"claim": {…}, "session": {…}, "limits": {…}}`. The branch key is `session.branch`, or
`session/<id>` when the agent did not declare one.

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
| `feature_cancelled_at` | The human cancelled the feature; nothing should be merged. |
| `feature_state` | Derived in `GET /api/sessions`: `pending`, `confirmed` or `cancelled`. |
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
