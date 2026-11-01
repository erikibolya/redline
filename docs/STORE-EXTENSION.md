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
> Capture a page or tab flow, annotate it, and send it to your own RedLine review server.

**Detailed description:**

> RedLine Capture turns a browser page into a review your AI agent can act on.
>
> • **Screenshot review** — capture the visible tab, draw rectangles, arrows, freehand marks and
>   text notes in the side panel, then submit.
> • **Flow review** — record a tab while collecting console output, network metadata and the
>   actions you take. Annotate the video at the exact moment, filter the trace, and submit the
>   clip, annotated frames and trace together.
> • **Your server** — everything goes to the RedLine server you configure. No account, no cloud,
>   no analytics, no third-party endpoints.
> • **Redaction** — bearer tokens and common token/key/secret/password query values are removed;
>   request/response bodies and headers are not captured in this version.
>
> RedLine never runs git, deploys or merges. It records the human's annotations and decision; the
> agent integration performs its own workflow.
>
> You need a running RedLine server and its token. Install, open the extension options, paste the
> server URL and token, then capture.
>
> Permissions: `debugger` is used only while you explicitly record, to read console/network
> events for the review; it is detached on stop. `tabCapture` records only the tab you start. The
> extension asks for access only to the RedLine server origin you configure.

**Category:** Developer Tools

**Homepage / support:** <https://github.com/erikibolya/redline>

**Privacy policy:** <https://github.com/erikibolya/redline/blob/main/docs/PRIVACY-EXTENSION.md>
(use the final public URL once the repository is public/pages is enabled).

## Permission justifications

Copy these into the Chrome Web Store / Partner Center fields:

- `activeTab` — the user explicitly invokes capture/record on the current tab; the extension does
  not read other tabs.
- `tabs` — read the active tab title/URL for the review and open the generated review.
- `storage` — save the RedLine server URL/token, project selection and unsent annotation drafts.
- `sidePanel` — the annotation editor.
- `scripting` — inject the small action listener into the recorded tab and its frames.
- `tabCapture` — record the active tab after Record is pressed.
- `offscreen` — keep MediaRecorder alive while the popup is closed.
- `debugger` — read console messages/exceptions and network metadata only during an explicit
  recording; detach immediately on stop. No request/response bodies or headers are read.
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
- [ ] 3–5 screenshots at **1280×800** or **640×400** (Chrome) / **1280×800** or **640×480**
      (Edge): options, screenshot annotation, video + trace panel, trace filter/errors.
- [ ] Small promo tile 440×280 (optional).
- [ ] Final privacy-policy URL (public HTML; GitHub blob is acceptable but a Pages page is nicer).
- [ ] Store developer accounts and identity verification.
- [ ] Version/package zip from `bash extension/scripts/package.sh`.

## Submission steps

1. Create the Chrome Web Store developer account ($5) and Edge Partner Center account (free).
2. Run `bash extension/scripts/package.sh`; upload `dist/redline-extension-<version>.zip`.
3. Fill the listing, privacy form and permission justifications from this file.
4. Upload screenshots; submit for review.
5. After approval, add the store links to `extension/README.md` and the GitHub release notes.
