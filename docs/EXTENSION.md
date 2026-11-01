# Browser capture extension — design and delivery plan

Status: **draft for approval** (2026-11-01). No code is written yet.

A Chromium extension that connects to a RedLine server, captures a screenshot or records a flow
from the current tab, collects the console and network evidence, and hands the result to RedLine
as a normal review. The human annotates in the existing RedLine editor; the agent reads the
annotations plus a structured trace.

## 1. Goals

- **One click from the page to a RedLine review.** The extension knows the current tab, the
  RedLine server and the project; the human does not paste URLs or files.
- **Screenshot review.** Capture the visible tab (and later full page), create an image session,
  open the RedLine editor for annotation.
- **Flow review.** Record the tab as WebM while collecting:
  - console messages, uncaught exceptions and browser log entries;
  - network requests and responses (method, URL, status, timing, resource type, failure);
  - user actions (click, input/change, submit, navigation) with stable-ish selectors;
  - manual markers the human drops while recording.
- **Annotate the recording in RedLine.** The existing video timeline gets a trace lane and a
  filterable event panel; clicking an event seeks the video to that moment.
- **Agent-ready evidence.** `review.md` gets a compact trace summary; `get_feedback` exposes the
  trace metadata and URL; the raw `trace.json` is available as an MCP resource.
- **Local and private.** The token, video and trace stay on the user's RedLine server. No third
  party sees the capture; headers/cookies/request bodies are redacted or omitted by default.

## 2. Non-goals for v1

- Whole-desktop or multi-tab recording (one tab per recording; screen capture is a later mode).
- Firefox/Safari support (Chromium MV3 first; a Firefox port is a separate plan).
- Editing/trimming the video in the extension.
- A second annotation editor inside the extension. Annotation stays in RedLine.
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

- **Screenshot & annotate** — visible-tab capture; creates an image review and opens its RedLine
  URL in a new tab.
- **Record flow** — asks for the capture options (console, network, actions, bodies), shows a
  countdown, then records. While recording the popup shows elapsed time and a **Mark** button.
- **Stop & review** — stops recording, shows the draft form (title, note, project, branch,
  capture counts, redaction summary), then **Send to RedLine**.
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
[Send to RedLine] [Discard]
```

The upload uses `POST /api/sessions/video` with `video`, `poster`, `trace`, the form fields and
the media metadata. On success the extension opens `review_url`; the human draws timecoded
annotations and submits the review. Creating the review does **not** wake the agent; only Submit
does, exactly like today.

## 4. Architecture

```
┌────────────────────────────── Chrome / Edge ──────────────────────────────┐
│ popup / options (plain HTML+JS)                                            │
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
                    POST /api/sessions/video  (video + trace)
                    GET  /sessions/<id>/trace
                    GET  /api/sessions/<id>   (meta.trace)
                                    │
                                    ▼
                    RedLine editor: video timeline + trace panel
                    Agent: review.md + get_feedback + MCP resource
```

The extension is deliberately a **capture client**, not a second review application. RedLine
owns sessions, annotation, status and the agent queue.

## 5. Capture pipeline

### 5.1 Screenshot

1. `chrome.tabs.captureVisibleTab` for the visible viewport.
2. Optional full page (`Page.captureScreenshot` with `captureBeyondViewport`) behind a
   `debugger` attach; a full-page screenshot can be much larger, so cap dimensions/bytes.
3. `POST /api/sessions` with `image_base64`, title, URL, note, project and branch.
4. Open the returned `review_url`; annotation and submission stay in RedLine.

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
   read the elapsed duration, and open the draft.
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
  schemes `http(s)`, no unknown top-level event fields that could smuggle binary data.
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

## 8. RedLine editor UI

- **Timeline trace lane**: a thin lane below annotation markers; console errors red, failed
  requests amber, actions blue, manual markers purple. Hover shows a tooltip; click seeks.
- **Trace toggle** in the video bar: `Off / Errors / All` (default `Errors`). It is remembered per
  browser like Ghosts.
- **Inspector Trace section**: tabs/chips `All`, `Console`, `Network`, `Actions`, `Markers`;
  rows show timecode, kind and a compact label. Clicking a row seeks; expanding shows details
  (status/duration/url, console text, selector). Empty state explains that the clip has no trace.
- **History**: a historical round loads its archived trace when present; the same lane/panel is
  read-only.
- **Rail/gallery**: a small `flow` chip with duration/error count; not a new card layout.
- **Image sessions**: no timeline; if a trace exists later, a simple panel without seeking.

The UI must remain dependency-free (`app.js` + `app.css`), served from the binary. Trace markers
must not compete with annotation markers: the lane is separate and can be toggled off.

## 9. Extension structure

```
extension/
  manifest.json            MV3, Chrome/Edge first
  src/
    background.js          service worker: state, debugger, upload orchestration
    offscreen.html/.js     tabCapture + MediaRecorder + poster
    content.js             action events (all frames), overlay, Mark
    popup.html/.js         connect status, screenshot, record, draft
    options.html/.js       server/token/project/redaction
    lib/
      api.js               auth fetch, multipart, project list
      trace.js             normalize, redact, cap, summary
      selectors.js         stable selector + label
      format.js
  icons/                   RedLine icon sizes
  tests/
    trace.test.mjs
    redact.test.mjs
    selectors.test.mjs
