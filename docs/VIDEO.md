# Video reviews — design and delivery plan

Status: **phase 1 implemented in `feat/video-media-core`** (server media core: media model,
agent `review_video` path/base64, poster, Range serving, timecoded annotations/review.md). The
editor, human multipart upload and frames/contact sheet are phase 2–3.

## 0. Decisions from the product owner (2026-10-31)

1. **Annotation model:** both point-in-time and time ranges (`at`, optional `until`).
2. **Sources:** human upload and agent-supplied video. The agent uploads an **existing clip**
   (`video_path`/`video_base64`/URL); RedLine itself does not record a browser session in v1
   (that is a separate `capture_video` follow-up).
3. **Limits:** delegated to engineering. Proposal: 5 min, 250 MB, 1080p recommended (4K accepted
   with a warning); no hard per-session cap in v1 — show disk usage instead.
4. **New video:** a new round in the same review; the previous video moves to history.
5. **History:** keep the existing 20-round retention; no separate video cap in v1, show size and
   warn for large sessions. Videos stay local.
6. **Annotation visibility:** current-time annotations full; other annotations are ghosts at low
   opacity and fade in/out as their time is approached (timeline markers always visible).
7. **Timeline:** basic controls plus a zoomable timeline (wheel/zoom + overview strip).
8. **Agent output:** contact sheet **and** full-resolution frames for the unique annotated
   timestamps; the cap is confirmed at **20 unique timestamps / ~60 MB per submission** (see §7).
9. **Audio:** no audio controls in v1; playback is muted.
10. **Desktop:** both the Windows WebView2 app and the browser UI.
11. **Time:** `mm:ss.mmm` for humans, seconds in JSON, fps optional when the file provides it.
12. **Deletion:** deleting a review removes its videos; no separate video purge.

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
- video playback in the editor: play/pause, scrub, frame step, current time/duration, speed,
  timeline markers, and a **zoomable timeline** with an overview strip;
- annotations tied to a timestamp; optional `until` for a time range;
- drawing while paused (drawing auto-pauses playback);
- annotations visible at the current time with full opacity; other annotations remain as faint
  ghosts and fade in as their time approaches; timeline markers are always visible;
- inspector rows with timecodes; clicking a row seeks the video;
- submit: annotations JSON + timecoded `review.md` + a labelled contact sheet **and**
  full-resolution frames for the unique annotated timestamps (bounded; see §7);
- history: view a previous video round read-only, seek and inspect its annotations;
- MCP/API: create a video review, read timecoded feedback, post a new video round;
- poster image used by the dashboard gallery and rail;
- validation, size/duration limits, `Range` HTTP support for seeking.

Out of v1 (candidates for later):

