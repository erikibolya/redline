# Browser capture extension — design and delivery plan

Status: **Phases 1–5 implemented** (2026-11-01). Screenshots and recorded flows are annotated in
the side panel; the store ZIP, privacy policy, listing copy and an extension e2e CI job are ready.
Store accounts and publishing are the only remaining owner actions.

A Chromium extension that connects to a RedLine server, captures a screenshot or records a flow
from the current tab, collects the console and network evidence, and lets the human annotate the
capture in a side panel before submitting it to RedLine as a normal review. The agent reads the
annotations plus a structured trace.

Decisions from the product owner (2026-11-01): **Chrome + Edge first**, annotation **in the
extension side panel**, active-tab capture, console/network **metadata + actions**, 5 min / 250 MB,
and distribution through the **Chrome Web Store / Edge Add-ons**.

## 1. Goals

- **One click from the page to a RedLine review.** The extension knows the current tab, the
  RedLine server and the project; the human does not paste URLs or files.
- **Screenshot review.** Capture the visible tab (and later full page), open it in the side-panel
  editor, annotate, and submit.
- **Flow review.** Record the tab as WebM while collecting:
  - console messages, uncaught exceptions and browser log entries;
  - network requests and responses (method, URL, status, timing, resource type, failure);
  - user actions (click, input/change, submit, navigation) with stable-ish selectors;
  - manual markers the human drops while recording.
- **Annotate in the side panel.** A compact canvas editor over the screenshot or video frame,
  with a timeline and the trace events, then **Submit to RedLine** (the same API the web editor
  uses). The full RedLine editor remains one click away for advanced work.
- **Agent-ready evidence.** `review.md` gets a compact trace summary; `get_feedback` exposes the
  trace metadata and URL; the raw `trace.json` is available as an MCP resource.
- **Local and private.** The token, video and trace stay on the user's RedLine server. No third
  party sees the capture; headers/cookies/request bodies are redacted or omitted by default.

## 2. Non-goals for v1

- Whole-desktop or multi-tab recording (one tab per recording; screen capture is a later mode).
- Firefox/Safari support (Chromium MV3 only in v1; a Firefox port is a separate plan).
- Editing/trimming the video in the extension.
- Full RedLine editor parity in the side panel: groups, z-order, align/distribute, diff overlay,
  ranges (`until`) and history browsing stay in the RedLine tab for v1. The panel ships the core
  drawing tools, notes, timecoded point annotations, timeline and submit.
- Automatic replay, DOM diffing or "fix this for me".
- Capturing requests made before the user presses Record.
- Full request/response bodies and headers by default.

## 3. Product UX

### 3.1 Connect

The options page stores one RedLine server profile:

| Field | Meaning |
|---|---|
| Server URL | `https://redline.ibolya.cz` or `http://127.0.0.1:8788` |
| Token | the server token; stored in `chrome.storage.local`, never synced |
| Project | optional; loaded from `GET /api/projects` |
| Branch | optional; the server derives `feat/<title-slug>-<id-tail>` when empty |
| Redaction | header/query patterns; request bodies off by default |

**Test connection** calls `/health` and `/api/sessions`; a `401` opens the token field with a
hint. The token is never rendered unmasked after saving; **Replace** and **Forget** are explicit.

### 3.2 Popup

The toolbar popup shows the current tab's title/URL and four actions:

- **Screenshot & annotate** — visible-tab capture; creates an image review and opens it in the
  **side-panel editor**.
- **Record flow** — asks for the capture options (console, network, actions), shows a countdown,
  then records. While recording the popup shows elapsed time and a **Mark** button.
- **Stop & review** — stops recording, shows the draft form (title, note, project, branch,
  capture counts, redaction summary), then opens the side-panel editor on the video.
- **Open RedLine** — the server dashboard.

