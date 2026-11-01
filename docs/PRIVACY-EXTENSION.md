# RedLine Capture — privacy policy

Effective: 2026-11-01

RedLine Capture is a Chrome/Edge extension that captures a page screenshot or a tab recording and
sends it to **the RedLine server the user configures**. It has no analytics, no advertising, no
third-party endpoints and no remote code.

## What the extension collects

Only after the user explicitly presses **Screenshot & annotate** or **Record flow**:

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

Captures and annotations are sent only to the RedLine server URL saved in the extension options.
They are stored in that server's session folder. The extension does not send data anywhere else.

## Local storage

- `chrome.storage.local`: the RedLine server URL, token, selected project and branch. The token is
  not synced to other browsers.
- `chrome.storage.session`: the active capture, unsent annotation draft and recording state. This
  is cleared when the browser session ends and is removed on submit/discard.
- The video, screenshot, trace and annotations live on the RedLine server until the user deletes
  the review there.

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
| `debugger` | Read console/network events while the user is recording; detached on stop. |
| Optional host permission | Access only the RedLine server origin the user configures. |

## Sharing and retention

The extension does not share data with the extension author, analytics providers or advertisers.
The RedLine server owner controls storage and retention. Delete a review in RedLine to remove its
screenshot/video, trace and annotations; remove the extension (or clear its storage) to remove the
local configuration and drafts.

## Contact

Issues and privacy questions: <https://github.com/erikibolya/redline/issues>.

This policy applies to the extension package published from this repository; the RedLine server
software itself may be self-hosted by its operator.
