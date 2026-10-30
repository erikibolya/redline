# Video reviews — design and delivery plan

Status: **proposal**, waiting for product answers. Nothing in this document is implemented.

## 1. Goal

Let a human attach a video to a review, watch it in the canvas, scrub/pause on a frame, and draw
annotations that belong to a point in time (or a time range). The agent receives the annotations
with timecodes plus a visual reference of the annotated frames, so it can reproduce and fix the
exact moment instead of guessing from a single screenshot.

Typical use cases:

- a screen recording of a UI flow: "the flicker happens at 00:02.340" or "between 00:04.1 and
  00:04.8";
- a short animation or transition: "the easing is wrong here";
- a mobile/browser interaction: "the dropdown closes too early";
- an agent-produced recording after a fix, reviewed in the same session as the next round.

## 2. Scope for v1 (proposal)

In:

- upload a video from the New review dialog (file picker, drag & drop, paste if the browser
  provides a video file), with a poster/thumbnail;
- video playback in the editor: play/pause, scrub, step, current time/duration, timeline markers;
- annotations tied to a timestamp; optional `until` for a time range;
- drawing while paused (drawing auto-pauses playback);
- annotations visible at the current time, with timeline markers for all annotations;
- inspector rows with timecodes; clicking a row seeks the video;
- submit: annotations JSON + timecoded `review.md` + a labelled contact sheet of annotated frames;
- history: view a previous video round read-only, seek and inspect its annotations;
- MCP/API: create a video review, read timecoded feedback, post a new video round;
- poster image used by the dashboard gallery and rail;
- validation, size/duration limits, `Range` HTTP support for seeking.

Out of v1 (candidates for later):

- screen recording from RedLine itself;
- audio annotations/waveforms (audio playback exists, annotation stays visual);
- automatic video diff / frame-by-frame comparison between rounds;
- transcoding or codec conversion;
- timeline zoom for very long videos;
- multiple videos in one session;
- in-browser video editing/trimming;
- full-resolution frame extraction on every annotation (see §7 for the contact sheet decision).

## 3. UX

### 3.1 New review

- The drop zone and file input accept `image/*,video/*`; the preview shows a `<video>` poster and
  its duration/size instead of an `<img>` for videos.
- The title, project, branch and note fields stay the same.
- A warning for unsupported codecs or oversized files appears before upload.
- After create, the editor opens on the first frame; the video is paused.

### 3.2 Video editor

Layout: the existing stage/canvas gains a **video bar** under the stage, above the status bar (or
just under the canvas, never covering the image):

```
[▶/❚❚]  00:02.340 / 00:12.000   [◀ frame] [frame ▶]   0.5× 1× 2×   [mute]   ● ● ● markers
[────────────────────────────────────────────────────────────────────────────────────────]
```

- The video frame is drawn into the existing canvas at natural resolution; zoom/pan, fit and
  history viewer pan/zoom keep working.
- The timeline is a range control with annotation markers (colour = annotation colour, cluster
  when close). Hover shows time and count; click seeks.
- Annotation visibility: point annotations show at `abs(t - at) <= window` while paused, with a
  small window (e.g. 120 ms) while playing; range annotations show between `at` and `until`.
  A "show all annotations" toggle can be added later if this is too restrictive.
- Drawing: pointerdown on the canvas pauses the video first, then starts the shape at the current
  frame. The annotation gets the current `at` (and `until` if the user creates a range).
- Selecting an annotation seeks to its `at`; the inspector row shows `00:02.340` (or
  `00:02.340–00:02.600`).
- Keyboard: `Space` is already pan; use `K` play/pause, `,`/`.` step frame, `J`/`L` jump 1 s,
  `M` mute. Keep the existing shortcuts; document the conflict.
- When the video ends, pause at the last frame (do not loop by default).
- A still image session behaves exactly as today; the video bar is absent.

### 3.3 Submit and history

- Submit sends annotations with `at`/`until` and a contact sheet PNG; the server stores the
  annotations JSON, `review.md`, `annotated.png` (contact sheet) and the current video.
- History rows for video rounds show a play badge and duration. Opening a round loads the
  archived video (if the video changed) and its annotations; playback/seek work; drawing is
  refused, as in the image viewer.
- The conversation's round thumbnails use the poster (or the annotated contact sheet).
- The gallery card uses the poster, with a small `video` badge and duration.

## 4. Data model (additive, backwards compatible)

### 4.1 Meta

Keep the existing `image` field as the filename of the session media (image or video) so old
clients and agents still find a file. Add:

```json
{
  "media": {
    "kind": "image" | "video",
    "file": "screenshot.png" | "video.mp4",
    "mime": "image/png" | "video/mp4",
    "width": 1440,
    "height": 900,
    "duration_ms": 12340,
    "fps": 30.0
  },
  "poster": "poster.png"
}
```

