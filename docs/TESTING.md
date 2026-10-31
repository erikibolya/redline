# Testing and regression gates

A feature is **not done** when the code renders or the endpoint returns 200. It is done when an
automated test exercises the user's path and asserts both the visible result and the persisted
state. A bug fix starts with a test that fails on the old code.

This document is the contract for every RedLine feature and release. The tool matrix is not
optional: a green suite that never drew a freehand stroke is not a green suite.

## Layers

| Layer | Command | What it proves |
|---|---|---|
| Rust unit/integration | `docker run --rm -v /mnt/user/Development/repos/redline:/src -w /src/desktop rust:1-bookworm cargo test --workspace` | store layout, HTTP/API, MCP, auth, history, feature decisions, path/config helpers, CLI |
| Browser e2e | `docker build -q -f desktop/Dockerfile.browser -t redline:local .` then `REDLINE_IMAGE=redline:local bash desktop/scripts/e2e.sh` | real Chromium: pointer drags, clicks, dialogs, DOM/geometry/canvas pixels, server round-trip |
| GitHub CI | required checks on `dev` and `main` | Linux + Windows, Tauri compile, browser e2e, container/image build |
| Release smoke | the checklist below against the exact tag/image | the thing users install, not just the source tree |

`desktop/scripts/e2e.sh` is the release gate. Run it against the release image, not only a local
driver image. The e2e listens for uncaught page errors; a silent `TypeError` still fails the run.

## The tool matrix

Every drawing tool must be **drawn**, not just selected. The e2e asserts the annotation exists,
the inspector row and its number exist, the badge is painted (for shapes), and at least one tool
proves the submitted JSON keeps its geometry.

| Tool | Draw | Badge | Inspector row / number | Move | Note | Undo | Submit |
|---|---|---|---|---|---|---|---|
| Select / move | rect fixture | — | — | yes | — | yes | — |
| Rectangle | yes | yes | yes | yes | yes | yes | yes |
| Ellipse | yes | yes | yes | via select | via inspector | yes | shared |
| Arrow | yes | yes | yes | via select | via inspector | yes | shared |
| Pen / freehand | yes | yes | yes | yes (`points`) | via inspector | yes (`points`) | yes |
| Text notes | legacy read-only / inspector | n/a | yes | n/a | n/a | — | server model |

The pen regression is why this table exists. The old e2e clicked the pen button and asserted
`window.redline.tool === "pen"`, then never drew. The inspector tried to read
`annotation.start.x` on a `{ type: "pen", points: [...] }` object, threw, and `mutate` aborted
before `render()`; the live preview was already on the canvas, so the tool *looked* alive while
the finished annotation had no badge and no list row. The new test draws a stroke, asserts the
row/number/label and a red badge pixel, submits it, and checks the points in the server JSON.

## Submission history

- Rust `tests/api.rs`: `resubmitting_archives_the_previous_round` (a changed round gets its own
  row), `resubmitting_an_unchanged_round_does_not_duplicate_history` (agent reads the round, the
  human presses STOP, then *Submit again* with the same annotations and picture → still one row;
  a later changed round adds one), `the_round_on_the_canvas_is_not_listed_as_a_second_current`
  (resolve and reopen keep the round `current`, and an agent picture makes it not current).
- Browser e2e: open History, assert one row per round, that an unchanged resubmission adds no
  row, that a row opens read-only while *Current* returns to the live canvas, and that after STOP
  the round on the canvas is one *Round N (current)* row with no extra *Current*.
- Round cost (2026-10-31): Rust `tests/api.rs` `every_round_reports_how_long_it_ran` — an older
  round ends where the next submission begins, the newest submitted round has no end yet, and
  `resolve` is the end of the round the agent had. The browser e2e asserts a History row prints
  `· took …`. In the rail, a resolved review keeps a `done` badge plus `· took …`, and an open row
  with nothing drawn on it says `open` instead of staying blank.

## Run state and agent output (2026-10-31)

