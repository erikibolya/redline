# RedLine Capture (browser extension)

A Chrome/Edge MV3 extension that captures the current tab, freezes it under a full-screen
annotation overlay, and keeps the tools in the side panel. It either submits the review to a
RedLine server or exports it as a folder/ZIP for an AI assistant.

## Development

1. Open `chrome://extensions` (or `edge://extensions`), enable **Developer mode**, choose
   **Load unpacked**, and select this `extension/` directory.
2. Click the RedLine Capture toolbar icon. The **side panel** opens; use the gear in its header
   to open Options, paste your RedLine server URL and token, then **Save & request access**.
   Chrome asks for access to that server origin.
3. Open an `http(s)` page, click the RedLine icon, and press **Screenshot current tab**. The
   first capture asks for access to that page (Chrome needs the click); later captures do not.
4. The page freezes under the overlay (without its scrollbar). Draw directly on the page; the side
   panel holds the tools, annotation list and note. Press **Submit to RedLine** (or **Send to
   developer** as a tester). The overlay closes and the page returns to live. Use **Discard** to
   cancel; there is no close or pause button in the overlay. Closing the side panel keeps the
   overlay, and reopening the panel reconnects to the same round.
   If the panel was opened on another tab, press **Allow capture on this site** once; the
   extension asks for access to that page and retries. If the screenshot succeeds but the overlay
   cannot be injected, press **Allow access to this page** in the workspace footer.
5. Video flow moves into the same panel in the next phase. Until then, use the RedLine web editor
   for video reviews; the side-panel video editor remains for captures created by the recording
   pipeline.

## Offline export (no server)

1. Do not configure a server (or use a fresh profile) and press **Screenshot current tab**. The
   page freezes under the same in-page overlay; no token or server is needed. The workspace shows
   the orange local-save warning.
2. Draw on the page, then click **Export review** in the side panel. The folder/ZIP contains the
   composed PNG and annotations. Local video still uses the side-panel editor.
3. Choose a folder (File System Access API) or let the ZIP fallback download
   `redline-review-<id>.zip`.

The package contains `review.md`, `annotations.json`, `AI-PROMPT.md`, `annotated.png` (image) or
`video.webm` + `poster.png` + `frames/` + `contact-sheet.png` (video), and `trace.json` for a
recording. Attach it to an AI assistant; the prompt explains what to read.

## What is implemented

- Server/token/project/branch configuration with an origin-scoped host permission.
- Visible-tab screenshot capture and image-session creation.
- **In-page screenshot annotation:** full-screen frozen overlay, direct drawing, side-panel tools,
  annotation list, note, pause/resume draft, submit/discard.
- Side-panel canvas editor for video and offline captures: select/move, rectangle, arrow, pen,
  text note, colour/width, undo/redo, delete, clear, per-session draft, annotated PNG composition
  and submit.
- **Open in RedLine** for the full editor; a submitted review is read-only in the panel.

## Recording

- `tabCapture` + MediaRecorder in an offscreen document; the video bytes never cross the extension
  message channel.
- `chrome.debugger` attaches after Record (console/exceptions, network metadata) and briefly for a screenshot when `activeTab` is unavailable;
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
for the privacy policy.

The extension e2e (`extension/scripts/e2e.mjs`) runs in CI against a temporary host-permitted
copy; the server paths need a running RedLine server and `REDLINE_BASE`/`REDLINE_TOKEN`, the
offline paths need only Chromium with the extension loaded and `CHROME_URL`.

## Tests

```bash
node --test extension/tests/annotations.test.mjs extension/tests/api.test.mjs extension/tests/config.test.mjs extension/tests/package.test.mjs extension/tests/trace.test.mjs
```

The same command runs in CI on Linux and Windows. There is no build step; reload the unpacked
extension after changing a file.

Manual smoke test: configure a local/test RedLine, click the toolbar icon, press
**Screenshot current tab**, draw a rectangle, arrow, pen and text note on the page, move one,
undo/redo, pause and resume to prove the draft survives, submit, then open the review in RedLine
and verify the numbered annotations and the agent note. Also verify that a submitted review opens
read-only. For offline mode, remove the server config and press **Screenshot current tab**,
annotate on the page, confirm the orange warning, export to a folder and verify the files; then
reject the folder picker and verify the ZIP fallback.

Recording smoke test: start Record on a local page, trigger a click, a console error and a failed
fetch, press Mark, stop, then verify the video plays in RedLine, `trace.json` has action/console/
network events and `review.md`/`get_feedback` carry the trace summary. Do not use a page with real
credentials; the trace redacts URLs and tokens but the video is a real screen recording.

## Permissions

- `activeTab` — capture only after the human clicks the action.
- `tabs` — read the current tab's title/URL and open the RedLine review.
- `storage` — config, active capture and unsent drafts.
- `sidePanel` — the control deck.
- `scripting` — injects the full-screen overlay after **Screenshot current tab**.
- `tabCapture` / `offscreen` / `debugger` — recording uses them after an explicit Record click; a
  screenshot fallback attaches the debugger briefly when `activeTab` is unavailable and detaches
  immediately (video recording UI is being moved into the panel).
- Optional host permission — requested for the configured RedLine server origin only.