```

No build step: ES modules in the service worker, plain HTML/JS pages. The extension is packaged as
a zip in the GitHub release (`redline-extension-<version>.zip`) and loaded unpacked for
development; a store listing is a separate decision.

Manifest permissions (proposed): `activeTab`, `scripting`, `tabs`, `storage`, `debugger`,
`tabCapture`, `offscreen`, `alarms`; host permissions for the configured RedLine server plus
optional `activeTab` grants for the recorded page. `debugger` shows a browser warning; the
extension only attaches after an explicit Record click and explains why.

## 10. Delivery phases

| Phase | Work | Exit |
|---|---|---|
| 0. Spike | MV3 skeleton, extension-origin auth, tabCapture + MediaRecorder in Chromium, debugger console/network events | A throwaway recording uploads to a local RedLine and plays |
| 1. Server trace | `TraceRef`/`Meta.trace`, storage, multipart `trace`, routes, validation, `review.md` summary, MCP feedback/resource, auth/CORS | Rust tests green; a curl upload creates a trace session |
| 2. Screenshot | options/popup, token/project config, visible-tab capture, create image session, open editor | Screenshot → annotate → submit works from the extension |
| 3. Recording | offscreen recorder, debugger collectors, content actions, redaction, draft/upload, poster/duration | A real flow records, uploads and plays with trace |
| 4. UI | trace lane, toggle, inspector panel, history trace, rail chip | E2E asserts markers, filters, seek and history |
| 5. Hardening | caps, tab-close/detach handling, error states, extension e2e, docs, release zip | Release checklist and CI green |

Rough effort: 2–3 days for the spike, 3 for server trace, 2 for screenshot, 5 for recording,
4 for UI/e2e, 2–3 for hardening. About three working weeks, assuming one browser and no store
review.

## 11. Testing

- **Rust**: trace validation and caps; multipart trace upload; storage/history; `review.md`
  summary; `get_feedback` and resource; extension-origin auth/preflight; web cross-site still
  denied; existing video tests remain green.
- **Extension unit (`node --test`)**: trace normalization, redaction (headers/query/body),
  selector generation, cap/truncation, duration formatting.
- **Browser e2e**: seed a session with a small trace; assert the trace lane, toggle, inspector
  rows/filters, click-to-seek, history trace, and that `get_feedback` includes `trace`.
- **Extension e2e**: load the unpacked extension in headful Chromium (Xvfb in the e2e image or a
  separate job), open a local page with a console error and a failing fetch, record 2–3 s, upload
  to the test server, assert the session has video + trace + expected events. If browser
  automation of `tabCapture` proves unreliable, keep the unit tests and a documented manual
  checklist as the gate, and add the automation as a follow-up — do not claim coverage that does
  not run.
- **Manual**: load unpacked in Chrome and Edge; capture a real app flow; verify console/network
  redaction; verify the agent sees the trace.

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

## 13. Decisions needed before coding

1. **Browser target**: Chrome/Edge (Chromium) first, or Firefox in the first release?
2. **Annotation**: reuse the RedLine editor tab (recommended), or build an annotation overlay in
   the extension side panel?
3. **Capture scope**: the active tab only (recommended), or whole screen/window (needed when the
   flow opens new tabs/popups)?
4. **Trace depth**: console + network metadata + actions (recommended), or also request/response
   bodies and headers with explicit redaction?
5. **Recording limit**: keep the existing 5 min / 250 MB (recommended for v1), or extend to
   15 min / 500 MB (needs a server streaming/upload change)?
6. **Distribution**: GitHub release zip + load unpacked (recommended for the first version), or a
   Chrome Web Store / Edge Add-ons listing?
7. **Project/branch defaults**: select a project in the popup and let the server derive the
   branch, or require the human to type a branch for every flow review?