- Rust `tests/api.rs`: `the_sessions_list_exposes_only_a_live_run_as_working` — no claim reads
  `agent_running=false`, a running claim `true`, a `waiting` claim `false` again, a released claim
  `false`.
- Rust `tests/mcp.rs`: `a_failed_run_hands_the_round_back_and_says_so` — `reply` with
  `state="failed"` reopens the submitted round, keeps `agent_state=failed`, stores
  `agent_failure {code,message,at}` (`timeout_idle` here), and returns it plus `review_guidance` in
  `get_feedback`; an unknown state is an error; the default stays an ordinary `replied` with the
  round still submitted, and a new run/reply clears the old failure.
- `redline-agent` tests: `log_line` strips terminal colours, drops blank lines and bounds the
  length; `a_run_is_killed_when_it_goes_silent_or_exceeds_the_hard_cap` runs a fake OpenCode that
  goes silent (must fail with `timeout_idle`), one that keeps printing (must survive the idle
  window), and one that hits the hard cap (`timeout_hard`); the two-worktree bridge integration
  test asserts the fake agent's own stdout line arrives in the review as a `state="log"` event.
- Browser e2e: a running claim on an answered round restores the rail's **working** badge and the
  project spinner, and releasing it returns to **replied**; a failed run unlocks the canvas, the
  tools and the composer, shows **agent failed** on the rail, a red run card and the "it is yours
  again" notice with the bridge's message; a `code=timeout_idle` failure shows **agent timed out**
  on the rail and in the card, not the generic label; the **output** button opens the streamed
  `log` lines in a popup centred in the viewport (asserted by geometry, and by the fact that
  nothing is injected into `#conv-log`), a poll keeps feeding it while it is open, the stage toast
  prints the last *step* and never a log line, and the popup closes from its button, `Esc` and not
  at all by itself on a reload.

## Agent concurrency (v0.22)

- Rust `tests/agent.rs`: global/project capacity, waiting frees a run slot but keeps the branch,
  one owner per branch, lease expiry, heartbeat/release ownership, persistence, settings
  validation.
- Rust `tests/api.rs`: settings round-trip and per-project overrides, claim/next, branch locks,
  `waiting` transitions, `next` skipping locked branches.
- `redline-agent` tests: a worktree is created on the declared branch and reused; two claims run
  in two separate worktrees (the bridge integration test starts a real server and a fake
  `opencode`); the prompt carries the branch; a tag that shares the base branch's name (`dev`)
  neither breaks the worktree nor moves its base off that branch.
- Browser e2e: the Agent settings dialog saves global + per-project values, and the rail shows the
  session's branch next to the relative time.

## Feature branches (v0.22)

- Rust `tests/store.rs`: `default_branch` folds Czech titles into a git-safe `feat/<slug>-<tail>`,
  and a review created without a branch already carries one.
- Rust `tests/api.rs`: a review created without a branch claims and lists on `feat/…` (never
  `session/<id>`); renaming is `409` while submitted and persisted when accepted; a v0.21
  `session/<id>` branch is read back as the derived `feat/…` name in both `GET /api/sessions` and
  `GET /api/sessions/{id}`.
- Browser e2e: the New review dialog suggests `feat/<title>` in the *Feature branch* placeholder,
  creates a review through the dialog with the field left empty, and asserts the rail row and the
  loaded meta carry `feat/<title>-<tail>`.

## Handoff after confirmation (v0.23)

- Rust `tests/store.rs` / `tests/api.rs`: confirming with `handoff:true` queues
  `handoff.status=requested`; confirming with `false` records only the decision; a new round or
  reopen clears it; `mark_handoff_running` is idempotent; `report_handoff` records the
  integration's `label`/`url`/`message`; `/api/agent/next` returns `job=handoff` only to a bridge
  that advertises `jobs:["review","handoff"]` and never to an old bridge; a handoff job's claim
  uses the branch the review actually worked on (including a legacy `session/<id>`).
- Rust `tests/mcp.rs`: `handoff_status` is listed, refuses an unconfirmed feature, records a
  generic done/failed result with label/url/message, and refuses a non-http URL.