- screen recording from RedLine itself (the agent can supply an existing clip; see §14 Q1);
- audio controls/waveforms (playback is muted in v1);
- automatic video diff / frame-by-frame comparison between rounds;
- transcoding or codec conversion;
- multiple videos in one session;
- in-browser video editing/trimming.

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
[▶/❚❚]  00:02.340 / 00:12.000   [◀ frame] [frame ▶]   0.5× 1× 2×   [− zoom +] [Fit]
[────────────────────────────────────────────────────────────────────────────────────────]
overview strip (full duration, viewport window, annotation markers)
[────────────────────────────────────────────────────────────────────────────────────────]
detail timeline (zoomable window; scrubber, markers, time ruler)
```

- The video frame is drawn into the existing canvas at natural resolution; zoom/pan, fit and
  history viewer pan/zoom keep working.
- The timeline has two levels: a full-duration overview strip showing the viewport window, and a
  zoomable detail ruler. Wheel over the timeline zooms around the pointer; drag pans the window;
  `Fit` returns to the full duration. Marker positions, the scrubber and the time labels all
  respect the current window. Basic controls stay usable without zooming.
- Annotation markers (colour = annotation colour, clustered when close) live on both strips.
  Hover shows time and count; click seeks.
- Annotation visibility: the annotation(s) at the current time are full opacity. Other
  annotations are ghosts at low opacity (proposal: 12–15%) so the geometry is visible without
  clutter; as playback approaches an annotation's `at` (within ~350 ms), it fades to full, and a
  finished point annotation fades back to ghost. Range annotations are full between `at` and
  `until` and ghost outside, with a short fade at both boundaries. A "ghosts" toggle can hide
  them entirely; timeline markers always show every annotation.
- Drawing: pointerdown on the canvas pauses the video first, then starts the shape at the current
  frame. The annotation gets the current `at` (and `until` if the user creates a range).
- Selecting an annotation seeks to its `at`; the inspector row shows `00:02.340` (or
  `00:02.340–00:02.600`).
- Keyboard: `Space` is already pan; use `K` play/pause, `,`/`.` step frame, `J`/`L` jump 1 s.
  Timeline zoom uses `+`/`-` or Ctrl+wheel when the pointer is over the timeline (not the canvas,
  where Ctrl+wheel still zooms the image). Keep the existing shortcuts; document the conflict.
- Audio is not exposed in v1: playback is muted, no waveform or mute control.
- When the video ends, pause at the last frame (do not loop by default).
- A still image session behaves exactly as today; the video bar is absent.

### 3.3 Submit and history

- Submit sends annotations with `at`/`until`, a labelled contact sheet PNG and the full-resolution
  frames for the unique annotated timestamps (bounded; see §7). The server stores the annotations
  JSON, `review.md`, `annotated.png` (contact sheet), `frames/NNN.png` and the current video.
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
  frames/                full-resolution PNGs for unique annotated timestamps
  agent-thread.jsonl
  agent-events.jsonl
  history/
    <stamp>.json
    <stamp>.png          image rounds / contact sheets
    <stamp>.mp4          archived video when the media was replaced (moved, not copied)
```

Policy: **move** the old video into history when a new video is posted (no duplicate bytes);
keep the current video on annotation-only resubmits; keep the existing 20-round retention and show
session disk usage (warning at 2 GB) rather than silently deleting videos.

## 5. Server and API

### 5.1 Upload

The current `MAX_BODY_BYTES` is 60 MB and the JSON path base64-encodes everything. Phase 1 ships
the agent path/base64 path; phase 2 adds the streaming browser upload:

- `POST /api/sessions` with `video_base64`/`video_path` (phase 1) creates a video session;
  `video_base64` is limited by the JSON body limit, `video_path` is for agents on the server.
- `POST /api/sessions/{id}/media` with `multipart/form-data` (field `file`) streams to a temp file
  (phase 2, browser): validates magic bytes/MIME/extension, moves it into the session, writes
  `media`/`poster`.
- `POST /api/sessions` for a video can return an upload URL (two-step) in phase 2; small
  `video_base64` stays for compatibility.
- MCP: `review_video` accepts a **path on the server** (`video_path`), a small `video_base64`, or
  a `url` fetched with a size/time limit; returns the session id and an upload URL if needed.
- The browser New review dialog uploads the file with `FormData` after creating the session (or
  in one multipart create request). Progress is shown; cancel removes the temp file.

### 5.2 Submit

- Image reviews keep the existing JSON submit (`annotations` + one base64 `png`).
- Video reviews use a multipart submit on `POST /api/sessions/{id}/annotations`:
  - `annotations` — the JSON `Submission` (with `at`/`until`),
  - `png` — the contact sheet,
  - `frames` — zero or more full-resolution PNG files named by their timecode
    (`2.340.png`), capped per §7.
  The server writes `frames/` and reuses the existing submission/archive path. A JSON-only video
  submit is still accepted for agents/tests with annotations but no visual frames; the server
  then has no contact sheet to serve and says so.

### 5.3 Serving and seeking

- `GET /sessions/{id}/media` (or `/video`) streams the file with `Accept-Ranges`, `Content-Type`
  and `206 Partial Content` for `Range`. The current `serve_file` reads the whole file and has no
  range support; video seeking needs a range-capable handler (tower-http `ServeFile` or a small
  reader).
