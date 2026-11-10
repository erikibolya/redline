# Store listing and submission — RedLine Capture

Status: package and listing prepared; **not published** (owner has no store accounts yet).

## Store facts

| Store | Fee | Account | Review |
|---|---|---|---|
| Chrome Web Store | **$5 one-time** developer registration | Google account | usually days; broad permissions can take longer |
| Microsoft Edge Add-ons | **free** | personal Microsoft account (MSA; not work/school) | up to 7 business days |

One ZIP works in both stores. Chrome creates the extension ID on the first manual upload; Edge
creates a Product ID on its first submission. Store APIs cannot create the first listing.

## Listing copy

**Name:** RedLine Capture

**Short description (132 chars max):**
> Capture a page, annotate it on the frozen tab, and send it to RedLine or export it offline.

**Detailed description:**

> RedLine Capture turns the page you are looking at into a review your AI agent can act on.
>
> • **Screenshot review** — capture the visible tab, freeze it under an overlay, and draw directly
>   on the page. The side panel keeps the RedLine tools: select/move, rectangle, arrow, pen,
>   ellipse, colour and width, undo/redo, and delete (including a trash button on every annotation
>   row).
> • **Destination on the capture** — choose the RedLine project and branch while you annotate. The
>   extension remembers the last project for each site, so the next capture is filed there.
> • **Server mode** — submit to your own RedLine server as an owner/developer, or send a task to a
>   developer from a tester account. The token stays in this browser profile and is sent only to
>   the server you configure.
> • **Offline mode** — no server, account or token needed. Capture and annotate anyway, then press
>   "Export review" to save a folder or ZIP with review.md, annotations.json and the annotated PNG.
>   Use it to try the extension, for store review, or to hand a review to any AI assistant.
> • **Dark and light theme**, matching the RedLine desktop editor.
> • **No analytics, no tracking, no third-party endpoints.** Offline exports never leave the
>   browser.
>
> To use server mode, open the gear in the side panel, paste your RedLine server URL and token,
> and press Connect. To test offline, leave the connection empty: press "Screenshot current tab",
> annotate, then "Export review".

**Category:** Developer Tools

**Homepage / support:** <https://github.com/erikibolya/redline>

**Privacy policy:** <https://github.com/erikibolya/redline/blob/dev/docs/PRIVACY-EXTENSION.md>
(the `dev` blob is live now; switch the listing to the `main` blob after the next release).

## Testing without a server

The 0.7.3 package supports offline export, so reviewers do not need a RedLine server, account or
token. Use this text in Chrome **Test instructions** and Edge **Notes for certification**:

```text
No RedLine server, account or token is required to test the extension.

1. Open any http(s) page and click the RedLine Capture toolbar icon.
2. In the side panel, press "Screenshot current tab". If Chrome asks for access to the page, allow it.
3. Draw a rectangle and an arrow on the frozen page; the controls are in the side panel.
4. Press "Export review", choose a folder (or let the ZIP fallback download).
5. Verify review.md, annotations.json and annotated.png in the export.

Offline mode is the default until a RedLine server URL and token are saved; an offline export
makes no request to the extension author or any third party. Server mode is optional: open the
gear in the side panel, enter a RedLine server URL/token, press Connect, then "Submit to RedLine".
```

## Permission justifications

Copy these into the Chrome Web Store / Partner Center fields:

- `activeTab` — the user explicitly invokes capture/record on the current tab; the extension does
  not read other tabs.
- `tabs` — read the active tab title/URL for the review and open the generated review.
- `storage` — save the RedLine server URL/token, the last chosen project and unsent annotation drafts.
- `sidePanel` — the annotation editor.
- `scripting` — inject the small action listener into the recorded tab and its frames.
- `tabCapture` — record the active tab after Record is pressed.
- `offscreen` — keep MediaRecorder alive while the popup is closed.
- `debugger` — read console messages/exceptions and network metadata during an explicit
  recording, and take a screenshot when `activeTab` is unavailable (briefly attached, then
  detached). Detach immediately on stop. No request/response bodies or headers are read.
- Optional host permission — access only the RedLine server origin entered by the user.

## Privacy practices form

- Single purpose: **capture a page or tab flow, annotate it, and submit it to the user's own
  RedLine server**.
- Remote code: **no**.
- Data handled: website content (screenshot/video/URL/title), web history (the active page URL),
  user activity (recorded clicks/input), and authentication information (the RedLine token used
  to call the user's server). Data is sent only to the configured RedLine server; no sale, no
  advertising, no analytics.
- Data retention: on the user's RedLine server; the user deletes reviews there. Local config and
  drafts can be cleared by removing the extension.

## Assets checklist

- [x] 128×128 extension icon (`extension/icons/128.png`; also 16/32/48).
- [x] 3 screenshots at **1280×800** in `extension/store/screenshots/`:
  - `01-capture.png` — the intro/capture panel next to the page it will capture;
  - `02-screenshot-review.png` — live review with rectangle, arrow and ellipse, project/branch and note;
  - `03-offline-export.png` — offline review with the not-connected warning and **Export review**.
  - `03-video-trace.png` is a legacy asset and is not part of the current set.
- [ ] Small promo tile 440×280 (optional).
- [ ] Final privacy-policy URL (use the `dev` blob now; switch to `main` after the next release).
- [x] Chrome Web Store and Microsoft Edge developer accounts; submit 0.7.3 with the offline export.
- [x] Store ZIP from `python3 extension/scripts/package.py` (`dist/redline-extension-0.7.3.zip`; rebuild after changes).

## Submission steps

1. Chrome Web Store developer account ($5) exists; create the Edge Partner Center account (free) when publishing there.
2. Run `python3 extension/scripts/package.py`; upload `dist/redline-extension-<version>.zip`.
3. Fill the listing, privacy form and permission justifications from this file.
4. Upload the three screenshots from `extension/store/screenshots/`; submit for review.
5. After approval, add the store links to `extension/README.md` and the GitHub release notes.
