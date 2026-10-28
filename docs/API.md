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
      "created_at": "2026-10-28T11:11:13+00:00",
      "submitted_at": null,
      "image": "screenshot.png"
    }
  ]
}
```

Newest first by `created_at`.

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
| `status` | `open` or `submitted`. |
| `created_at` / `submitted_at` | UTC ISO-8601 timestamps. |
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
