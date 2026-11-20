# Store listing and submission — RedLine Capture

Status: **published** in the Chrome Web Store as
[RedLine Capture](https://chromewebstore.google.com/detail/redline-capture/jpoejaidggepljglhakmcknjdijmfjoo).
The public listing serves the last version that passed review; the release workflow submits each
newer `extension/manifest.json` automatically (see "Automated updates"). Edge Add-ons is not
submitted yet.

## Store facts

| Store | Fee | Account | Review |
|---|---|---|---|
| Chrome Web Store | **$5 one-time** developer registration | Google account | usually days; broad permissions can take longer |
| Microsoft Edge Add-ons | **free** | personal Microsoft account (MSA; not work/school) | up to 7 business days |

One ZIP works in both stores. Chrome creates the extension ID on the first manual upload; Edge
creates a Product ID on its first submission. Store APIs cannot create the first listing.

## Automated updates

Stable releases publish the extension through the Chrome Web Store API v2 with a service account
(no third-party action). The `extension-store` job in `.github/workflows/release.yml` runs after
the release assets exist and invokes:

```bash
node extension/scripts/cws.mjs release
```

The script compares `extension/manifest.json` with the store's `fetchStatus`:

- **published version equal** → nothing to do (exit 0);
- **same version pending review** → nothing to do, so a re-run cannot upload it twice;
- **rejected/cancelled submission** → uploads and submits again;
- **manifest newer** → uploads `dist/redline-extension-<version>.zip`, waits for the upload and
  calls `publish`, which submits for review and publishes on approval;
- **store version newer than the manifest** → fails instead of downgrading.

The store still reviews updates; the job reports **submitted**, not live. Installed extensions
update after Google approves the version. `node extension/scripts/cws.mjs status` prints the
published and submitted versions, and the release job appends the same output to its run summary.

### One-time credentials

1. Google Cloud Console → create/select a project → enable **Chrome Web Store API**.
2. IAM & Admin → Service accounts → create `redline-ci` (no project roles needed) → Keys → add
   key → JSON. Treat the downloaded file as a password.
3. Chrome Web Store Developer Dashboard → **Account** → add the service account email
   (`redline-ci@<project>.iam.gserviceaccount.com`). Only one service account can be linked.
4. Copy the **Publisher ID** from Dashboard → Publisher → Settings.
5. Add both to GitHub without printing the key (the Google account needs 2-step verification):

   ```bash
   gh secret set CWS_SERVICE_ACCOUNT_JSON < /path/to/service-account.json
   gh secret set CWS_PUBLISHER_ID --body "<publisher id>"
   ```

   For a local check, pass the file instead of the JSON value:

   ```bash
   CWS_PUBLISHER_ID=<id> CWS_SERVICE_ACCOUNT_FILE=/path/to/service-account.json \
     node extension/scripts/cws.mjs status
   ```

The release job skips cleanly when the secrets are absent, so forks and pre-credential releases
still build.

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

**Privacy policy:** <https://github.com/erikibolya/redline/blob/main/docs/PRIVACY-EXTENSION.md>

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
- [x] Privacy policy on `main` (`docs/PRIVACY-EXTENSION.md`).
- [x] Chrome Web Store developer account; item `jpoejaidggepljglhakmcknjdijmfjoo` published.
- [ ] Microsoft Edge Partner Center account (the same ZIP works there).
- [x] Store ZIP from `python3 extension/scripts/package.py` (`dist/redline-extension-0.7.3.zip`; rebuild after changes).
- [ ] Service account linked and `CWS_PUBLISHER_ID` + `CWS_SERVICE_ACCOUNT_JSON` secrets set (see "Automated updates").

## Submission steps

1. Chrome Web Store developer account ($5) exists; create the Edge Partner Center account (free) when publishing there.
2. Run `python3 extension/scripts/package.py`; upload `dist/redline-extension-<version>.zip`.
3. Fill the listing, privacy form and permission justifications from this file.
4. Upload the three screenshots from `extension/store/screenshots/`; submit for review.
5. After approval, add the store links to `extension/README.md` and the GitHub release notes.
6. Later versions need no manual upload: bump `extension/manifest.json`, merge to `main`, and the
   release tag submits the new version (see "Automated updates"). Listing text, screenshots and
   privacy answers are still dashboard-only edits.
