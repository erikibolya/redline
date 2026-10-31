# Changelog

## Unreleased

- **The dashboard opens with its content, not empty.** Clicking a filter that no longer contains
  the open review sends the human back to `/`; that full load painted the chip row, the rail and
  the gallery only after `/api/sessions` answered, so the page sat blank and then everything
  popped in and pushed "Start a review" ~200 px down (2026-10-31). The rail now paints the filter
  chips immediately (counts once the data is there), and the last list this tab rendered is kept
  in `sessionStorage` and painted at boot before the request lands; with no cache the gallery
  paints one placeholder card of the real card's shape, so the row its cards will take is already
  reserved. The first response replaces the cached list in place, and a failed request keeps what
  the tab already painted. The e2e holds `/api/sessions` on a reload of `/`, asserts chips + rail
  rows + gallery cards are already painted, then releases it and asserts the page does not move;
  it repeats the measurement with the cache cleared, against the placeholder.
- **A run is killed for being silent, not for being long.** The bridge used a 30-minute wall-clock
  cap and killed a run that was actively working/building, then labelled it a generic "agent
  failed" (2026-10-31). It now tracks output activity: `REDLINE_AGENT_IDLE_TIMEOUT` (default 15 m)
  kills a run with no output, while `REDLINE_AGENT_TIMEOUT` is only a 6 h hard backstop. A long
  build that keeps printing survives; a hung process is stopped promptly.
- **A timeout says it timed out.** `reply {state:"failed", code:"timeout_idle"}` stores
  `meta.agent_failure { code, message, at }`; the rail badge reads **agent timed out** (or
  **agent failed** for other codes) and the card/message keep the exact limit. `get_feedback`/
  `wait_for_feedback` expose the same reason. A new run/reply clears the stored failure.
- **A handoff can no longer read "handing off" after its agent is gone.** A stopped/crashed handoff
  released its claim but left `handoff.status=running` forever; `GET /api/sessions` now derives
  `handoff_stale` when a running handoff has no live claim, and the rail shows **handoff stale**
  with the retry action. The bridge also reports `failed` when a handoff stops while the feature is
  still confirmed. The handoff prompt now pushes a **draft PR first**, lets GitHub run the full
  suite, and only then marks it ready; local preflight uses `REDLINE_BIN` to mount a fresh server
  binary into the published browser image instead of rebuilding the ~768 MB image.
- **A review run does not build the release.** The bridge prompt used to permit build/deploy; an
  agent spent its whole run on a full image build. Review prompts now say to run focused checks
  only and not to build/publish the release/browser image or run the release pipeline; the same
  `review_guidance` is returned by MCP `get_feedback`/`wait_for_feedback` and in the tool
  descriptions, so non-bridge agents receive it too.
- **The agent's output is a popup, and the stage toast narrates a step instead of a command.** The
  bridge streams the agent's own terminal lines into the event log as `state: "log"`, and two
  places printed the newest event whatever it was: the stage toast read **Agent is working — $ cd
  … && $ sed -n …** and the inspector's output tail sat under the messages, taking the little
  height a short window had. The **output** button now opens a centred popup (monospace, scrolling,
  following the tail while the run is live, closed by the button, `Esc` or the backdrop, not
  reopened by a reload), and the toast plus the conversation's activity label use the last
  narrated step, one short line, with the full text on hover. The e2e asserts the toast never
  prints a log line, the popup's centre and that it is not inside the inspector, that a poll keeps
  feeding it, and every way to close it.

## 0.25.0 — 2026-10-31

- **A history round stays the round you opened.** A live conversation poll that was already in
  flight could finish after a history round started loading and replace its archived clip with the
  live one (the annotations stayed historical, so the picture and the marks disagreed). The poll
  now re-checks the session and `viewing`/round-load state after its awaits, and a round load is
  atomic: a later poll or another round click can no longer paint over it.
- **Errors no longer hide in Done.** The rail had five views (`All`, `Active`, `Open`,
  `Submitted`, `Done`), and `Active` meant "not resolved" — so a confirmed feature whose handoff
  was queued, running or failed sat in `Done`, together with the `handoff failed` /
  `worker-offline` badge that said something was wrong. There are now three views: `Active`
  (default) holds every review that still needs a human — anything not resolved, plus a confirmed
  feature whose handoff is not `done`; `Done` is only work that finished cleanly; `All` is the
  archive. When any handoff has failed the `Active` chip carries a red edge and a count in its
  title, and the poll now redraws the rail when a handoff changes state (it used to redraw only
  when the working set changed, so a failure stayed in the old list until a reload). Stored
  preferences for the removed views migrate (`open`/`submitted` → `active`, `resolved` → `done`).
  The e2e asserts the three chips, that a queued/failed handoff stays in `Active` and out of
  `Done`, and still walks confirm → handoff → retry → reopen → cancel.
- **A finished review says `done` in the rail, and every round says how long it ran.** A resolved
  session that was neither `working` nor decided showed no badge at all, so finished tasks looked
  like they were still in progress (2026-10-31). Resolved rows now carry a `done` badge and
  resolved rows' subtitles print `· took 4m 12s`; an open round with nothing drawn on it says
  `open` instead of staying blank. `GET /api/sessions/<id>/history` adds `started_at`,
  `finished_at` and `duration_secs` per round — submit → the agent's next picture, the next human
  submission, or `resolve` — and the History rows plus the conversation's run summary print the
  cost. The fields are additive and omitted while a round is still running.
- **The header carries the icon, not the wordmark — and the icon is as big as the text.** The shell
  printed a 22 px tile *and* the word `RedLine` beside the session title, so the app name read as a
  second heading and the tile overtopped the 13 px text next to it. The wordmark is gone on all four
  pages (editor, dashboard, setup, servers), and the mark is now 28 px — the height of the two-line
  title block it stands beside (13 px title + 11 px subtitle) — centred in the header row; screen
  readers still hear the name through `role="img" aria-label="RedLine"`. The first fix sized it to
  the title's *font size* (18 px), which still read as a chip floating next to the title. Covered by
  an e2e measurement of both the editor and the dashboard header (no text node, mark within 2 px of
  the title block, at least twice the font size).