The popup may be closed while recording; the toolbar badge shows `REC` and the elapsed minute.
Keyboard shortcuts: `Alt+Shift+S` screenshot, `Alt+Shift+R` start/stop recording,
`Alt+Shift+M` mark the current moment.

### 3.3 Draft and upload

After stop, before upload:

```
Flow: Checkout — 00:42
[recording thumbnail]
Console 14 (2 errors) · Network 38 (1 failed) · Actions 9 · Markers 2
Redaction: authorization, cookie, set-cookie; bodies off
Title [Checkout fails after coupon]
Note  [The total stays stale]
Project [ActiUnit / Au.Net]  Branch [feat/checkout-coupon]
[Continue to annotator] [Discard]
```

The upload uses `POST /api/sessions/video` with `video`, `poster`, `trace`, the form fields and
the media metadata. The session is created **open**; the side panel loads it for annotation. The
extension does **not** wake the agent until the human presses **Submit to RedLine**, which uses
the same annotation API as the web editor. **Open in RedLine** is always available for advanced
editing.

The Phase 2 screenshot path is shorter: the popup creates the image session immediately with the
tab title and the configured project, then opens the side panel where the note is edited. An
editable title/project draft before creation is recording-phase polish; the side panel already
lets the human discard the session.

The Phase 3 recording path also uses the tab title and configured project; the note is edited in
RedLine until the phase-4 video panel lands. The draft form above is the target UX, not a claim
that every field is editable today.

### 3.4 Side-panel annotator

A compact editor, not a copy of the full RedLine shell:

```
┌──────────────────────────────────────────────┐
│ Checkout — 00:42          [Open in RedLine]  │
│ ┌──────────────────────────────────────────┐ │
│ │            canvas (image/frame)          │ │
│ │                                          │ │
│ └──────────────────────────────────────────┘ │
│ [V] [▭] [↗] [✎] [T]   color  width  [undo]  │
│ [▶] 00:02.340 / 00:42.000  [◀ frame] [▶]    │
│ [trace: errors ▾]  ● 00:01.700 console error │
│ ──────────────────────────────────────────── │
│ Annotations (3)   ● 00:00.000  ● 00:02.340   │
│ Note for the agent: [.....................]  │
│ [Submit to RedLine]        [Open in RedLine] │
└──────────────────────────────────────────────┘
```

- Tools: select/move, rectangle, arrow, pen, text note. Colour/width; undo/redo; delete.
  Ellipse, groups, align/distribute and ranges (`until`) stay in the RedLine tab for v1.
- Screenshot canvas: natural pixel coordinates, zoom/pan, fit; compose the annotated PNG on
  submit.
- Video: draw the current frame into the canvas; drawing pauses playback; each annotation gets
  `at` (point-in-time); the clip seeks by clicking a marker or an annotation row. Capture one
  frame per unique annotated time and a contact sheet for the submit, using the browser canvas.
- Trace strip: console errors, failed requests and actions; clicking seeks. The full trace panel
  is a list under the stage, filterable by kind.
- Submit: image sessions use the existing JSON endpoint with `png`; video sessions use the
  multipart submit with `annotations`, `png` (contact sheet) and `frames`. After a successful
  submit, the panel says the review is queued for the agent and offers **Open in RedLine**.
- Drafts: keep an unsent annotation set in `chrome.storage.session` keyed by session id, so a
  service-worker restart or panel close does not lose it. Clear on submit/discard.

## 4. Architecture