- `GET /sessions/{id}/image` keeps serving the image; for a video session it serves `poster.png`
  so the existing gallery/rail card code keeps working.
- `GET /sessions/{id}/annotated.png` remains the contact sheet for videos.
- Optional later: `GET /sessions/{id}/frame?at=2.340` (server-side frame extraction with ffmpeg)
  for agents/tools that want full-resolution frames.

### 5.4 Store / model

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
- Submit (video): group annotations by their unique `at`/range, seek a cloned video element (or
  seek the main one sequentially), draw each frame into a grid with a timecode label and the
  annotations, encode the contact sheet, and capture each unique annotated time as a
  full-resolution PNG. Cap the unique frames at 20 and the total at ~60 MB; if the cap is hit,
  keep the contact sheet and the first N frames and note it in `review.md`.
- Browser codec support: MP4 H.264/AAC and WebM VP8/VP9 where the webview supports them; show a
  clear error if `video.error`/`canPlayType` fails. Audio is muted; no audio controls.

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

- **Contact sheet vs full frames.** They solve different needs:
  - a **contact sheet** is one image with a grid of annotated moments, each tile labelled with a
    timecode — quick for a human/LLM to see the whole story at a glance;
  - a **full frame** is a full-resolution PNG of one annotated moment — exact pixels for reading
    small text, checking a color, or seeing a detail the thumbnail hides.
- v1 ships **both**: `annotated.png` is the labelled contact sheet, and `frames/<at>.png` contains
  full-resolution frames for each unique annotated timestamp. Cap the number (e.g. 20 unique
  timestamps) and total bytes (e.g. 60 MB) per submission; if the cap is exceeded, keep the
  contact sheet and the first N frames, and say so in the response/review.md.
- The frames are generated in the browser on submit (the video and annotations are already
  there), not by ffmpeg on the server; this keeps the slim/desktop image dependency-free. The
  upload uses the multipart submit path because multiple PNGs do not fit in the single base64
  `png` field.
- `review.md` contains the timecodes and the same annotations in Markdown.
- The video URL is available for agents that can download and inspect it with their own tools.
- Later: server-side ffmpeg extraction for every annotated frame at full resolution without a
  browser submission, and a per-frame cache.
- `post_round` accepts `video_path`/`video_base64`/`url`; `capture_round` remains image-only.
- MCP tool naming: add `review_video` rather than overloading `review_screenshot`; keep the old
  tool for images. The instructions in `initialize` mention both.

## 8. Server-side vs client-side frames

Option A+ (chosen v1): the browser creates the contact sheet **and** the full-resolution frames
for the unique annotated timestamps on submit; no ffmpeg on the server; works in the existing
`:browser` image and in the desktop local server. The submit endpoint accepts a multipart form
with the annotations JSON, contact sheet and frame files; the server stores them and exposes
`redline://session/<id>/frames/<time>.png` resources. The caps in §7 keep the payload sane.

Option B (later): server-side ffmpeg extracts frames from the video at annotation times; requires
ffmpeg in the runtime image (and a fallback for the slim image/desktop). Useful for re-generating
frames or for agents that submit videos without a browser, but a second rendering path and a new
runtime dependency.

## 9. History and retention

- Video files are large. **Move** the previous video to `history/<stamp>.mp4` when a new video
  round replaces it; do not copy on every annotation-only resubmit.
- Keep the existing 20-round retention; no separate video cap in v1. Show the session's video
  bytes in the UI/API and warn when a session grows large (the default budget is a warning, not a
  deletion). The owner's filesystem remains the real limit; videos stay local.
- Deleting a session deletes all videos, frames and posters; `Delete cancelled` and the rail
  delete use the same recursive removal.
- The dashboard gallery uses posters, never decodes videos.

## 10. Security / limits

