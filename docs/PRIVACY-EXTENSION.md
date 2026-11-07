# RedLine Capture — privacy policy

Effective: 2026-11-03

RedLine Capture is a Chrome/Edge extension that captures a page screenshot or a tab recording and
either exports it locally or sends it to **the RedLine server the user configures**. It has no
analytics, no advertising, no third-party endpoints and no remote code.

## What the extension collects

Only after the user explicitly presses **Screenshot & annotate**, **Screenshot offline**,
**Record flow** or **Record offline**:

- the visible tab's screenshot, or the tab's video while recording;
- the page URL and title;
- while recording: console messages and exceptions, network request metadata (method, URL,
  status, timing and resource type; **not** request/response bodies or headers by default), and
  user actions such as click/change/submit;
- annotations and notes the user adds in the side panel.

The extension redacts `Authorization`/`Cookie`/`Set-Cookie`/`X-Api-Key` values and common
token/key/secret/password query parameters before upload. Request and response bodies are not
captured in this version. A recording can still contain sensitive information visible on screen;
the user sees the draft and can discard it before upload.

## Where the data goes

- **Server mode:** captures and annotations are sent only to the RedLine server URL saved in the
  extension options and stored in that server's session folder.
- **Offline mode:** captures and annotations stay in IndexedDB in the browser profile. **Export
  review** writes a folder or ZIP the user chooses; no data is sent to the extension author or a
  third party.

## Local storage

- `chrome.storage.local`: the RedLine server URL, token, selected project and branch. The token is
  not synced to other browsers.
- `chrome.storage.session`: the active capture pointer, unsent annotation draft and recording
  state. This is cleared when the browser session ends and is removed on submit/discard.
- IndexedDB (`redline-local`): offline screenshots, videos, posters, traces and annotations. They
  stay in the browser profile until the user discards the review; exported files stay in the
  folder/ZIP until the user deletes them.
- In server mode, the video, screenshot, trace and annotations live on the RedLine server until the
  user deletes the review there.

## Permissions

| Permission | Why |
|---|---|
| `activeTab` | Capture/record only after the user invokes the extension on that tab. |
| `tabs` | Read the active tab's title/URL and open the RedLine review. |
| `storage` | Save the server configuration and unsent drafts. |
| `sidePanel` | The annotation panel. |
| `scripting` | Inject the action recorder into the recorded page's frames. |
| `tabCapture` | Record the active tab. |
| `offscreen` | Run MediaRecorder without keeping a visible page open. |
| `debugger` | Read console/network events while the user is recording; also take the screenshot when `activeTab` is unavailable (attached briefly and detached immediately); detached on stop. |
| Optional host permission | Access only the RedLine server origin the user configures. |

## Sharing and retention

The extension does not share data with the extension author, analytics providers or advertisers.
In server mode, the RedLine server owner controls storage and retention; delete a review in RedLine
to remove its screenshot/video, trace and annotations. Offline captures can be deleted with
**Discard** in the side panel; remove the extension (or clear its storage) to remove the local
configuration, drafts and offline captures. Exported folders/ZIPs are ordinary files on the user's
disk.

## Contact

Issues and privacy questions: <https://github.com/erikibolya/redline/issues>.

This policy applies to the extension package published from this repository; the RedLine server
software itself may be self-hosted by its operator.