```
┌────────────────────────────── Chrome / Edge ──────────────────────────────┐
│ popup / options / side-panel annotator (plain HTML+JS)                    │
│        │ chrome.runtime messages                                           │
│ background service worker ── chrome.debugger ── CDP: Network, Runtime, Log  │
│        │                         │                                         │
│        │                         └─ console, exceptions, requests          │
│        ├── content script (all frames) ── click/input/submit/scroll events  │
│        ├── offscreen document ── tabCapture + MediaRecorder ── WebM + poster│
│        └── trace buffer ── redact / normalize / cap ── trace.json           │
└───────────────────────────────────┬─────────────────────────────────────────┘
                                    │ HTTPS + Bearer
                                    ▼
                         RedLine server (Rust/axum)
             POST /api/sessions              POST /api/sessions/video
             POST /api/sessions/<id>/annotations (image/video submit)
             GET  /sessions/<id>/trace       GET /api/sessions/<id> (meta.trace)
                                    │
                                    ▼
                    RedLine rail/gallery/history + full editor
                    Agent: review.md + get_feedback + MCP resource
```

The extension is a **capture client plus a compact annotator**. RedLine remains the source of
truth: it owns the session store, the full editor, the review status and the agent queue. The
side panel is a focused editing surface; advanced work is one click away in the RedLine tab.

## 5. Capture pipeline

### 5.1 Screenshot

1. `chrome.tabs.captureVisibleTab` for the visible viewport.
2. Optional full page (`Page.captureScreenshot` with `captureBeyondViewport`) behind a
   `debugger` attach; a full-page screenshot can be much larger, so cap dimensions/bytes.
3. `POST /api/sessions` with `image_base64`, title, URL, note, project and branch.
4. Open the session in the side-panel annotator; submit from there or open the full RedLine
   editor.

### 5.2 Recording

1. User presses Record on the active tab.
2. The service worker attaches `chrome.debugger` and enables `Network`, `Runtime`, `Log` and
   `Page`; it filters RedLine's own host and `chrome-extension://` traffic to avoid a feedback
   loop.
3. The offscreen document obtains the tab stream with `chrome.tabCapture.getMediaStreamId` and
   records `video/webm;codecs=vp9` (VP8 fallback) at ~2.5 Mbps / 30 fps with MediaRecorder.
4. A content script (`all_frames`) captures user actions. It records a selector, tag, role,
   accessible label and safe value only; never passwords or typed secrets.
5. All timestamps are milliseconds from a shared `t0`; the trace is sorted and capped while
   recording. The offscreen document reports the actual recorder start time so video and trace
   share a clock.
6. Stop: stop MediaRecorder, detach the debugger, stop the content script, build the poster,
   read the elapsed duration, and open the side-panel annotator.
7. The extension uploads the video, poster and `trace.json`; it does not upload while the
   debugger is attached, so the upload itself is not part of the trace.

WebM/MediaRecorder caveat: a WebM produced by MediaRecorder can report `Infinity` duration until
it is seeked. The extension uses the measured recorder elapsed time for `duration_ms` and fixes
the element duration before generating the poster, following the lesson in `AGENTS.md` §7.

### 5.3 What is captured

| Kind | Default | Details |
|---|---|---|
| Console | on | `console.*`, exceptions, browser log entries; text capped at 2 KB each |
| Network | on | method, redacted URL, status, resource type, start time, duration, failure; no bodies |
| Headers | off | names only by default; values only with explicit opt-in and redaction |
| Bodies | off | request/response bodies opt-in, max 64 KB each, redacted |
| Actions | on | click, input/change (values masked for sensitive fields), submit, navigation |
| Scroll | off | high volume; opt-in with a throttled sample |
| Markers | on | manual timestamp + label from the popup/shortcut |

The trace never contains `Authorization`, `Cookie`, `Set-Cookie`, `X-Api-Key` or
`Proxy-Authorization` values; query parameters matching `token`, `key`, `secret`, `password`,
`code` and `session` are redacted. The extension shows the redaction summary before upload.

Caps: 5,000 events, 10 MB trace JSON, 2 KB event text. When a cap is reached the trace gets
`"truncated": true` and the draft warns; the video continues.

## 6. Trace format (`trace.json`)

Versioned, additive, server-validated. The client sends raw events; the server derives the
summary (do not trust a client-provided count).