- `media` is optional; missing means an image (legacy).
- `duration_ms` and `fps` are optional; fps may be unknown (variable frame rate). Time is stored
  in seconds with millisecond precision; frames are a display concern.
- `poster` is a generated first-frame (or selected) PNG used by cards; for images it is the image
  itself (or absent).
- `image_width` / `image_height` remain for compatibility and are set from the video dimensions.

### 4.2 Annotation

Add optional fields to each annotation:

```json
{
  "id": "...",
  "type": "rect",
  "color": "#e11d48",
  "width": 3,
  "start": {"x": 100, "y": 200},
  "end": {"x": 300, "y": 260},
  "at": 2.34,
  "until": 2.6
}
```

- `at`: seconds, `0 <= at <= duration`; required for video annotations; absent/0 for images.
- `until`: optional seconds, `>= at`; absent means a point annotation.
- Coordinates stay in video pixel space; all frames share the same dimensions.
- Existing annotations remain valid; the UI ignores time fields for images.

### 4.3 `annotations.json` / `review.md`

- `AnnotationSet` gains `media` (same shape as Meta.media) and keeps `image`.
- `review.md` groups marks by timestamp:

```md
## @ 00:02.340
- 1. rectangle (100, 200) → (300, 260) — “flicker starts”
- 2. freehand (5 points) — “the gap jumps”

## @ 00:04.100–00:04.800
- 3. arrow (500, 300) → (700, 320) — “dropdown closes”
```

- A still-image review keeps today's "Marked regions" section.

### 4.4 Storage layout

```
sessions/<id>/
  meta.json
  video.mp4              current media (or screenshot.png)
  poster.png             first frame / thumbnail
  annotations.json
  review.md
  annotated.png          contact sheet for video
  agent-thread.jsonl
  agent-events.jsonl
  history/
    <stamp>.json
    <stamp>.png          image rounds / contact sheets
    <stamp>.mp4          archived video when the media was replaced (moved, not copied)
```

Policy questions: whether to move or copy archived videos, retention (20 rounds today), and a
disk cap. Recommendation: **move** the old video into history when a new video is posted (no
duplicate bytes), keep a configurable per-session video budget, and never delete the current
video on an annotation-only resubmit.

## 5. Server and API

### 5.1 Upload

The current `MAX_BODY_BYTES` is 60 MB and the JSON path base64-encodes everything. Video needs a
streaming upload:

- `POST /api/sessions/{id}/media` with `multipart/form-data` (field `file`) streams to a temp file,
  validates magic bytes/MIME/extension, moves it into the session, writes `media`/`poster`.
- `POST /api/sessions` for a video creates the session with metadata and returns an upload URL
  (two-step), or accepts a small `video_base64` for tiny clips (<= 10 MB) for compatibility.
- MCP: `review_video` accepts a **path on the server** (`video_path`), a small `video_base64`, or
  a `url` fetched with a size/time limit; returns the session id and an upload URL if needed.
- The browser New review dialog uploads the file with `FormData` after creating the session (or
  in one multipart create request). Progress is shown; cancel removes the temp file.

### 5.2 Serving and seeking

- `GET /sessions/{id}/media` (or `/video`) streams the file with `Accept-Ranges`, `Content-Type`
  and `206 Partial Content` for `Range`. The current `serve_file` reads the whole file and has no
  range support; video seeking needs a range-capable handler (tower-http `ServeFile` or a small
  reader).
- `GET /sessions/{id}/image` keeps serving the image; for a video session it serves `poster.png`
  so the existing gallery/rail card code keeps working.
- `GET /sessions/{id}/annotated.png` remains the contact sheet for videos.
- Optional later: `GET /sessions/{id}/frame?at=2.340` (server-side frame extraction with ffmpeg)
  for agents/tools that want full-resolution frames.

### 5.3 Store / model

- `Store::create_video`, `Store::attach_media`, `Store::submit` (media-aware), `archive_round`
  (move video), `review_markdown` (timecodes).
- MIME validation by magic bytes, not just the extension; reject `text/html` and executables.
- `MAX_BODY_BYTES` stays for JSON; the multipart route has its own (configurable) limit and
  streams to disk.
- `ServeFile` / range tests; cache headers stay `no-store` for API, but video bytes may be cached
  with a valid range/ETag policy (seek performance); decide.

## 6. Browser/editor engine

Current editor model:

- `this.image` is an `Image`; `render()` does `ctx.drawImage(this.image, 0, 0)`;
  `fit()` uses `naturalWidth/naturalHeight`; `toImage()` maps screen → media coordinates.
