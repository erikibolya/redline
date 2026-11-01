# RedLine Capture (browser extension)

A Chrome/Edge MV3 extension that captures the current tab, opens it in a side-panel annotator, and
submits the annotated screenshot to a RedLine server.

## Development

1. Open `chrome://extensions` (or `edge://extensions`), enable **Developer mode**, choose
   **Load unpacked**, and select this `extension/` directory.
2. Open the extension options (gear in the popup), paste your RedLine server URL and token, then
   **Save & request access**. Chrome asks for access to that server origin.
3. Open an `http(s)` page, click the RedLine Capture toolbar icon, and choose
   **Screenshot & annotate**.
4. Draw in the side panel, add a note, and **Submit to RedLine**. The review goes into the normal
   RedLine queue; the agent reads the annotations and the submitted note.
5. To record a flow, use **Record flow** in the popup. Chrome shows a debugging bar; click through
   the app, use **Mark** for a moment, then **Stop & save**. The clip and trace upload together.
   The side panel opens the recorded flow with a video timeline, trace filters and timecoded
   annotations.

## What is implemented

- Server/token/project/branch configuration with an origin-scoped host permission.
- Visible-tab screenshot capture and image-session creation.
- Side-panel canvas editor: select/move, rectangle, arrow, pen, text note, colour/width, undo/redo,
  delete, clear, per-session draft, annotated PNG composition and submit.
- **Open in RedLine** for the full editor; a submitted review is read-only in the panel.

## Recording

- `tabCapture` + MediaRecorder in an offscreen document; the video bytes never cross the extension
  message channel.
- `chrome.debugger` attaches only after Record and collects console/exceptions, network metadata
  (no headers/bodies) and navigation. A content script records click/change/submit actions.
- URLs and console text are redacted (Bearer tokens, `token/key/secret/password/code/session`
  query values). The trace is capped at 5,000 events and 10 MB; the video is capped by RedLine.
- Stop builds the poster and uploads video + trace; the review enters the normal queue.
- Video annotator: play/pause, frame step, timeline, annotation and trace markers, Ghosts,
  timecoded point annotations, annotated frames/contact sheet and submit. Maximum 20 unique
  annotated times per round.

The store package is built by `python3 extension/scripts/package.py` (output:
`dist/redline-extension-<version>.zip`). See [`../docs/STORE-EXTENSION.md`](../docs/STORE-EXTENSION.md)
for the listing/permission copy and [`../docs/PRIVACY-EXTENSION.md`](../docs/PRIVACY-EXTENSION.md)
for the privacy policy. Extension e2e is the remaining engineering item.

## Tests

```bash
node --test extension/tests/annotations.test.mjs extension/tests/api.test.mjs extension/tests/config.test.mjs extension/tests/trace.test.mjs
```

The same command runs in CI on Linux and Windows. There is no build step; reload the unpacked
extension after changing a file.

Manual smoke test (until the extension e2e lands): configure a local/test RedLine, capture a page,
draw a rectangle, arrow, pen and text note, move one, undo/redo, reload the side panel to prove
the draft survives, submit, then open the review in RedLine and verify the numbered annotations and
the agent note. Also verify that a submitted review opens read-only.

Recording smoke test: start Record on a local page, trigger a click, a console error and a failed
fetch, press Mark, stop, then verify the video plays in RedLine, `trace.json` has action/console/
network events and `review.md`/`get_feedback` carry the trace summary. Do not use a page with real
credentials; the trace redacts URLs and tokens but the video is a real screen recording.

## Permissions

- `activeTab` — capture only after the human clicks the action.
- `tabs` — read the current tab's title/URL and open the RedLine review.
- `storage` — config, active capture and unsent drafts.
- `sidePanel` — the annotator.
- `scripting` / `tabCapture` / `offscreen` / `debugger` — only after an explicit Record click;
  the debugger is detached on stop.
- Optional host permission — requested for the configured RedLine server origin only.