```json
{
  "version": 1,
  "kind": "browser-flow",
  "started_at": "2026-11-01T14:00:00.000Z",
  "duration_ms": 42123,
  "page": {"url": "https://app.example.test/checkout", "title": "Checkout"},
  "browser": {
    "name": "Chrome",
    "version": "143.0.0.0",
    "platform": "Windows",
    "viewport": {"width": 1440, "height": 900}
  },
  "redaction": {
    "headers": ["authorization", "cookie", "set-cookie"],
    "query": ["token", "key", "secret", "password", "code"],
    "bodies": false
  },
  "truncated": false,
  "events": [
    {
      "t": 1200,
      "kind": "action",
      "action": "click",
      "selector": "[data-testid=apply-coupon]",
      "tag": "button",
      "label": "Apply coupon"
    },
    {
      "t": 1500,
      "kind": "network",
      "method": "POST",
      "url": "https://app.example.test/api/coupon",
      "status": 500,
      "duration_ms": 120,
      "resource": "fetch",
      "failed": false
    },
    {
      "t": 1700,
      "kind": "console",
      "level": "error",
      "text": "Uncaught TypeError: cannot read properties of undefined"
    },
    { "t": 2000, "kind": "navigation", "url": "https://app.example.test/checkout" },
    { "t": 2100, "kind": "marker", "label": "total did not update" }
  ]
}
```

Event kinds:

| `kind` | Required fields | Optional |
|---|---|---|
| `console` | `t`, `level`, `text` | `source`, `line`, `url` |
| `exception` | `t`, `text` | `stack` (capped) |
| `network` | `t`, `method`, `url` | `status`, `duration_ms`, `resource`, `failed`, `error` |
| `action` | `t`, `action`, `selector` | `tag`, `role`, `label`, `value` (masked) |
| `navigation` | `t`, `url` | — |
| `marker` | `t`, `label` | `note` |

- `t` is an integer millisecond offset from `started_at`; events are sorted by `t`.
- URLs are absolute and redacted; request bodies are opt-in and base64/text with `body_encoding`.
- No HTML, no DOM snapshots and no cookies in v1. A DOM snapshot is a later feature (it is large
  and contains secrets).
- `version` starts at 1; unknown fields are ignored by old clients.

## 7. RedLine server changes

### 7.1 Data model

Add to `Meta` (additive, omitted when absent):

```json
{
  "trace": {
    "file": "trace.json",
    "kind": "browser-flow",
    "events": 42,
    "started_at": "2026-11-01T14:00:00.000Z",
    "duration_ms": 42123,
    "summary": {
      "console": 14,
      "console_errors": 2,
      "network": 38,
      "network_failed": 1,
      "actions": 9,
      "markers": 2
    },
    "redacted": true,
    "truncated": false
  }
}
```

`AnnotationSet` gets the same `trace` reference for the submitted round. `trace.json` lives in the
session folder; a new video round moves it to `history/<stamp>/trace.json` with the old clip.

### 7.2 API

- `POST /api/sessions/video` and `POST /api/sessions/<id>/video` accept an optional `trace` file
  field (JSON, max 10 MB) alongside `video`, `poster` and the existing metadata fields.
- `GET /sessions/<id>/trace` serves the current `trace.json`; history rounds get
  `GET /sessions/<id>/history/<stamp>/trace`.
- `GET /api/sessions` and `GET /api/sessions/<id>` include the `trace` summary.
- Validation: known `kind` values, event count/size caps, all `t <= duration_ms + 2 s`, URL
  schemes `http(s)`, `ws(s)`, `data:`, `blob:` and `file:`, no unknown top-level event fields
  that could smuggle binary data.
- An invalid trace rejects the upload with `400`; the video is not stored.
- A trace can also be attached to an image session later via `POST /api/sessions/<id>/trace`;
  v1 only uses it with video.

### 7.3 `review.md`

Add a **Browser trace** section after the media summary:

