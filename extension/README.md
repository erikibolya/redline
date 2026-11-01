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

## What is implemented

- Server/token/project/branch configuration with an origin-scoped host permission.
- Visible-tab screenshot capture and image-session creation.
- Side-panel canvas editor: select/move, rectangle, arrow, pen, text note, colour/width, undo/redo,
  delete, clear, per-session draft, annotated PNG composition and submit.
- **Open in RedLine** for the full editor; a submitted review is read-only in the panel.

Recording, console/network traces, the trace UI and store packaging are later phases. See
[`../docs/EXTENSION.md`](../docs/EXTENSION.md).

## Tests

```bash
node --test extension/tests/annotations.test.mjs extension/tests/config.test.mjs
```

The same command runs in CI on Linux and Windows. There is no build step; reload the unpacked
extension after changing a file.

Manual smoke test (until the extension e2e lands): configure a local/test RedLine, capture a page,
draw a rectangle, arrow, pen and text note, move one, undo/redo, reload the side panel to prove
the draft survives, submit, then open the review in RedLine and verify the numbered annotations and
the agent note. Also verify that a submitted review opens read-only.

## Permissions

- `activeTab` — capture only after the human clicks the action.
- `tabs` — read the current tab's title/URL and open the RedLine review.
- `storage` — config, active capture and unsent drafts.
- `sidePanel` — the annotator.
- Optional host permission — requested for the configured RedLine server origin only.