- Rust `tests/agent.rs`: a bridge advertises `jobs`, a later sync does not downgrade it, and an
  old presence file defaults to review-only.
- `redline-agent` tests: the review prompt explicitly forbids opening a PR/feature-ready/merging;
  the handoff prompt grants the PR/merge and includes the `handoff_status` curl with generic
  label/url/message; a legacy session branch with the commits wins over a `feat/...` name with
  none.
- Browser e2e: the confirm dialog starts the handoff checkbox off for a review-only agent; tick
  it → rail shows `confirmed` + `handoff queued`; a queued/failed/running handoff keeps the review
  in **Active**, not **Done** (asserted right after the confirmation and after the retry); call
  MCP `handoff_status` done with label "merged" + URL → the rail's refresher shows the label and
  link from Done; report failed with a URL + message → the poll moves it back into Active and
  marks the chip, the rail shows the link and the row `↻` retry; hover and click
  it → `/handoff` queues it again and the badge returns to `handoff queued`. The e2e also asserts
  the retry does not select or close the review, that the rail offers exactly
  `All`/`Active`/`Done`, and that a stored `submitted` preference migrates to `Active`.

## Every UI control

- A test must **click** the control, not only call the method or press the shortcut.
- Assert the visible result (text, class, geometry, pixel) **and** the state/server result.
- Cover both directions: lock/unlock, confirm/reopen, cancel/reopen, select/deselect, open/close.
- Dialogs: every button plus Escape/cancel.
- Destructive actions: assert the confirmation and the state after it.
- If the control is visual, measure it. A border is not a separator just because the class exists.

## The header brand

The shell header carries the icon alone — no `RedLine` wordmark, because the app name printed next
to the session title read as a second heading. The mark is as tall as the **two-line text block**
it stands beside (title + subtitle), not the title's font size: sized to one 13 px line it read as a
chip floating next to the title, and the human had to draw the rectangle they wanted.

The e2e measures the brand on the editor **and** on the dashboard: no text node, exactly one
`.brand-mark` with `aria-label="RedLine"`, a mark at least twice the title's font size (so it cannot
regress to a one-line chip) and within 2 px of the title-block height, centred in the header row.
The block is the union of the title and subtitle lines, never the `.subline` that also carries the
branch chip. Text is not a substitute for geometry here: "the wordmark is gone" and "it is on par
with the text" are measurements, not class names.

## Bug-fix protocol

1. Reproduce the bug in the real UI (or the real API) and capture the failing state.
2. Write the smallest automated test that fails on the current code and passes after the fix.
3. Fix the root cause, not the symptom.
4. Run the **full** suite, not only the new test.
5. Add a §7 lesson to `AGENTS.md` if the bug exposed a process gap.
6. Keep the test forever; it is the regression fence.

## Video reviews (phase 2)

The e2e records a real WebM in Chromium (`canvas.captureStream` + `MediaRecorder`) and drives the
same path a human does: upload, play, draw, range, zoom, submit.

- Rust `tests/store.rs`: `video_sessions_keep_media_metadata_and_timecoded_annotations` (media,
  poster, `at`/`until` survive submit and `review.md` prints `00:01.250–00:02.500`);
  `video_uploads_are_bounded_before_they_are_read` (250 MB sparse file rejected);
  `a_video_longer_than_five_minutes_is_refused`; `a_heic_image_is_not_accepted_as_an_mp4`.
- Rust `tests/api.rs`: `video_sessions_are_created_and_served_with_range_requests` (create,
  metadata, poster on `/image`, `206` with `Content-Range`, invalid range `416`, non-video
  `400`), `video_sessions_accept_a_multipart_browser_upload`.
- Rust `tests/mcp.rs`: `review_video_creates_a_session_with_media_and_a_video_url` (tool listed,
  path/base64, poster, `get_feedback.media` + tokenised `video_url`).