```md
## Browser trace
- 42 s · 9 actions · 38 requests (1 failed) · 14 console messages (2 errors) · 2 markers
- Errors:
  - @ 00:01.700: Uncaught TypeError: cannot read properties of undefined
- Failed requests:
  - @ 00:01.500: POST /api/coupon → 500 (120 ms)
- Key actions:
  - @ 00:01.200: click "Apply coupon"
```

The full trace stays in `trace.json`; the Markdown is a summary, not a dump. `get_feedback`
includes `trace` and `trace_url`; MCP resources gain
`redline://session/<id>/trace.json`. The raw trace is never inlined into `get_feedback` (size and
secret risk).

### 7.4 Extension auth and CORS

An extension fetch is cross-origin. `require_token` currently refuses every
`Sec-Fetch-Site: cross-site` request. The server will:

- allow a cross-site request only when it has a valid Bearer token **and** either an
  `Origin: chrome-extension://…`/`moz-extension://…` or an explicit
  `X-RedLine-Client: browser-extension` header;
- return `Access-Control-Allow-Origin: <extension origin>`, `Vary: Origin`,
  `Access-Control-Allow-Headers: authorization, content-type, x-redline-client`,
  `Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS` for extension origins;
- answer `OPTIONS` preflight with `204` for extension origins only;
- keep rejecting cross-site web pages, even with a token in a URL;
- never enable `Access-Control-Allow-Credentials`.

Tests: web cross-site → `403`; extension origin + token → `200`; extension origin without token
→ `401`; web origin preflight → `403`; extension preflight → `204`.

## 8. Side-panel annotator and RedLine UI

### 8.1 Side-panel annotator (primary)

- **Stage**: a canvas at the screenshot/video's natural pixel size; zoom/pan/fit. For a video,
  the current frame is drawn into the canvas; drawing pauses playback.
- **Tools**: select/move, rectangle, arrow, pen, text note; colour/width; delete; undo/redo.
  Ellipse, groups, align/distribute and ranges stay in the RedLine tab for v1.
- **Coordinates**: RedLine annotation schema, canvas pixels relative to the natural media size
  (the same model as `docs/API.md` § Annotation schema). The extension reuses the existing
  `{type,start,end,points,color,width,at}` fields; no new annotation format.
- **Video bar**: play/pause, current/total time, frame step, scrubber, annotation markers and
  trace markers. Clicking an annotation or trace row seeks. Drawing captures `at`.
- **Trace strip**: under the stage, filter `Errors / All / Console / Network / Actions /
  Markers`; each row shows timecode + compact label; click seeks; expanding shows details. The
  default is errors + failed requests, so the panel is useful without noise.
- **Annotation list**: compact rows with timecode (for video) and note; click seeks/selects.
- **Note**: one note per selected shape (the RedLine `note` field) plus an overall review note
  (the submission `note`). The full editor's per-shape inspector is still available via **Open in
  RedLine**.
- **Submit**: image → JSON `POST /annotations` with composed `png`; video → multipart submit with
  annotations, contact sheet and one frame per unique annotated time. The panel shows upload
  progress and a success state with `Open in RedLine`.
- **Drafts**: per-session annotation draft in `chrome.storage.session`; flush on panel hide and
  clear on submit/discard. A changed media fingerprint invalidates the draft.
- **Accessibility**: keyboard tool shortcuts, focus rings, aria labels; at minimum the panel is
  usable without a mouse for submit/cancel.

### 8.2 RedLine UI (v1 minimal)

- The rail/gallery card gets a `flow` chip with duration/error count (additive metadata).
- The full editor shows a read-only **Flow trace** summary in the inspector when `meta.trace`
  exists: counts, console errors, failed requests, and a link to open the trace JSON.
- The interactive trace timeline in the full editor is **phase 2**; v1 keeps the full trace in
  the extension panel and on disk for the agent. This avoids duplicating the timeline UI before
  the annotator is proven.