- Add a `this.media` abstraction with `kind`, `element` (Image or HTMLVideoElement), width/height,
  duration; `loadMedia` creates the right element; `render()` draws the current video frame;
  `fit()` uses `videoWidth`/`videoHeight` when present.
- Playback loop: `requestVideoFrameCallback` when available, otherwise
  `requestAnimationFrame`; redraw only while playing/seek/draw.
- Auto-pause on drawing; annotation time = `video.currentTime` (rounded to ms).
- `history`/`undo` snapshot cost is small (annotations only); the video does not enter undo.
- Timeline: DOM overlay + markers; no framework, no build step. Use a `<input type="range">` for
  the scrubber and absolutely positioned marker buttons; accessible labels/timecode.
- Poster generation: draw the first frame into a canvas, `toBlob`, upload/keep `poster.png`.
- Contact sheet: when submitting a video, group annotations by their unique `at`/range, seek a
  cloned video element (or seek the main one sequentially), draw each frame into a grid with a
  timecode label and annotations, then encode one PNG. Cap frame count (e.g. 12) and tile size;
  the annotations JSON still has every timestamp.
- Browser codec support: MP4 H.264/AAC and WebM VP8/VP9 where the webview supports them; show a
  clear error if `video.error`/`canPlayType` fails.

## 7. Agent output / MCP

Proposed `get_feedback` for video:

```json
{
  "session_id": "...",
  "media": {"kind": "video", "duration_ms": 12340, "width": 1440, "height": 900, "fps": 30},
  "annotations": [{"at": 2.34, "until": 2.6, "type": "rect", "...": "..."}],
  "annotated_png": "redline://session/<id>/annotated.png",
  "review_md": "...",
  "video": "https://redline.../sessions/<id>/media",
  "frames": [
    {"at": 2.34, "resource": "redline://session/<id>/frames/2.340.png"},
    {"at": 4.10, "resource": "redline://session/<id>/frames/4.100.png"}
  ]
}
```

- v1: `annotated.png` is a labelled **contact sheet** of up to N annotated frames; this is the
  visual reference an LLM can inspect directly.
- `review.md` contains the timecodes and the same annotations in Markdown.
- The video URL is available for agents that can download and inspect it with their own tools.
- Later: per-frame full-resolution PNG resources generated by the server (needs ffmpeg) or
  uploaded by the browser; and a "frames" manifest for every unique timestamp.
- `post_round` accepts `video_path`/`video_base64`/`url`; `capture_round` remains image-only.
- MCP tool naming: add `review_video` rather than overloading `review_screenshot`; keep the old
  tool for images. The instructions in `initialize` mention both.

## 8. Server-side vs client-side frames

Option A (recommended v1): the browser creates the contact sheet on submit; no ffmpeg on the
server; works in the existing `:browser` image and in the desktop local server. The agent gets one
image with all annotated moments + JSON timecodes.

Option B: server-side ffmpeg extracts full frames at every annotation time; requires ffmpeg in
the runtime image (and a fallback for the slim image/desktop). Better for agents that need
full-resolution detail, but a new runtime dependency and a second rendering path.

Ask: is the contact sheet enough for the agent's first version, or is full-resolution per-frame
output required immediately?

## 9. History and retention

- Video files are large. **Move** the previous video to `history/<stamp>.mp4` when a new video
  round replaces it; do not copy on every annotation-only resubmit.
- Keep the existing 20-round retention? A 20×200 MB history is 4 GB per session. Propose a
  per-session video budget (e.g. 2 GB or 10 video rounds) and surface it in docs; ask for a limit.
- Deleting a session deletes all videos; `Delete cancelled` and the rail delete use the same
  recursive removal.
- The dashboard gallery uses posters, never decodes videos.

## 10. Security / limits