- Browser e2e: the dialog previews the recorded clip; the upload stores duration/dimensions and a
  poster; `/image` is PNG and `/media` answers `206`; the editor loads muted with the video bar;
  play turns the control into pause; drawing stamps `at`; the inspector prints the timecode and
  groups rows under chronological timestamp headers; a group header seeks; a range group stays
  active to its end; other moments are hidden by default, a selected annotation still obeys the
  playhead, Ghosts shows a 0.12 ghost, and the current annotation is full; the ⏱ button sets
  `until`; zoom narrows the window and Fit restores it; the timeline shows a marker and a range
  band; `K` pauses and `.` steps a frame; a second shape at the same moment stays in one group; a
  third at another moment gets its own frame and group; the draft is autosaved to localStorage,
  survives a dashboard round-trip with its `at` values and groups, and is cleared by submit; a
  draft left over an agent `post_round` is discarded and says so; the submit persists
  `at`/`until`; `review.md` prints the range.
- Frames and rounds: two annotated moments produce two `frames/*.png` files served as PNG, a
  1014×394 contact sheet (not the current frame), and `### Frames` entries in `review.md`; the
  history entry records `frames: 2`. STOP after `get_feedback` keeps the round, **New video**
  asks first, a second recorded clip becomes the live 0.63 s round with empty annotations, the
  previous clip answers `200 video/webm` under `history/<stamp>/video`, and clicking the history
  row plays that clip read-only with its three annotations and two timestamp groups; *Back to
  current* restores the new live clip and the New video control.
- Rust `tests/api.rs` also covers `multipart` submit dedupe/limits, `POST
  /api/sessions/{id}/video` + archived-range, and MCP `post_round` video in `tests/mcp.rs`.

## Feature protocol


1. Write the acceptance criteria as observable states (DOM, JSON, canvas pixels).
2. Add the feature's row to the tool/feature matrix **before or with** the implementation.
3. Test the full user path: create → interact → persist → reload → undo/delete.
4. Test the failure paths: empty, locked, cancelled, no annotations, invalid input.
5. Update `docs/UI.md`, `docs/API.md`, `CHANGELOG.md`, and `PLAN.md` when behaviour changes.
6. Run Rust tests and the browser e2e locally; CI runs both on the PR.
7. Release only from a green tag; rerun the e2e against the release image.

## Release gate

- [ ] `cargo test --workspace` green
- [ ] `desktop/scripts/e2e.sh` green (see `PLAN.md` for the current count)
- [ ] the tool matrix covers rectangle, ellipse, arrow, pen, select/move, note, undo, submit
- [ ] video: upload a real clip, pause on a frame, draw a point and a range, scrub to another
      time (hidden by default; Ghosts shows it), check the timestamp groups and group seek, zoom
      the timeline, submit, check the agent feedback timecodes/frames/contact sheet, post a new
      clip and replay the previous round from History; leave the review and come back before
      submitting to prove the draft survives
- [ ] agent concurrency: settings save, branch shown in the rail, claim limits/branch locks green,
      and two claims run in separate worktrees
- [ ] every changed UI control was clicked by the e2e; every changed endpoint has a Rust test
- [ ] no uncaught page errors and no unexpected console errors
- [ ] `dev` and `main` required checks green; auto-merge only on a ready PR
- [ ] a 5-minute manual walk through the release image: draw one of each tool, submit, read the
      feedback, confirm/cancel, reopen, switch filters, resize a panel
- [ ] version bumped in `desktop/Cargo.toml`, `desktop/app/Cargo.toml`,
      `desktop/app/tauri.conf.json`; tag; deploy; `GET /health` on the deployed server

## Current gaps / next

- The browser e2e is one long script. Keep it deterministic; poll, never `sleep`.
- Visual checks are only as strong as their assertions: add a pixel or a DOM measure for every
  visual bug, not just a class check.
- Windows-only Tauri behaviour is compile-checked; installer/updater UI still needs the manual
  smoke.
- The e2e uses the disposable container; always rebuild `redline:local` after changing
  `desktop/ui/*` or the Rust code, otherwise it tests the previous binary.