- History rows keep their existing shape; archived traces are available on disk/API but no new
  history trace UI in v1.

The RedLine UI remains dependency-free (`app.js` + `app.css`), served from the binary. The
extension editor is a separate vanilla JS file; it must share the annotation JSON schema but not
the shell.

## 9. Extension structure

```
extension/
  manifest.json            MV3, Chrome/Edge, side panel
  src/
    background.js          service worker: state, debugger, upload orchestration
    offscreen.html/.js     tabCapture + MediaRecorder + poster
    content.js             action events (all frames), overlay, Mark
    sidepanel.html/.js     annotator shell, tools, timeline, trace, submit
    popup.html/.js         connect status, screenshot, record, draft
    options.html/.js       server/token/project/redaction
    lib/
      api.js               auth fetch, multipart, project list, submit
      editor.js            canvas engine, tools, annotations, undo
      media.js             video frame drawing, frames, contact sheet, poster
      trace.js             normalize, redact, cap, summary
      selectors.js         stable selector + label
      format.js
  icons/                   RedLine icon sizes
  tests/
    trace.test.mjs
    redact.test.mjs
    selectors.test.mjs
    editor.test.mjs
```

No build step: ES modules in the service worker, plain HTML/JS pages. Development uses an
unpacked extension; the release target is the **Chrome Web Store and Edge Add-ons**. The same
zip is also attached to the GitHub release for internal/testing installs.

Store requirements we must prepare:

- a single, clear purpose: "capture a page or tab flow and submit it to your own RedLine server";
- a privacy policy URL (the repo can host `docs/PRIVACY-EXTENSION.md` via GitHub Pages) stating
  that captures go only to the user's configured RedLine server, what is stored locally, and the
  redaction defaults;
- permission justifications for `debugger`, `tabCapture`, `activeTab`, `scripting`, `storage`,
  `sidePanel` and the optional host permission for the configured server;