- Validate magic bytes, MIME and extension; reject HTML/SVG/executables.
- **Limits (v1):** max 5 minutes, 250 MB per file, MP4 H.264/AAC and WebM VP8/VP9; 1080p
  recommended, 4K accepted with a warning. Limits are configurable
  (`REDLINE_MAX_VIDEO_BYTES`, `REDLINE_MAX_VIDEO_SECONDS`); the client checks duration/size before
  upload, the server enforces bytes while streaming. No hard per-session cap; warn at 2 GB.
- Stream uploads to a temp file; enforce max bytes while streaming; delete on failure.
- Range requests must be path-safe and support only the session's media file.
- Content-Type from a whitelist (`video/mp4`, `video/webm`; MOV/QuickTime only if the browser can
  play it).
- CSRF/auth: media is behind the existing token/cookie; `<video>` requests send the cookie.
- Filename is generated server-side; never use the uploaded name for the path.
- Privacy: videos contain screen content; retention/deletion policy; no external upload.
- Audio is not exposed in v1; playback is muted.
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
- timeline zoom: wheel/`+` changes the visible window, markers/rescale, Fit resets; overview
  window follows;
- ghost visibility: an annotation at another time is faint, then becomes full near its time and
  fades back; the ghosts toggle hides it;
- draw a rect at 1.2 s, assert `at` in the inspector and server JSON; pause on draw;
- create a range annotation and assert `until`;
- contact sheet is uploaded and shows the timecode; full-resolution frames for unique timestamps
  are stored and served; `annotated.png` served;
- submit, `get_feedback` includes media + timecoded annotations + frame resources; history round
  plays/seeks;
- image reviews still pass the full existing suite;
- codec error path (unsupported video) shows a clear message;
- no uncaught JS errors.

Visual: screenshot the editor paused at a timecode with the marker and annotation; a zoomed
timeline; the contact sheet tile; a full frame; the poster card.

## 12. Phased delivery (each phase = one branch/PR)

0. **Plan/schema ADR** — this document + answers; add the fixture; no production code.
1. **Server media core** — `media` model, agent video path/base64 (`review_video`), MIME/limits,
   poster, Range serving, timecoded annotations/`review.md`; Rust tests. No UI. *(implemented in
   `feat/video-media-core`; browser multipart upload moves to phase 2)*
2. **Editor and browser upload** — multipart streaming upload from the New review dialog, video
   load/draw/fit, video bar, basic controls, zoomable timeline with overview strip and markers,
   ghost/fade annotation visibility, pause/draw, annotation `at`/`until`, inspector timecodes;
   e2e with fixture.
3. **Submit and agent output** — contact sheet + full-resolution frames for unique annotation
   times, `review.md` timecodes, `annotated.png`, MCP `review_video`/`get_feedback` frames,
   history viewer, poster cards.
4. **Polish and release** — desktop upload/codec/limits, accessibility/touch, docs, full e2e,
   version bump/release.

Estimated effort: phase 1–3 are each a multi-day change; phase 4 a day plus tests. The risky
parts are the streaming upload, range serving, timeline zoom/ghost rendering, and the multi-image
submit (contact sheet + full frames).

## 13. Risks

- **Codec support** in WebView2 vs Chromium; MP4 H.264 is safest; document what works.
- **Large files** and memory: never read a video through `serve_file`; stream and range.
- **History disk**: videos multiply; move them, show usage, warn at 2 GB; no silent deletion.
- **Annotation clutter**: ghosts + fade must be predictable; a ghosts toggle and timeline markers
  keep the current frame readable.
- **Seeking accuracy**: `currentTime` is approximate; step by a nominal frame (1/fps) and show
  ms; variable frame rate means frame count is a display hint, not truth.
- **Agent visual context**: a contact sheet may miss detail; ship full-resolution frames for the
  unique annotated times as well.
- **No-build UI**: keep the timeline CSS/JS dependency-free; test touch.

## 14. Open questions

None. The product owner confirmed:

- the agent uploads an existing clip in v1; a RedLine-owned `capture_video` recorder is a
  follow-up feature;
- the 20 unique timestamps / ~60 MB full-frame cap is sufficient.

The plan is locked; the next step is phase 1 (server media core).