- Validate magic bytes, MIME and extension; reject HTML/SVG/executables.
- Stream uploads to a temp file; enforce max bytes while streaming; delete on failure.
- Range requests must be path-safe and support only the session's media file.
- Content-Type from a whitelist (`video/mp4`, `video/webm`, `video/quicktime` if supported).
- CSRF/auth: media is behind the existing token/cookie; `<video>` requests send the cookie.
- Filename is generated server-side; never use the uploaded name for the path.
- Privacy: videos contain screen content; retention/deletion policy; no external upload.
- Performance: cap resolution/duration; warn for 4K/long clips; the canvas draws at natural size
  (zoom does not upscale the decoded frame beyond the browser's decode).

## 11. Testing plan

Rust:

- store: create a video session, attach media, validate MIME, poster, duration, annotation time
  fields, `review.md` timecodes, history move (not copy), delete.
- API: multipart upload (small fixture), size/type rejection, `Range` responses (206, correct
  bytes, invalid range 416), cookie auth, video URL.
- MCP: `review_video` path/base64, `get_feedback` media/frames, `post_round` video, list.
- migration: old image sessions still load; `media` absent = image.

Browser e2e:

- fixture video (2 s, small WebM/MP4) in `desktop/scripts/fixtures/`;
- New review accepts it, shows duration/poster, creates a session;
- editor loads video, timeline appears, play/pause changes currentTime, seek marker;
- draw a rect at 1.2 s, assert `at` in the inspector and server JSON; pause on draw;
- create a range annotation (if approved) and assert `until`;
- contact sheet is uploaded and shows the timecode; `annotated.png` served;
- submit, `get_feedback` includes media + timecoded annotations; history round plays/seeks;
- image reviews still pass the full existing suite;
- codec error path (unsupported video) shows a clear message;
- no uncaught JS errors.

Visual: screenshot the editor paused at a timecode with the marker and annotation; contact sheet
tile; poster card.

## 12. Phased delivery (each phase = one branch/PR)

0. **Plan/schema ADR** — this document + answers; add the fixture; no production code.
1. **Server media core** — `media` model, upload/multipart, MIME/limits, poster, range serving,
   store/history, API/MCP read paths; Rust tests. No UI.
2. **Editor playback** — video load/draw/fit, video bar, timeline markers, pause/draw, annotation
   `at`/`until`, inspector timecodes; e2e with fixture.
3. **Submit and agent output** — contact sheet, `review.md` timecodes, `annotated.png`, MCP
   `review_video`/`get_feedback` frames, history viewer, poster cards.
4. **Polish and release** — desktop upload/codec/limits, accessibility/touch, docs, full e2e,
   version bump/release.

Estimated effort: phase 1–3 are each a multi-day change; phase 4 a day plus tests. The risky
parts are the streaming upload, range serving, and timeline/annotation UX consistency.

## 13. Risks

- **Codec support** in WebView2 vs Chromium; MP4 H.264 is safest; document what works.
- **Large files** and memory: never read a video through `serve_file`; stream and range.
- **History disk**: videos multiply; move, cap, and document.
- **Annotation clutter**: point-in-time visibility rules must be predictable; add a visible
  "current time" filter and timeline markers.
- **Seeking accuracy**: `currentTime` is approximate; step by a nominal frame (1/fps) and show
  ms; variable frame rate means frame count is a display hint, not truth.
- **Agent visual context**: a contact sheet may miss detail; provide the video URL and later
  full frames.
- **No-build UI**: keep the timeline CSS/JS dependency-free; test touch.

## 14. Questions for the product owner

Please answer these before phase 1 code. Recommended defaults are in parentheses.

1. **Annotation shape**: point-in-time only, time ranges only, or both? (both; `at` required,
   `until` optional)
2. **Video sources**: human file upload only, or also agent `video_path`/`video_base64`/URL? Is
   a screen recording made by RedLine itself in scope? (human file + agent path/base64; no
   recorder in v1)
3. **Formats/limits**: which codecs/containers must work (MP4 H.264/AAC, WebM VP8/VP9, MOV?), and
   what maximum duration/size/resolution? (MP4 + WebM, 5 min, 200 MB, 1080p)
4. **New video in the same review or a new session?** (same review as a new round; move the old
   video to history)
5. **History budget**: how many video rounds and/or GB per session should be kept? (10 rounds /
   2 GB; current video always kept)
6. **Annotations during playback**: pause automatically when drawing (yes), show point
   annotations only near their time and ranges during their range (yes), or show all annotations
   always? (current-time visibility)
7. **Timeline controls**: is play/pause + scrub + frame step + 0.5/1/2× enough for v1, or do you
   need timeline zoom, speed >2×, loop, or frame-accurate stepping for VFR? (v1 as listed; zoom
   later)
8. **Agent output**: is one labelled contact sheet + JSON timecodes + a video download URL enough,
   or do you need full-resolution PNGs for every annotated timestamp immediately? (contact sheet
   in v1; full frames later/optional)
9. **Audio**: play audio by default, mute toggle, or ignore audio entirely? (play with mute
   toggle; no waveform)
10. **Desktop**: must video work in the Windows WebView2 app, the browser UI, or both? Any
    codec restrictions from the desktops you use? (both; MP4 H.264 baseline)
11. **Time display**: `mm:ss.mmm` for humans and seconds in JSON, or frame numbers too? Is fps
    metadata available from the files? (ms display, seconds JSON, fps optional)
12. **Deletion/retention**: should deleting a review delete its videos immediately, and should
    there be a separate "purge videos" action? (delete with the session; no separate purge v1)

Once these are answered, I will turn the plan into phase 1 acceptance criteria and start the
server media core.