- **A failed run no longer looks like a working one, and no longer keeps the round locked.** The
  bridge reports "I could not run this" through `reply {state:"failed"}` instead of a normal
  answer; the server sets `agent_state=failed` and hands the review back (`status=open`,
  annotations and history intact), the rail shows a red **agent failed** badge and the run card
  turns red. Before, a bridge that could not start OpenCode left the round `submitted` behind a
  *replied* label: the canvas was locked, the working indicator was gone and nothing said the
  agent was never coming back (reported as *"the UI is blocked… the working label is not present,
  is it a bug?"*).
- **A live claim outranks the last message, so the working label survives a mid-run reply.**
  `GET /api/sessions` now returns `agent_running`, derived from the bridge's claim while its lease
  is alive; the rail shows **working** for it even when `agent_state` already says `replied`, and
  drops the spinner when the claim is released or expires. `agent_state` alone only records who
  spoke last, so an answer during a long run used to erase the only sign of activity.
- **The agent's own output is one click away.** The conversation header has an **output** toggle
  that shows the lines the bridge streams from the OpenCode process (`progress {state:"log"}`,
  throttled to one line every 2 s, ANSI stripped, ≤ 400 chars, last line of a short run kept) in a
  monospace tail with the time of the last line. Off by default and remembered per browser, so a
  wall of tool output never buries the conversation. Requested as *"a button that would show
  realtime output from the agent, like in a regular opencode window"*.

## 0.24.0 — 2026-10-31

- **"Install latest dev build…" offers the newest dev build again.** Dev builds were versioned
  `0.23.0-dev.N` after stable `0.23.0` had shipped; semver correctly treats that as an *older*
  prerelease, so the updater refused it and the menu looked broken. The dev base is now `0.24.0`,
  and the dev workflow fails unless `DEV_BASE_VERSION` sorts above the latest stable release. The
  next dev build (`0.24.0-dev.<run>`) is offered to an installed `0.23.0` (and to
  `0.23.0-dev.68`).
- **Unsent video work survives leaving the review.** Drafts were an in-memory `Map`, so a rail
  switch kept them but a dashboard round-trip, a reload or the desktop webview navigating started
  a new document and threw away every timecoded shape. Drafts now live in `localStorage` per
  session, autosave on a 400 ms debounce and flush on `pagehide`/hidden; submit clears the draft,
  a new agent round discards a stale one, and at most 20 drafts / 30 days are kept. The e2e draws
  three shapes, leaves for the dashboard, returns and asserts the `at` values and groups, then
  checks that submit removed the stored draft.
- **Moving the playhead hides the annotations that belong to another moment.** A freshly drawn
  shape stayed selected, and `annotationOpacity` returned `1` for selected rows — so an old
  annotation stayed fully visible until another drawing stole the selection. Selection now only
  draws the outline; the playhead decides visibility. Ghosts are opt-in (the old 12 % ghost
  became the *Ghosts* toggle, remembered per browser), so the canvas shows exactly the current
  frame unless the human asks for context. Timeline markers still show every annotation.
- **Video annotations are grouped by timestamp.** The inspector splits a video's rows into
  chronological sections with a timecode/count header; clicking a header seeks there, the group
  under the playhead is marked, a range group stays active to its end, and read-only history
  rounds use the same shape. A second shape on the same moment stays in one group rather than
  making a duplicate header. Image sessions keep the flat list.
- **The dashboard gallery follows the rail's poll and empties when it hides.** The `/` gallery was
  painted only at boot and on a back-forward restore, so a review cancelled elsewhere (or restored
  from the back-forward cache before the refresh landed) could keep its card while the rail itself
  was already current. Worse, when the last pending review was cancelled, `renderGallery` set
  `hidden` but left the old card in the DOM. The rail's 8 s session poll now hands the same list to
  the gallery, and the empty state clears the shell. The e2e cancels a review through the API,
  calls that poll, and asserts no card remains and the gallery is empty.
- **The working scrim is dark enough to read, and the chrome over the stage follows the theme.**
  While a round is submitted the canvas carries a scrim, but at `.18` (dark) and `.10` (light) it
  was invisible on a light stage, so nothing said the canvas was locked. It is now `.28`/`.18`.
  The centred *"Agent is working — …"* pill and the read-only round bar hard-coded a near-black
  capsule; on a white screenshot that read as a hole in the picture. Both get a light-theme card
  (white fill, slate text). The same sweep covers the other surfaces that only had dark colours:
  the *Delete cancelled* chip, the row-menu hover, the new-review drop zone's drag state, form
  errors, and the amber banners on the Servers and Setup pages. The e2e reads the computed
  colours of the pill, the round bar and the scrim in both themes instead of trusting the class.

## 0.23.0 — 2026-10-31

- **A history glyph belongs to the round's title, not to the middle of the card.** A history row
  is two lines (title + timestamp), and `.ann-row` centres its glyph on the whole row, so the
  bullet and the round number floated between "Current"/"Round N" and the date — two annotations
  on the same column. The row now starts at the top and the glyph is lifted to the title's line
  box. The number inside its square was also ~2 px high: a centred em box has no descender to
  balance the digit, so it got a top pad. The e2e measures every glyph against its title's line
  and the digit's ink against the square.

- **History rows are cards, not a chopped stacked list.** They reuse `.ann-row`, whose divider
  squares the top corners of every row after the first, so each archived round looked cut off
  (the rows sit 4 px apart and each owns its whole border). The radius is restored per history
  row. The live round no longer tints its fill and border blue either: it is the same card as the
  archived rounds, with a green glyph as the only marker, and the accent outline is left to the
  round currently being viewed. The e2e measures the radius of every row and compares the live
  row's fill and border with a sibling's.

- **A feature is confirmed first, then optionally handed off — and the rail says what happened.**
  The review prompt let an agent follow its repository's shipping rules mid-review, so overnight
  runs opened and merged PRs before the human confirmed anything; by morning every review was
  already touched. A review run now pushes its branch and replies but must not open a PR, merge or
  release. The confirm dialog has an optional **handoff** checkbox (default on only when an online
  bridge advertises `jobs: ["review","handoff"]`); confirming without it records only the decision.
  With it, the server queues a generic `handoff` (`requested` → `running` → `done`/`failed`); the
  integration performs its own post-confirmation action and reports a `label`, `url` and `message`
  through MCP `handoff_status`. The reference git bridge opens/merges a PR and reports
  `label=merged` + the PR URL; a deploy integration can report `label=deployed` + an environment;
  an integration with no post-confirmation action simply confirms without a handoff. The rail
  shows `handoff queued`, `handing off`, the integration's label with its link, or a red
  `handoff failed` label with a row `↻` retry; a retry keeps the label/URL/message from the
  failed attempt. Old bridges that send no `jobs` are never handed a handoff job, and an old
  presence file is review-only.
  Rust tests cover the opt-in queue, the capability gate, the generic MCP report and the state
  reset; the browser e2e checks the review-only default, confirms with a handoff, reports
  done/failed through MCP, follows the link and retries. The e2e's cancel dashboard check and two
  selection/z-order checks now poll instead of assuming an instant.
- **Feature details in `docs/INTEGRATIONS.md`.** The MCP contract is redocumented as a generic
  decision (`feature_state`) plus an optional handoff (`requested`/`running`/`done`/`failed`,
  `label`/`url`/`message`) and the `jobs` capability — with git, deploy and no-handoff examples.
  RedLine still runs no git and holds no GitHub token; the git workflow lives in the reference
  `redline-agent` bridge.
- **Video media core.** The server can create a video session (MP4/WebM, magic-byte validation),
  store a poster and duration/dimensions, serve the clip with HTTP `Range` for seeking, and write
  timecoded annotations (`at`/`until`) into `annotations.json` and `review.md`. MCP gains
  `review_video` (server path or base64) and `get_feedback` returns `media` plus a tokenised
  `video_url`. Limits: 250 MB, five minutes when declared, 8 MB poster; HEIC/AVIF are rejected as
  videos. Rust tests cover the media model, range 206/416, poster and timecoded Markdown.
- **Video reviews in the editor.** The New review dialog accepts a dropped, picked or pasted
  video, shows a muted preview and uploads the clip with its first-frame poster
  (`POST /api/sessions/video`). The canvas plays the video with play/pause, frame stepping,
  speed, a full-duration overview and a zoomable timeline with a time ruler and annotation
  markers/range bands. Drawing pauses playback and stamps the annotation with `at`; the inspector
  adds an optional `until` range and shows `mm:ss.mmm` timecodes. Annotations from other moments
  render as ~12 % ghosts that fade in near their time, with a Ghosts toggle. Keyboard `K` play/
  pause, `J`/`L` ±1 s, `,`/`.` frame step. Gallery cards use the poster and a `▶ mm:ss` chip.
  Contact sheets, full-resolution frames, video rounds in history and `post_round` video are the
  next phase. Browser e2e covers the real upload, playback, timecoded draw, ghost opacity, range,
  zoom and persisted submit (182 checks).
- **Contact sheets, full frames and new video rounds.** A video submit now captures one
  full-resolution frame per unique annotated timestamp (20 / ~60 MB cap, full-resolution JPEG
  fallback), paints the moment's annotations over each tile of a labelled contact sheet in
  `annotated.png`, writes `frames/` and lists both in `review.md` and `get_feedback`. The editor's
  **New video** button posts the next clip: the previous round (annotations, sheet, frames and
  clip) is archived under `history/<stamp>/` and the review reopens on the new video. History
  plays archived video rounds read-only with their annotations, and MCP `post_round` accepts
  `video_base64`/`video_path`. The live `/media` URL is cache-busted so a replaced clip cannot be
  served from the previous round. Rust tests cover frame storage/limit/archive, new-round
  archiving and MCP video rounds; browser e2e covers the composed sheet, two timecoded frames,
  the new-video flow and history playback, plus the 2 GB session-size warning (207 checks).

## 0.22.0 — 2026-10-31

- **Confirmed and cancelled badges are tinted, not black pills.** The dark rail painted
  `confirmed` as `#10231a` and `cancelled` as `#2a1115`, which read as a black background with
  coloured text. They now use translucent green/red tints with matching borders in dark mode and
  solid pastel green/red in light mode; the text keeps its contrast. The e2e samples both badges
  in both themes and asserts the computed backgrounds, text and borders.
- **Back to sessions returns to the active server, not to the local one.** The Servers and Setup
  pages are served by the desktop app's local server; their "Back to sessions" link was always
  `href="/"`, so with a remote profile active it opened the local dashboard, which has no
  sessions and reported "No agent connected". `/api/servers` and `/api/setup` now expose
  `active_url` (the active profile's origin + token, or the local origin), and both pages point
  the link there. The Rust test asserts the remote and local cases; the e2e stubs the profile
  endpoints and checks the link on both pages.
- **Connect an agent is a dialog, shown only when it is needed.** The dashboard embedded the MCP
  JSON forever, even after the bridge was already polling, so the most prominent block on `/` was
  setup the human had finished. The header chip now opens a **Connect an agent** dialog (masked
  config, Copy config button, live online/offline status) on both pages. The dashboard keeps only
  an amber invitation card while no agent is online; it and the rail warning disappear as soon as
  a bridge polls. The old `#mcp-json`/`#copy-mcp` block is gone. The e2e opens the dialog from the
  dashboard and editor, checks the masked token and status, verifies the offline invitation and
  its disappearance after a poll.
- **The server says whether an agent is listening.** A submitted review with no bridge behind it
  waited silently; nothing on screen explained why. `AgentControl` now records `last_seen` on
  every agent-facing call (repo sync is the periodic 60 s heartbeat, `next`/claims touch it too),
  persists `/data/agent-presence.json`, and `GET /api/agent/status` exposes `online` + the
  per-agent rows (online for 180 s). The header chip on both pages says *N agents connected* or
  *No agent connected*; when nobody is online the rail head shows a warning that reviews will
  wait. Nothing is blocked: work can still be created, annotated and submitted. The e2e checks
  both chip states, the warning, recovery after a poll, and the chip's panel. Rust tests cover
  the online/offline expiry and the API status payload.
- **Finishing the open review returns to the dashboard, and filters do not leave a ghost selected.**
  Confirming, cancelling or deleting the review currently on screen left the human staring at the
  same canvas (or a deleted session); a filter or search that no longer contained the open review
  still kept it selected. `railAction` and the bulk confirm/delete now navigate to `/` when the
  current session gets a decision, and `Rail.onViewChange` uses the editor's fresh `meta` plus the
  chosen filter/search to leave a review the view no longer shows. The dashboard refreshes a
  back-forward-cache restore, and all authenticated API/session responses carry
  `Cache-Control: no-store` so a gallery cannot be built from a list fetched before the decision.
  The e2e covers confirm, cancel, delete-of-open, and a filter that hides the open review.
- **Every popup is a fixed frame with a close X.** The Agent settings form scrolled as one block:
  the title and the Save/Cancel buttons left the screen when the project list grew, and there was
  no way to dismiss it except Cancel. The settings, New review, Keyboard and feature/branch
  dialogs now share `.dialog-frame` / `.dialog-head` / `.dialog-body` / `.dialog-foot`: a 32 px
  X in the top-right, an immovable title row, an immovable action row and `overflow-y: auto` only
  on the middle. The e2e opens settings at 380 px height, scrolls the body and asserts the
  head/foot geometry is unchanged, then clicks the X on settings, New review, help and the
  feature decision dialog.
- **One New review button, in the rail, always accent blue.** The dashboard put it in the header
  as a green (`primary`) button while the editor put it at the top of the left rail in blue; the
  primary action moved and changed meaning on navigation. The dashboard now uses the same
  `.btn.accent` rail button as the editor, and the header keeps only icon actions. The e2e asserts
  it is inside `.rail`, absent from `.header-actions`, and that its computed background matches
  `.btn.accent` on both pages.
- **The session rail is one panel on every page.** The dashboard rendered the rail at the CSS
  default 272 px and had no divider, while the editor restored `redline:railWidth` — switching
  from `/` to a review jumped the whole layout sideways. The divider and the shared
  `redline:railWidth` preference now live in one `bindRailResize` helper used by both pages; the
  editor still clamps against the inspector, the dashboard against the viewport. The e2e drags
  the rail on the dashboard, opens a review and asserts the same width, then drags it back.
- **The dashboard gallery is for work in progress.** `renderGallery` listed the last twelve
  sessions regardless of state, so a confirmed or cancelled feature kept a large screenshot card
  on `/` even though it no longer needed the human. The gallery now keeps only sessions whose
  feature decision is still pending, re-renders after a rail action/purge/bulk decision, and each
  card carries `data-session` for the test. Decided reviews remain reachable through the rail's
  All/Done filters. The e2e confirms and cancels two sessions, opens `/` with the All filter, and
  asserts every card belongs to a pending session.
- **A historical round is a viewer, not a dead image.** Opening a round from History left the
  canvas frozen: `pointerdown` returned early whenever `viewing` was set, so no pan or pinch
  survived, and the "Viewing round N" pill sat inside the stage without its own pointer guard.
  The viewer now pans with the main button (grab/grabbing cursor), middle/right-drag and
  space+drag, scrolls with the wheel and zooms with Ctrl/Cmd+wheel, the zoom buttons and
  `+`/`-`/`0`/`1`; drawing and selection stay refused. Space+drag also works when a history row
  still holds keyboard focus, which the old `event.target === document.body` guard silently
  blocked, and the live "agent is working" toast is no longer drawn over an archived screenshot.
  The e2e opens a round and asserts the cursor, a real left-drag, wheel scroll, ctrl+wheel zoom,
  space+drag under row focus, and that panning does not edit the round.
- **The round on the canvas is listed once.** After the agent resolved a review, History showed the
  last submission twice: as *Round N* and again as the live *Current* row, so a session with one
  round looked like two (2026-10-30). `GET /api/sessions/<id>/history` now marks a round `current`
  whenever the saved annotations and picture are still what the canvas shows (submitted, resolved,
  or reopened), and the editor omits the extra *Current* row. Clicking that row returns to the live
  canvas instead of opening a read-only copy. Regression test:
  `the_round_on_the_canvas_is_not_listed_as_a_second_current` plus the *round on the canvas* e2e
  check.
- **One task is one round in History.** Pressing *STOP* and then *Submit again* without touching
  the canvas archived the same annotations and the same picture a second time, so a single review
  showed up as two identical rounds with nothing to diff (2026-10-30). `GET
  /api/sessions/<id>/history` now collapses a run of identical copies (same annotations, same
  PNG) to its newest entry; a round that really changed still gets its own row. Regression tests:
  `resubmitting_an_unchanged_round_does_not_duplicate_history` (Rust) and the *unchanged
  resubmission* check in the browser e2e.
- **The rolling dev release is tagged `dev-build`, not `dev`.** A tag that shares its name with a
  branch is a trap: git resolves the *revision* `dev` to the tag (`git log dev`, `git diff dev`,
  `git merge dev` all used the tag's commit, silently) and `git worktree add … dev` refuses it with
  `fatal: ambiguous object name`. The channel tag now has its own name; `DEV_MANIFEST`, the dev
  workflow and `docs/DESKTOP.md` follow. The old `dev` release is kept only long enough for an
  installed dev build to pick up a transitional `latest.json`, then removed. Stable releases are
  untouched.
- **A review belongs to a feature, not to a session id.** A review the human created without
  naming a branch (the New review dialog never asked) was claimed on the fallback
  `session/<id>`: the rail showed `session/20261030-…`, and the agent was told to push a branch
  named after a timestamp. Every review now carries a feature branch from the moment it is
  created — `feat/<title-slug>-<id-tail>`, e.g. `feat/wrong-branches-52e0` — and the New review
  dialog has an optional *Feature branch* field whose placeholder shows that name while you type
  the title. The header shows the branch, and clicking it renames it through the new
  `PUT /api/sessions/{id}/branch`; a submitted round keeps its name, because the agent's worktree
  was already created for it. Sessions claimed by v0.21 with a persisted `session/<id>` are read
  back as their `feat/…` name (no rewrite of old folders), and the bridge's `work_key` derives the
  same name, so the claim, the prompt and the rail agree. Tests: the API creates without a branch
  and asserts claim work key, rail row and rename; the store test pins the naming; the e2e creates
  a review through the dialog and asserts the rail branch is `feat/…`.
- **A tag named like the base branch no longer kills the run.** The bridge picked the worktree's
  start point with a short ref (`dev`, `main`, `origin/dev`, …). This repository also has a tag
  `dev`, so `git worktree add` answered `warning: refname 'dev' is ambiguous` and
  `fatal: ambiguous object name: 'dev'`, and the review died with *I cannot start this review*
  before the agent ever ran. The candidates are now fully qualified (`refs/heads/dev`,
  `refs/remotes/origin/dev`, …) and peeled to a commit, which also removes the silent variant: a
  successful resolution that picked the tag's commit instead of the branch tip. A regression test
  creates a tag with the branch's name and asserts the worktree starts at the branch tip.

## 0.21.0 — 2026-10-30

- **The header is icons, and the status pills are gone.** `submitted`/`replied`/`working` sat in
  the header as GitHub-style pills and competed with the review title, although the rail card, the
  stage toast and the composer/STOP button already say whose turn it is; they are removed. Agent
  settings is now a solid gear instead of a `⚙` glyph, keyboard shortcuts is a keyboard instead of
  `?`, the theme button is a sun/moon SVG, and the two panel toggles are left/right frame icons
  with `aria-pressed` and a *Hide/Show* title, separated from the app tools by a hairline. Every
  icon is inline SVG, so none of it depends on the platform's emoji font. The e2e clicks both panel
  toggles, asserts the canvas bitmap follows the grid, swaps the theme icon, and checks the header
  has no `.pill` while the state still renders in the toast and the rail card.

- **The dev server follows the dev build.** Every push to `dev` now publishes
  `ghcr.io/erikibolya/redline:browser-dev` (plus a sha tag) from the same workflow as the Windows
  dev build. The `redline-dev` stack can pull that tag instead of building from the working tree,
  and a User Script or scoped Watchtower updates it every few minutes. The image includes
  Chromium, so capture works on dev too.
- **Agent concurrency control (foundation).** Sessions carry an optional `branch` declared by the
  agent (fallback `session/<id>`), the rail shows it after the relative time, and the server has
  branch ownership leases plus global/per-project run limits — defaults 2 global and 1 per
  project, editable in the new **Agent settings** dialog. A waiting claim keeps the branch lock
  but frees the execution slot. The bridge runs parallel claim workers against this API.
- **The agent bridge runs reviews in parallel.** `redline-agent` now runs up to four claim workers
  (`REDLINE_AGENT_WORKERS`, default 4) against `/api/agent/next`; each claim gets its own git
  worktree on the declared branch, heartbeats its lease, moves to `waiting` when the human has the
  ball (freeing the run slot but keeping the branch), and releases/removes a clean worktree when
  the round is resolved. The server's global/per-project limits and the one-owner-per-branch rule
  still apply; a dirty worktree is left for inspection.
- **Freehand strokes are numbered and listed again.** `renderInspector` read
  `annotation.start.x` for every non-text shape, but a pen stores `points`; the resulting
  `TypeError` aborted `mutate` before `render()`, so the live stroke stayed on screen with no
  badge and no inspector row (and could only be removed with Ctrl+Z). Shapes now get explicit
  labels per type, and the e2e draws a pen, asserts its row/number and badge pixel, submits it
  and checks the points in the server JSON.
- **Regression gate.** `docs/TESTING.md` defines the tool matrix, the bug-fix protocol (failing
  test first), the feature protocol and the release checklist. The e2e now draws every drawing
  tool (rectangle, ellipse, arrow, pen) instead of only clicking its button.
- **A reply is not the finish line.** The run card treated `agent_state=replied` as the end of the
  run, so it said **Run finished** while the review was still submitted, the canvas was frozen and
  the red STOP button was armed — and then the agent kept working for minutes. `reply` now only
  marks an answer: while the round is still submitted the card reads **Agent replied — waiting for
  it to finish** (amber, with the age of the last step), a later `progress` line turns it back into
  **Working**, and **Run finished** is reserved for a round that was handed back (`post_round`) or
  resolved. The server no longer freezes `working` after a reply on an open round; the browser e2e
  covers the answered-but-still-running state and the resume.
- **The rail says who has the ball.** A submitted round that no agent has picked up used to carry
  the **new** badge (which only meant "this browser has not looked at it yet"), so a review
  sitting in the queue read as the human's turn. It now reads **waiting for agent**; `new` is
  reserved for rounds that are back in the human's hands, and `working` still wins while an agent
  runs; a submitted round the agent has answered but not resolved now reads **replied**. The
  project header keeps a single count instead of a green waiting count beside the same total. The
  browser e2e asserts the badge and the missing header count.
- **The Servers page Remove button is readable again.** `.server-actions .btn.danger` kept the
  filled `.btn.danger` background and only overrode the label to `--danger`, so *Remove* was red
  text on a red button. It is now an outline in both themes (label contrast 9.2:1 dark, 6.3:1
  light), and the e2e measures that contrast instead of trusting the stylesheet.
- **Adding a server lives on the Servers page only.** The app menu and the tray no longer carry
  **Add a server…**; **Servers…** stays and its **Add server** button opens the same wizard. The
  wizard's **Use this computer** button is gone (the *This computer* row in the list is always
  present and cannot be removed) and is replaced by **Back**, which returns to the server list.
- **Each session is one feature.** The rail's ✓ now asks for confirmation and records
  `feature_confirmed_at`. The ✕ opens a feature-actions dialog: **Cancel feature** records
  `feature_cancelled_at` and keeps the screenshot/annotations, while **Delete files…** removes
  them permanently. A decided feature offers **Reopen** in the same menu. Both decisions close
  the session (`resolved`) and ↺ Reopen undoes them. Agents read the decision as `feature_state`
  (`pending`/`confirmed`/`cancelled`) from `GET /api/sessions`, MCP `get_feedback`,
  `wait_for_feedback` and `list_reviews`; RedLine itself never touches git. A new submission or
  agent round makes the feature pending again. Bulk mode gained **Confirm**; multi-delete stays
  in bulk **Delete**, and **Delete cancelled** purges only cancelled features; confirmed features
  stay as the log.

## 0.20.0 — 2026-10-30

- **A locked canvas also freezes the view.** While a submitted review waits for the agent, the
  wheel, zoom buttons, keyboard zoom and pan no longer move the screenshot, and a light scrim
  covers the stage; STOP removes both. The toolbar and status pill stay above the scrim.
- **Multiple server profiles in the desktop app.** The local **Servers** page (tray → *Servers…*)
  lists Production, Dev and *This computer*, shows each server's version/state and switches the
  window with one click. Profiles and tokens are stored side by side in `config.json`; switching
  only changes the active profile, so other servers stay signed in. Adding a server reuses the
  existing wizard, and the old `remote_url`/`token` config is read as the first profile.
- **The annotation list is a list, not a stack of broken frames.** In a submitted (read-only) round
  the separator between rows was a `border-top` on a rounded row, so it painted only the middle
  segment between the two rounded corners and every row looked like an open frame. The separator is
  now a full-width hairline and read-only rows are a plain `.ann-row.readonly` (no pointer cursor,
  no hover highlight, no delete button). The browser e2e measures the separator and the class.

- **Panels are resizable.** The two column borders and the line above the conversation in the
  editor are now drag handles: the session rail, the annotation inspector and the conversation
  each remember the size you drag them to (`localStorage`), the canvas is re-measured mid-drag, and
  the handles are keyboard-operable (`←/→` or `↑/↓`, Shift for bigger steps). Limits keep the stage
  at least ~360 px wide and stop the conversation from swallowing the annotation list. The browser
  e2e covers all three dividers, including a reload.
- **A finished round can no longer keep saying "working".** The agent's last narration line often
  arrives *after* `post_round` (review handed back) or `reply` (answer posted); `progress` used to
  set `agent_state=working` unconditionally, so the review kept pulsing a spinner with a green
  **Submit review** button and the human could not tell a live agent from a dead one. The server
  now only marks a round working while it is still submitted, the UI derives the state from
  `status` as well, and the run card shows how long ago the last step was ("Used 1 tool · last
  step 6m ago"). The narration event itself is still recorded.

- **The rail shows which review and which project an agent is working on.** While an agent runs a
  round the review row carries a spinner and a **working** badge (in place of the annotation
  count), its dot pulses, and its project header shows the same spinner and label — with a count
  when more than one review runs at once — even when the project is collapsed. The rail polls the
  session list every 8 s and redraws only when the set of running reviews changes; the indicator
  clears as soon as the agent replies or the human presses STOP. The browser e2e asserts both the
  appearance and the removal.

- **A new review starts in the project you were just working in.** Opening a review (or picking a
  project in the dialog) records it in the browser, and the next **Ctrl+V** or **New review** opens
  with that project already selected instead of the placeholder — on the editor and on the start
  page, and across reloads. The memory is per browser and per server origin; a project that no
  longer exists or is hidden from the picker falls back to the placeholder. The browser e2e covers
  both the preselect and its survival across a reload.

- **The submitted lock actually holds.** After a submit, Delete/Backspace, arrow-key nudging,
  undo/redo, the inspector's align/layer/group buttons, the note field and the keyboard tool
  shortcuts could still change annotations while the stage pill claimed the canvas was locked.
  All annotation writes now funnel through one `Editor.mutate` guard that refuses while the round
  waits for the agent (and while a history round is open), the drawing tools are `disabled` and
  dimmed instead of merely ignoring clicks, and the inspector's write toolbar hides with the note
  field. The browser e2e proves the refusal, not just the flag.

## 0.19.0 — 2026-10-29

- **Projects are repositories now.** The agent discovers git repos from OpenCode
  (`/api/project`) and by scanning its repo root, then syncs them to `POST /api/agent/repos`.
  Projects store a **portable relative key** (`redline`, `ActiUnit/Au.Net`) and an agent id, never
  an absolute mount path. The New review select is grouped into **Repositories** and **Other
  projects**, requires a choice, and hides empty legacy leftovers. `+ New project…` still creates a
  label without a repo.
- **Reviews run in their project's repo.** Before each run the agent resolves the project key
  under `REDLINE_AGENT_REPO_ROOT`, canonicalizes it and refuses a missing or escaping path instead
  of silently using the default repo. Legacy projects without a key keep using
  `REDLINE_AGENT_REPO`; error reviews get a reply and are resolved so they do not spin.
- **New review picks a project from the server's list.** The free-text project field (and its
  datalist, which only the editor had) is now a `<select>` fed by `GET /api/projects` on both the
  editor and the start page, so projects created by agents are selectable without typing.
  **+ New project…** reveals a name field; the project is created on first use.
- **One URL field, explained.** "Page URL" and "Or screenshot a URL" were two boxes doing almost
  the same thing. There is now one *Page URL*: metadata for an uploaded screenshot, or — when no
  image is attached — the page the server captures. "What should be checked?" is now "What should
  the agent check?" with a hint that the note becomes the first message.
- **The conversation shows the agent's run at a glance.** A card under the step lines reads
  **Working** with a pulsing dot while `agent_state` is `working` and tallies the round:
  **Used 4 tools: Edit and Shell**, from the `tool` names sent with `progress`. It settles to
  **Run finished** when the agent replies, counts only the current round, and stays out of the way
  when nobody has reported anything. The browser e2e covers it.
- **The window follows the configured server.** The tray said *Server: …* from `config.json`
  while the window could still be sitting on the local (or an old) server, so reviews were saved
  somewhere the agent never watches. The label now reads **Configured**, *Open RedLine* navigates
  the window to that server, and focusing a window left on another origin puts it back — except
  the setup page, which is served locally on purpose.
- **The submitted lock is a status pill, not a modal.** While a review was `submitted` a dark
  scrim plus a centred "Submitted — waiting for the agent / STOP unlocks the canvas" box covered
  the screenshot — while the same agent state already floated above the toolbar. The modal is
  gone (`#review-lock` and `.lock-box` removed); the stage status pill is centred where the box
  was, keeps the message short ("Agent is working — …" / "Waiting for the agent…"), carries the
  STOP hint as a tooltip and lets the screenshot show through. The `locked` guards in `app.js` are
  untouched, so editing stays blocked.

## 0.18.0 — 2026-10-29

- **Menu actions no longer run twice.** Tray items use their own `tray:` ids and the dispatcher
  ignores the same action within 400 ms, so update dialogs and the repository picker open once.
- **The connect wizard matches the app and can be left.** It uses the app's tokens and buttons, has
  a **← Back to sessions** link, a proper **Use this computer** button (disconnect, no token
  needed) and a clearer hint where the token comes from.
- **Bulk operations in the rail**: *Select* turns rows into checkboxes with a bar to resolve or
  delete several sessions at once; Escape leaves the mode.
- **Touch toolbar**: on coarse pointers the tools, chips and buttons grow to 40 px targets and the
  rail's per-session actions stop being hover-only.
- **Accessibility**: visible `:focus-visible` rings, accessible names on the canvas, composer,
  session rows and history rows, and the conversation is an `aria-live` region.
- **README rewritten** for 0.18 (desktop, server, agent bridge, the round loop, configuration).

## 0.17.0 — 2026-10-29

- **Submitting locks the canvas**: a dark overlay blocks editing and the composer is disabled
  while `submitted`; the primary button turns into a red **STOP**.
- **STOP** calls `POST /api/sessions/<id>/cancel`, unlocks the canvas and keeps the annotations.
  A round no agent has read yet is withdrawn from history; a round the agent has seen stays.
  The bridge watches the session every 2 s while the agent runs and kills it on cancel.


- **A round is archived when it is submitted**, not when the next one replaces it. History fills
  up immediately, keeps the source screenshot (`history/<stamp>-source.png`) and numbers rounds
  oldest = 1. Two submissions in the same second get unique stamps.
- **History is a round viewer**: `Current` is always the way back, every round is clickable and
  opens the source screenshot plus its annotations read-only. The separate ghost (`◫`), text diff
  (`Δ`) and open-PNG (`↗`) buttons are gone, and so is the ghost chip.
- **The conversation no longer shows the round screenshot**; the canvas and History own it.
- **Drafts**: unsent annotations and the composer text survive switching sessions. A new agent
  round clears the draft (the coordinates belong to the old image) and says so. On quit, the shell
  asks before discarding unsaved drafts (the webview marks the window title with `•`).


## 0.15.0 — 2026-10-29

- **Dev update channel in the desktop app.** Help/tray gains *Install latest dev build…* (the
  rolling `dev` prerelease) and *Revert to last stable version…* (the updater's comparator accepts
  a downgrade, so a dev build can go back). Every push to `dev` publishes a dev build
  (`.github/workflows/dev.yml`).
- **History rows** are compact and bounded; the empty-state class no longer leaks into a populated
  list.
- The browser e2e waits for the canvas to actually swap after a round, not only for the metadata.

## 0.14.1 — 2026-10-29

Fixes found by using rounds for real.

- **The canvas actually switches to the new round.** `/sessions/<id>/image` is a stable URL while
  the file behind it changes, so the browser kept showing the previous screenshot; it is now
  cache-busted with the image name.
- **History updates.** The count is loaded with the session and refreshed when a round arrives,
  instead of staying at 0 until the disclosure is opened.
- **No invented messages from the bridge.** It no longer posts "check `git status`" or resolves on
  the agent's behalf; it logs what happened and leaves the session in the state the agent left it.
  A short grace period covers the CLI exiting just before the last tool call lands.
- **Rounds keep running.** The bridge keys its "already handled" set by submission, not by session,
  so the same review can be submitted again after a new round.
- `post_round` clears `resolved_at` when it reopens a session.

## 0.14.0 — 2026-10-29

One task is one session: the agent's follow-up no longer opens a new review.

- **`post_round` / `capture_round`** (MCP): post the next screenshot (uploaded or captured) into an
  existing review. The previous round is archived, the canvas image is replaced, annotations are
  cleared and the session returns to the human (`open`), so the rail keeps one item and History
  fills up.
- **The editor follows the round**: when the poll sees a new image, the canvas reloads it and the
  conversation shows the round with its thumbnail.
- **Waiting state on the canvas**: while `submitted`, a small pill at the top of the stage says
  "Waiting for the agent…" or the latest activity line; it never blocks annotation.
- The bridge prompt now forbids creating a new review for a follow-up round and tells the agent to
  hand the session back instead of resolving when it posts a screenshot.

## 0.13.4 — 2026-10-29

- **New-message indicator in the conversation.** When you scroll up and a reply or activity line
  arrives, the header shows `↓ N new messages`; click it (or scroll back down) to clear. The log
  no longer jumps while you read.

## 0.13.3 — 2026-10-29

- **Conversation keeps your place.** The 5 s poll only re-renders when something changed and only
  follows the tail when you are already at the bottom; scrolling up to read history no longer
  yanks you back.
- **No more stdout dumps in the conversation.** When the agent forgets to report, the bridge posts
  one short note (changes are in the working tree) and resolves the review; the transcript stays
  in the agent's server log. Failure messages are a single line, not a pasted stderr.
- The bridge prompt now makes the closing `reply` + `resolve` explicit.

## 0.13.2 — 2026-10-29

- **New review** — the drop zone shows a thumbnail of the attached image (dropped, chosen or
  pasted) instead of only `image.png — 285 KB`. You can now see that you pasted the right screen
  before creating the review. The caption keeps the file name and size plus *click to replace*;
  the preview is capped in height and the dialog scrolls on short screens.

## 0.13.1 — 2026-10-29

- **Desktop app** — *Check for updates…* moved from the **RedLine** menu to **Help**, where people
  look for it. The tray menu keeps its own entry (a tray has no Help submenu).
- **Editor** — conversation timestamps are shown in the reader's timezone. The API sends RFC 3339
  UTC and the UI sliced the clock out of the string, so a submission made at 13:03 in Prague read
  “12:03”. Relative times (“5m ago”) and day labels were already local and are unchanged.

## 0.13.0 — 2026-10-29

The agent side becomes part of the product.

- **`redline-agent`** — a Rust bridge (binary + library) that turns a submission into an OpenCode
  run: polls submitted reviews, reads the feedback, runs `opencode run` in the repository
  (optionally against an already running OpenCode), and lets the run report `progress`, `reply`
  and `resolve`. Replaces the Python prototype.
- **Desktop app** — tray menu: *Choose agent repository…*, *Start local agent*, *Stop local agent*.
  Settings live in `agent.json`; the agent talks to the embedded server in standalone mode or to
  the configured remote server.
- **`docs/LOCAL-AGENT.md`** — server-on-a-NAS vs everything-on-one-PC, headless and container
  recipes.
- Release assets: `redline-agent` for Windows and Linux.

## 0.12.0 — 2026-10-29

Silence after Submit is the bug this release fixes.

- **Waiting states in the conversation**: "Waiting for an agent" (nobody has read the submission
  yet, with its age and the MCP calls that would read it), "Agent received this" with the time,
  then the live activity line while the agent works.
- **`REDLINE_WEBHOOK_URL`**: the server POSTs a small JSON body on every submission so a host can
  wake an agent or a relay. Plain HTTP, fire-and-forget, never blocks the submission.
- Fixed: `[hidden]` was overridden by component `display` rules, so the ghost chip (and any future
  hidden button) showed up with an empty label.

## 0.11.0 — 2026-10-29

Small daily-use wins, plus the comparison the review loop was missing.

- **Ctrl+V** opens the New review dialog with the clipboard image attached; no file on disk.
- **Gallery on `/`**: the last twelve reviews as cards with screenshot, project, time and status.
- **Diff overlay**: the History panel's `◫` ghosts a previous round's annotations behind the
  current ones (35 % opacity) with a `Ghost · <stamp>` chip in the header to clear it.
- Fixed: the New review dialog was never wired on the editor page, so it did nothing there.

## 0.10.0 — 2026-10-29

The rail becomes a list of projects, so a conversation has a home and a history.

- **Projects are folders**: `sessions/projects/<slug>/project.json` with their sessions one level
  deeper. `GET`/`POST /api/projects` (counts, waiting, last activity), a `project` argument on
  `review_screenshot` / `capture_review`, a filter on `list_reviews`, a new `list_projects` MCP
  tool and `redline open --project`.
- **Inbox, not migration.** Sessions created without a project stay flat and are labelled Inbox on
  read; no data is moved.
- **Rail grouping**: collapsible project headers with waiting/total counts, filters and search
  still apply inside each project, day separators only for long lists.
- The New review dialog gains a project field with autocomplete.

## 0.9.1 — 2026-10-29

- Hide the agent-state pill when there is no state (author `display` was overriding `[hidden]`).

## 0.9.0 — 2026-10-29

The review becomes a conversation: the agent announces itself, and you submit from the chat.

- **Agent state you can see.** `get_feedback` stamps `seen_by_agent_at` and sets `received`; the new
  MCP `progress` tool narrates each step (`working`, with a message and an optional tool name in
  `agent-events.jsonl`); `reply` sets `replied`. The header shows the state, the conversation shows
  the activity feed.
- **Conversation + composer.** The inspector's lower half is a timeline — agent request, your
  prompt, activity events, replies — with a prompt box and **Submit review** at the bottom. The
  top-right Submit button is gone.
- **The prompt travels with the submission** (`meta.prompt`, shown in `review.md` and returned by
  `get_feedback`).
- `GET /api/sessions/{id}/events` exposes the activity feed.

## 0.8.0 — 2026-10-29

The review becomes a conversation: text moves off the canvas and onto the marks.

- **Notes on shapes.** Every rectangle, ellipse, arrow and freehand mark carries an optional note,
  written in the inspector right after drawing. The text tool is gone; legacy text annotations
  still render, read-only.
- **Numbers everywhere.** Shapes get a numbered badge on the canvas, in the annotation list, on
  `annotated.png` and in `review.md`, so the agent can match a paragraph to a region.
- **Ellipse tool** (`E`) — the missing "circle that" gesture.
- **RedLine branding** in the UI, CLI banner, desktop menus and docs (display only: binary, image,
  container and paths keep `redline`).
- `docs/UX-PLAN.md`: projects, chat flow and agent status — the rest of the 1.0 rework.

## 0.7.0 — 2026-10-29

Found by pointing redline at itself: an agent reply had nowhere to appear, and the start page
printed the access token.

- **Agent replies are visible in the editor.** The MCP `reply` tool wrote `agent-thread.jsonl`,
  but no route and no screen read it — a human who submitted a review saw nothing. New
  `GET /api/sessions/{id}/thread` plus an "Agent replies" panel in the inspector, polled every
  5 s, so an answer appears without reloading.
- **The token is masked on the start page.** `GET /api/config` returns the bearer token and the
  page rendered it, so a screenshot of `/` (which redline's own capture produces) leaked it. The
  block now shows `Bearer ••••••`; "Copy config" still copies the real JSON.
- Tests: `agent_replies_are_readable_by_the_human` (20 API tests; 50 total).

## 0.1.0 — 2026-10-28

Initial MVP. Renamed from the working title `au-visual-review` to **redline**; published publicly at
https://github.com/erikibolya/redline under the MIT license.

- Stdlib Python CLI: `serve`, `open`, `list`, `get`, `wait`, `remove`.
- File-based sessions under `sessions/<id>/` (gitignored).
- Browser session list and canvas editor.
- Rectangle, arrow, pen and text annotations; undo/clear; submit/resubmit.
- Output: `meta.json`, `annotations.json`, `annotated.png`, `review.md`.
- Shared `--sessions` directory for Windows browser + WSL/agent split.
- Unit tests for session creation, submission output and ID validation.
- Documentation: README, AGENTS.md, PLAN.md, architecture/API/workflow docs.
- MIT license.