- screenshots, 128/48/16 px icons, a short and long description, and support URL;
- a Chrome Web Store developer account and an Edge Partner Center account (one-time fees and
  identity verification are the owner's steps);
- a review build with no remote code, no analytics and no third-party endpoints.

The `debugger` permission is the most scrutinised one; the store listing must explain that it is
only attached after an explicit Record action and is used solely to read console/network events
for the review the user is creating.

Store cost (2026-11-01): the Chrome Web Store charges a **one-time $5** developer registration
fee; Microsoft Edge Add-ons registration is **free**. Both require an owner account and identity
verification; Edge review can take up to seven business days. The owner does not have the accounts
yet, so packaging/listing is prepared first and publishing is a later, explicit step.

Manifest permissions (proposed): `activeTab`, `scripting`, `tabs`, `storage`, `debugger`,
`tabCapture`, `offscreen`, `sidePanel`, `alarms`; optional host permission for the configured
RedLine server; `activeTab` grants the recorded page. `debugger` shows a browser warning; the
extension only attaches after an explicit Record click and explains why.

## 10. Delivery phases

| Phase | Work | Exit |
|---|---|---|
| 0. Spike | MV3 skeleton, extension-origin auth, side panel, tabCapture + MediaRecorder, debugger console/network events | A throwaway recording uploads to a local RedLine and plays in the panel |
| 1. Server trace | `TraceRef`/`Meta.trace`, storage, multipart `trace`, routes, validation, `review.md` summary, MCP feedback/resource, auth/CORS | Rust tests green; a curl upload creates a trace session |
| 2. Screenshot + image editor | options/popup, token/project config, visible-tab capture, side-panel canvas editor, image submit, draft | Screenshot → annotate → submit works entirely in the panel |
| 3. Recording + trace | offscreen recorder, debugger collectors, content actions, redaction, draft/upload, poster/duration | A real flow records, uploads and plays with trace |
| 4. Video annotator + trace UI | video canvas/timeline, timecoded annotations, trace strip/filters, frames/contact sheet, submit | A recorded flow is annotated and submitted from the panel |
| 5. Hardening + store | caps, tab-close/detach handling, error states, extension e2e, privacy policy, store assets/listing | Release checklist green and store submission ready |

Rough effort: 2–3 days for the spike, 3 for server trace, 4 for screenshot + image editor, 5 for
recording, 5 for video annotator/trace, 3 for hardening/store. About four working weeks, excluding
store review time.

## 11. Testing

- **Rust**: trace validation and caps; multipart trace upload; storage/history; `review.md`
  summary; `get_feedback` and resource; extension-origin auth/preflight; web cross-site still
  denied; existing video tests remain green.
- **Extension unit (`node --test`)**: trace normalization, redaction (headers/query/body),
  selector generation, cap/truncation, duration formatting, canvas annotation serialization,
  undo/redo and draft invalidation.
- **Browser e2e (RedLine)**: seed a session with a small trace; assert the rail `flow` chip,
  the read-only trace summary, and that `get_feedback` includes `trace`; no extension needed.
- **Extension e2e**: load the unpacked extension in headful Chromium (Xvfb in the e2e image or a
  separate job), open a local page with a console error and a failing fetch, record 2–3 s, upload
  to the test server, open the side panel, draw a rectangle, submit, and assert the session has
  video + trace + annotation + the expected `get_feedback` fields. If browser automation of
  `tabCapture` proves unreliable, keep the unit tests and a documented manual checklist as the
  gate, and add the automation as a follow-up — do not claim coverage that does not run.
- **Manual**: install the store build in Chrome and Edge; capture a real app flow; verify
  console/network redaction; annotate and submit; verify the agent sees the trace.

## 12. Risks and mitigations

- **`debugger` warning** is scary. Explain it in onboarding; attach only while recording; show a
  visible REC overlay; detach on stop. Content-script-only mode is a later fallback, not v1.
- **Secrets in console/network.** Default to metadata, redact headers/query, bodies opt-in and
  capped; show a pre-upload summary. The video itself can still contain secrets; the user sees
  the draft and can discard.
- **Recording size/duration.** 2.5 Mbps + 5 min cap stays under 250 MB; warn near the limit.
  Longer flows need chunked/streamed upload in a later phase.
- **Clock alignment.** Use a shared `t0`; store recorder start; allow ±2 s validation. A visible
  countdown can improve alignment if needed.
- **Cross-origin iframes/workers.** CDP captures network/console from frames; content actions
  use `all_frames`. Service-worker console may be incomplete in v1; document.
- **Cross-site auth.** The extension-origin allowlist must not become a web-page bypass. Test
  with a malicious web origin and without `X-RedLine-Client`.
- **Browser API drift.** MV3 service worker suspension, offscreen lifetime and `tabCapture`
  permissions are the fragile parts; keep state in `chrome.storage.session` and test a 60 s
  recording with the popup closed.

## 13. Decisions and open items

Taken (2026-11-01):

| Decision | Choice |
|---|---|
| Browsers | Chrome + Edge (Chromium MV3) |
| Annotation | Extension side panel |
| Capture scope | Active tab |
| Trace depth | Console/network metadata + actions; bodies off |
| Recording limit | 5 min / 250 MB |
| Distribution | Chrome Web Store + Edge Add-ons |

Taken after the review:

| Decision | Choice |
|---|---|
| Annotator tools | v1 core: select/move, rectangle, arrow, pen, text note, timecoded point annotations, undo/redo, draft, submit; advanced tools stay in RedLine |
| Project/branch | choose a project in the popup; leave branch empty so the server derives it; allow an override |

Still open:

1. **Store accounts.** The owner has no Chrome Web Store or Edge Partner Center account yet
   (Chrome: one-time $5; Edge: free). Prepare the package, listing text, screenshots and
   `docs/PRIVACY-EXTENSION.md`; publishing happens only after an explicit owner action.

## 14. Implementation status

Phase 1 (server trace foundation) is on `feat/browser-extension`:

- `Meta.trace` / `AnnotationSet.trace` (`TraceRef`, `TraceSummary`, `TraceHighlight`).
- `validate_trace`: 10 MB / 5,000-event / text / timestamp / kind validation; the server derives
  the summary and highlights instead of trusting the client.
- Multipart `trace` on `POST /api/sessions/video` and `POST /api/sessions/<id>/video`; invalid
  traces fail before a session is created.
- `GET /sessions/<id>/trace` and `/history/<stamp>/trace`; the trace moves with the archived
  video round.
- `review.md` Browser trace section; `get_feedback` `trace`/`trace_url`; MCP
  `redline://session/<id>/trace.json`.
- Extension-origin auth/CORS: token + extension Origin (or `X-RedLine-Client`) bypasses the
  cross-site guard; web cross-site stays `403`; extension preflight is `204`, web preflight `403`.
- Tests: 125 Rust tests green (3 new trace tests + auth preflight coverage).

Phase 2 (options/popup, screenshot capture, side-panel image editor) is implemented on
`feat/extension-panel`:

- MV3 options page with server/token/project/branch, origin-scoped host permission request and a
  connection test; token in `chrome.storage.local` only.
- Popup capture flow: visible-tab PNG, create an image session, open the side panel, fall back to
  a normal review tab when the side panel is unavailable.
- Side panel: select/move, rectangle, arrow, pen, text note, colour/width, undo/redo, delete,
  clear, numbered list, per-session draft, annotated PNG composition and submit.
- Submitted reviews open read-only; **Open in RedLine** remains available.
- Eighteen extension unit tests (`node --test`) for annotation geometry, the API request shape,
  URL/permission config, and trace redaction, wired into the Linux + Windows CI test job.

Phase 3 (tab recording) is implemented on `feat/extension-recording`:

- `tabCapture` + MediaRecorder in an offscreen document; the video Blob never crosses the
  extension message channel.
- `chrome.debugger` attaches only while recording and collects console/exceptions, network
  metadata and navigation; a content script (`all_frames`) records click/change/submit actions.
- URL/text redaction, a 5,000-event cap and `chrome.storage.session` persistence for a
  service-worker restart.
- Stop builds the poster, uploads multipart video + trace, stores the active capture and opens the
  review. The phase-4 video annotator is still read-only in the panel; use RedLine for video
  annotations until it lands.

Phase 4 (video annotator and trace panel) is implemented on `feat/extension-video`:

- The side panel draws the current video frame into the same canvas; drawing pauses playback and
  tags the annotation with `at`.
- Timeline playhead, frame step, annotation markers, trace markers, a Ghosts toggle and a
  filterable console/network/action trace panel.
- Submit captures one annotated frame per unique time (max 20), composes a labelled contact sheet
  and posts the video multipart payload. A submitted round remains readable and seekable.
- The side-panel annotator was smoke-tested in headless Chromium against a local RedLine server:
  two timecoded rectangles, frames at 0.000/1.500 and a successful submit. Trace upload itself
  still needs a dev server with Phase 1; the server tests cover its API.

Phase 5 preparation is in this branch:

- `extension/scripts/package.py` builds `dist/redline-extension-<version>.zip` with only
  `manifest.json`, `src/` and `icons/` (no external zip dependency).
- `docs/PRIVACY-EXTENSION.md` and `docs/STORE-EXTENSION.md` cover the privacy disclosure,
  permission justifications and listing copy.
- The release workflow attaches the ZIP to every GitHub release.
- The store accounts are not created (Chrome: $5 one-time; Edge: free), so publishing is still
  the owner's explicit action.
- `extension e2e` runs in CI: it loads a temporary host-permitted extension in headless Chromium,
  configures a local RedLine, submits an image annotation, then a video + trace annotation at
  0.5 s, and checks the server payload.
