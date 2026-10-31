# Agent instructions — redline

Read this before touching anything. It is the handoff document: what redline is, where it runs,
how to change and ship it, and the traps that have already cost time.

---

## 1. What this is, and where it stands

`redline` is a local visual review queue for human ↔ AI collaboration: an agent produces a
screenshot, a human draws on it in a browser or the desktop app, the agent reads the result as
JSON + Markdown + an annotated PNG.

**Current release: v0.24.0.** Live in production on holly:

| Thing | Value |
|---|---|
| Human UI | `https://redline.ibolya.cz` (Traefik → container, Let's Encrypt via cloudflare resolver) |
| Agent endpoint | `https://redline.ibolya.cz/mcp` (or `http://127.0.0.1:8788/mcp` on holly) |
| Container | `redline`, image `ghcr.io/erikibolya/redline:browser`, no published port |
| Data | `/mnt/user/appdata/redline` → `/data` (sessions in `/data/sessions`) |
| Token | `/mnt/user/appdata/redline/token` (also printed by `docker logs redline | grep -i token`) |
| Dev server | `https://dev.redline.ibolya.cz`, stack `redline-dev`, data `/mnt/user/appdata/redline-dev` |
| Repo | `https://github.com/erikibolya/redline` (public, MIT), branch `main` |

The Windows desktop app (`desktop/app`) is a Tauri v2 shell: tray + menu bar + a webview pointed at
the server. It is a *client*; all logic is server-side. It auto-updates from GitHub Releases.

## 2. Environment you are actually in

You are in a **container on holly (Unraid)** with, by explicit decision of the owner:

- `/var/run/docker.sock` mounted → full control of every container;
- `/mnt/user` mounted read-write → the whole array.

That is **root-equivalent**. The global rules in `~/.config/opencode/AGENTS.md` require asking the
human before anything destructive (deleting data, touching containers other than `redline`,
changing other services, `/boot`, rewriting git history). Read them; they are loaded automatically.

Environment traps that have already bitten:

- **Docker runs on the host, not in this container.** Bind-mount sources must be *host* paths
  (`/mnt/user/...`). Mounting `/tmp/opencode/...` silently creates an empty directory on the host.
- **The sandbox home is ephemeral.** `~`, `~/.cargo`, `/tmp` vanish when the container is
  recreated. Tooling lives on the array:
  - `gh` → `/mnt/user/appdata/agent-tools/bin/gh`
  - gh credentials → `GH_CONFIG_DIR=/mnt/user/appdata/agent-tools/gh`
  - git credential helper is configured globally to use that path.
  Export both before pushing: `export GH_CONFIG_DIR=/mnt/user/appdata/agent-tools/gh PATH=/mnt/user/appdata/agent-tools/bin:$PATH`
- **No Rust toolchain on the host.** Build with Docker:
  `docker run --rm -v /mnt/user/Development/repos/redline:/src -w /src/desktop rust:1-bookworm cargo test --workspace`
- **Signing/updater keys**: `/mnt/user/appdata/redline-secrets/updater.key` (private, 600) and
  `.pub`. The private key is also a GitHub secret. Losing it means existing installs can never
  update again.

## 3. Repository layout

```
desktop/
  Cargo.toml              workspace: crates + no app (the app is its own workspace, needs a GUI)
  crates/redline-core/    store, api, auth, mcp, capture, diff, client, paths — no GUI deps
  crates/redline-server/  `redline` binary: serve / open / list / get / wait / remove /
                          token / mcp-config / health
  app/                    Tauri v2 shell (tray, menu, updater, setup wizard)
  ui/                     app.css, app.js, index.html (editor), list.html, setup.html
  scripts/                e2e.mjs (browser test), e2e.sh, add-signing-cert.sh
  Dockerfile              scratch + static musl binary (~4 MB)
  Dockerfile.browser      debian-slim + chromium (~768 MB) — needed for capture
  install-unraid.sh       idempotent install/upgrade of the container
  docker-compose.example.yml
docs/                     DESKTOP.md (ADR), UI.md (UI plan/status), API.md, ARCHITECTURE.md,
                          WORKFLOW.md, TESTING.md (tool matrix + release gate),
                          INTEGRATIONS.md (MCP contract for review/handoff agents), PLAN.md at the root
static/, redline.py       v0.1 Python implementation, frozen; do not add features to it
```

## 4. Build, test, ship

```bash
# Rust (host has no toolchain: use the rust image)
docker run --rm -v /mnt/user/Development/repos/redline:/src -w /src/desktop rust:1-bookworm \
  sh -c 'rustup component add rustfmt; cargo fmt --all; cargo test --workspace'

# Browser end-to-end (draws every tool; see docs/TESTING.md and PLAN.md for the current count)
docker build -q -f desktop/Dockerfile.browser -t redline:local .
REDLINE_IMAGE=redline:local bash desktop/scripts/e2e.sh     # prints ok/FAIL per check
# Release gate: docs/TESTING.md — tool matrix, feature protocol and the checklist.

# Ship a feature (dev is protected; never push it directly)
# A review run is not a handoff: push the branch and reply, do NOT open a PR or merge.
# The human's ✓ Confirm feature can queue a generic handoff; the bridge runs the handoff prompt:
git push origin feat/my-feature
feature-ready "what changed and why"   # only in the handoff run, after the confirmation
# draft PR -> ready -> required CI -> auto-merge -> dev build

# Release (only after an explicit yes; main is protected)
gh pr create --base main --head dev --title "release: vX.Y.Z" --body "Release notes"
# wait for the required checks, merge the PR, then:
git checkout main && git pull --ff-only
git tag -a vX.Y.Z -m "..." && git push origin vX.Y.Z   # release: installer, CLI, linux binary, images
REDLINE_IMAGE=ghcr.io/erikibolya/redline:browser REDLINE_DOMAIN=redline.ibolya.cz \
  bash desktop/install-unraid.sh                        # deploy to holly
```

Version bumps touch three files: `desktop/Cargo.toml`, `desktop/app/Cargo.toml`,
`desktop/app/tauri.conf.json`.

CI secrets: `TAURI_SIGNING_PRIVATE_KEY` (updater), optional `WINDOWS_CERT_PFX` +
`WINDOWS_CERT_PASSWORD` (Authenticode — not set; builds are unsigned and SmartScreen warns).

## 5. Deployment shape

The installer adds Traefik labels (copied from the other stacks on this host):

```
--network traefik_network
traefik.http.routers.redline.rule=Host(`redline.ibolya.cz`)
traefik.http.routers.redline.entrypoints=websecure
traefik.http.routers.redline.tls.certresolver=cloudflare
traefik.http.services.redline.loadbalancer.server.port=8788
```

When `REDLINE_DOMAIN` is set the container publishes **no** port and runs with
`--public-url https://$DOMAIN`. `REDLINE_PUBLISH_PORT=1` keeps a LAN port (don't, unless needed).

On holly the container is owned by a Compose project (`/boot/config/plugins/compose.manager/projects/redline`)
together with `redline-agent`; `install-unraid.sh` must not be run there. See `docs/DESKTOP.md` §
"Compose deployment".

The dev server is its own stack (`/boot/config/plugins/compose.manager/projects/redline-dev`):
it follows `ghcr.io/erikibolya/redline:browser-dev` (published by every dev workflow run) and a
label-scoped Watchtower (`redline-dev-watchtower`, 5-minute interval) recreates it. The Watchtower
Swagger UI is `https://watchtower.ibolya.cz` (basic auth; credentials in
`/mnt/user/appdata/watchtower/credentials.txt`), routed by the Traefik file provider
`/mnt/user/ProgramDataSSD/Traefik/data/dynamic/watchtower.yaml`. Do not enable Watchtower's
`update` endpoint there: it disables the periodic scheduler.

Env knobs: `REDLINE_DOMAIN`, `REDLINE_IMAGE`, `REDLINE_PORT`, `REDLINE_TOKEN`, `REDLINE_DATA`,
`REDLINE_PUBLISH_PORT`, `REDLINE_CHROME` (browser binary for capture).
Rust side: `REDLINE_PORT`, `REDLINE_DATA_DIR`, `REDLINE_TOKEN`, `REDLINE_CHROME`, `REDLINE_HOST`.

## 6. Architecture in one screen

- Desktop profiles: `config.json` holds `servers[]` + `active`; the local server serves
  `/servers`, and switching only navigates the webview to another origin. All profiles/tokens
  stay; never treat "switch" as "disconnect".
- `redline-agent` — unattended bridge: watch submitted reviews → resolve the project's repo →
  `opencode run` → progress/reply/resolve. It syncs the git repos it can see (`POST
  /api/agent/repos`) and runs each review in its project's repo. Also embedded in the desktop app
  (tray → *Start local agent*); see `docs/LOCAL-AGENT.md`.
- `redline-core` — session folder store (`meta.json`, `annotations.json`, `review.md`,
  `annotated.png`, `frames/`, `history/`), project folders (`sessions/projects/<slug>/`, v0.10;
  optional `repo` key + `agent`), axum HTTP API, bearer/cookie auth, MCP JSON-RPC, headless
  capture (shells out to Chromium), annotation diffing, loopback HTTP client.
- Auth: token from `/data/token`; `Authorization: Bearer`, `redline_token` cookie (set by
  `/s/<id>?t=…` → 307), or `?t=`. `Sec-Fetch-Site: cross-site` is refused.
- MCP tools: `review_screenshot`, `review_video`, `capture_review`, `get_feedback`, `wait_for_feedback`,
  `list_reviews`, `list_projects`, `reply`, `progress`, `post_round`, `capture_round`, `resolve`,
  `handoff_status`. Resources: `redline://session/<id>/{annotated.png,review.md}`.
- Agent state: `get_feedback` stamps `seen_by_agent_at` and sets `agent_state=received`;
  `progress` appends to `agent-events.jsonl` and sets `working`; `reply` sets `replied`.
  The editor polls meta + thread + events every 5 s and renders them as a conversation.
- Agent concurrency (v0.21.0): `/data/agent-settings.json` + `/data/agent-claims.json`; defaults
  global 2, per-project 1, lease 120 s. A claim is `running` or `waiting`; waiting keeps the
  branch lock but frees the run slot. The branch key is `meta.branch` (fallback `session/<id>`);
  the bridge runs `REDLINE_AGENT_WORKERS` (default 4) claim workers, each in its own worktree under
  `<repo_root>/.redline-worktrees/`, with heartbeats and one owner per branch. The OpenCode
  server must see the worktree path just like the repo itself.
- UI: no framework, no build step. `app.js` holds the shell (rail, canvas engine, inspector);
  `app.css` the design tokens incl. light theme. Served from the binary at `/static/*`.
- Editor model: annotations = paint order; selection is a `Set`; viewport is a transform
  (`scale/x/y`) applied in `render()`, so zoom is crisp and pointer mapping is `(screen - view)/scale`.
  A video session draws `<video>` into the same canvas via `loadMedia()`; `annotationOpacity()`
  hides other moments by default (the Ghosts toggle shows them) and `videoWindow` drives the
  zoomable timeline; the inspector groups video rows by timestamp. Unsent work is a per-session
  draft in `localStorage` (`Drafts`), flushed on `pagehide` and cleared by submit. A video submit
  captures `frames/` and a contact sheet; a new clip is a new round whose predecessor lives under
  `history/<stamp>/video.*` and replays read-only.

## 7. Lessons already paid for (do not relearn)

- **Pointer capture in the stage swallows clicks** on children (options bar). Guard:
  `if (event.target.closest(".options, .text-editor, .viewing-bar, .video-bar")) return;`.
  The video bar was added inside the stage and its play button did nothing until it joined that
  guard; the timeline wheel also needs `stopPropagation()` or it zooms the canvas while zooming
  the ruler.
- **A media element ignores `no-store` when the URL stays the same.** Replacing a review's clip
  rewrote `video.webm`, but `<video src="/sessions/<id>/media?v=video.webm">` kept playing the
  previous round. Cache-bust the live URL per load (`&r=<timestamp>`), not just with the file
  name; the history URL already carries the unique round stamp.
- **A MediaRecorder WebM has no duration until it is seeked.** `video.duration` is `Infinity`
  straight after metadata, which collapses the timeline to a zero-length window. Seek past the
  buffered end once (`currentTime = 1e101`, wait for `durationchange`), then reset to 0; the
  upload's poster/duration must wait for the resolved value too. A 250 MB cap is not a length
  cap: the server also rejects a declared duration over five minutes.
- **A Map in one document is not a save.** Drafts lived in `Editor.drafts`, so switching reviews
  in the rail kept them but clicking a review on the dashboard (or reloading, or the desktop
  webview navigating) started a new document and lost every shape. Persist per session in
  `localStorage`, save while editing and flush on `pagehide`/hidden; clear on submit, and invalidate
  a draft when the media fingerprint changes so old coordinates cannot land on a new round. Test
  the full navigation, not only `openSession`.
- **A selection is not a time filter.** `annotationOpacity` returned `1` for selected rows, and a
  freshly drawn shape stays selected — so an annotation from another moment remained fully
  visible until a new drawing stole the selection. Selection draws an outline; the playhead (and
  the Ghosts toggle) decides whether the shape is visible. Test the selected case, not only an
  unselected ghost.
- **A flat list is not a timeline.** Video annotations are rows with timecodes; a long round read
  as an undifferentiated stack and the moment was only in tiny text. Group the inspector by
  timestamp, make the group header a seek control, mark the group under the playhead, and keep
  the group shape in the read-only history viewer. Test the same-moment group (one header) and
  the range that stays active.
- **Text entry**: `pointerdown` must `preventDefault()` (else mousedown focus-steals and the empty
  editor commits), focus synchronously, and `commit()` must be idempotent (removal fires `blur`).
- **`RunEvent::ExitRequested` is vetoed** to keep the tray app alive; a quit/restart must set a
  flag first or it silently does nothing.
- **Bundle builds need the updater key**: `createUpdaterArtifacts` fails without
  `TAURI_SIGNING_PRIVATE_KEY`, in `desktop.yml` too.
- **Release job ordering**: the Linux-binary job needs `needs: windows` (the release is created by
  the Windows job).
- **A prerelease of the current stable is older, not newer.** Dev builds were versioned
  `0.23.0-dev.N` while stable was already `0.23.0`, so the updater correctly refused to install
  one; "Install latest dev build…" looked broken. `DEV_BASE_VERSION` must be the *next* minor
  (now 0.24.0); the dev workflow now fails if the base does not sort above the latest stable.
- **Chromium binds its debugger to loopback**: run the puppeteer container with
  `--network container:redline-e2e`.
- **In tests, poll; never `sleep`** for a submit to land (flaked twice).
- **`--public-url` was silently ignored** once because the flag was missing from `VALUE_FLAGS`;
  unknown `--flags` now error instead of being swallowed.
- **`docker run --rm` containers exit** when the test closes the browser; each e2e run needs a
  fresh container (the script does this).
- **Test the UI by clicking**, not only via keyboard: dead tool buttons shipped because the e2e
  used shortcuts exclusively.
- **A file nobody reads is not a feature.** `reply` appended `agent-thread.jsonl` and no route or
  screen read it, so a human who submitted a review heard silence for three releases. The agent →
  human half of the loop needs a route *and* a UI, just like the human → agent half.
- **The start page printed the token** (`/api/config` returns it verbatim). Any screenshot of `/`
  leaked it — including one made by redline's own capture. Mask secrets at render time, keep the
  copy button working.
- **A filename is not a preview.** The New review drop zone said `image.png — 285 KB` for a pasted
  screenshot, so there was no way to tell *which* screen you had on the clipboard. Attach the
  picture itself (`objectURL`, revoked on replace); the name and size become a caption. Cap the
  height and let the modal scroll — a taller preview must not push *Create review* off-screen.
- **Never slice a clock out of an ISO stamp.** The inspector did `iso.slice(11, 16)`, which is UTC:
  a review submitted at 13:03 in Prague was shown to every reader as 12:03. Parse (`new Date`) and
  format with the browser's locale — `clockTime()` / `stampTime()` in `app.js`. Capture screenshots
  inherit the *container's* timezone (usually UTC), so a "fixed clocks" screenshot looks unfixed
  unless you capture from a browser started with `TZ=Europe/Prague`.
- **Switching a server is not logging out.** The desktop app keeps every profile and token in
  `config.json`; `/servers` only changes `active` and navigates to the chosen server. Cookies are
  per origin, so the other servers stay signed in. Switching back to *This computer* must also
  preserve profiles.
- **A project is not a repository.** RedLine projects began as labels; a label does not tell the
  agent where to work. Repo projects now store a **relative** key (`redline`, `ActiUnit/Au.Net`)
  plus an agent id. Never store an absolute mount path: the OpenCode server, the agent container
  and the Windows app can see the same repo through different roots. Resolve keys under
  `REDLINE_AGENT_REPO_ROOT`, canonicalize and refuse escapes/missing paths — a silent fallback to
  the default repo is worse than a visible error.
- **A branch name is not a ref name.** The bridge chose the worktree's start point with a short
  ref (`dev`, `main`, `origin/dev`, …). This repository also carries a tag `dev`, so git answered
  `warning: refname 'dev' is ambiguous` and then `fatal: ambiguous object name: 'dev'`, and the
  review never started (*I cannot start this review*). The same lookup also just **picks the tag**
  when it succeeds, so a feature can silently start from the wrong commit. Resolve bases by full
  refname (`refs/heads/dev`, `refs/remotes/origin/dev`) and peel to `^{commit}`; keep the short
  name only where git demands a branch name (`worktree add <path> <branch>`). Tags are not
  branches: a stray `dev` tag in a repo must never change where work starts.
- **A session id is not a branch name.** The bridge's fallback when nobody declared a branch was
  `session/<id>`, and the New review dialog never asked — so a human-made review ran on a
  timestamp branch (the rail showed `session/20261030-…`) that an agent cannot push or merge as a
  feature. A review now gets `feat/<title-slug>-<id-tail>` at creation, the dialog offers a
  *Feature branch* field and the header renames it (`PUT /api/sessions/{id}/branch`, refused while
  the round is submitted because the worktree already exists); v0.21's persisted fallback is read
  back as the `feat/…` name and `Meta::work_key()` derives it instead of inventing a session
  branch. A name that only says *when* is not a name for what is being built.
- **The OpenCode project API is experimental.** `/api/project` is useful for display names and
  active projects, but discovery must still scan the repo root for `.git`; dedupe the same repo
  seen through two mounts (`/mnt/user/...` and `/mnt/c/...`).
- **A push is not a done signal.** A feature branch becomes a PR only when the agent runs
  `feature-ready`; auto-merge then waits for the required checks. Merging on
  every push would ship checkpoints and half-finished work. Do not open a non-draft PR while the
  feature is still being worked on.
- **An empty-state class outlives the empty state.** `#history-list` was declared once in
  `index.html` as `inspector-empty` (22 px padding, centred text) and never lost the class when
  rows were injected, so a populated list still carried the empty state's dead space. Toggle the
  class with the content — a populated list needs its own layout class.
- **A rounded border is not a separator.** `.ann-row + .ann-row` used `border-top-color` on a
  rounded row to draw the divider, so the line only appeared between the two rounded corners and
  the annotation list read as a stack of open frames. A divider spans the row: square the top
  corners (or use a pseudo-element), and assert it in the e2e — a "broken border" is invisible to
  every DOM test that does not measure the row.
- **A shared list class is not a shared shape.** The history rows reuse `.ann-row`, so the
  divider's squared top corners also reached them — but history rows are 4 px apart and each owns
  its whole border, so every round below the first looked chopped off. A style that exists for one
  list must be scoped to it (or restored on the other). Assert the radius of *every* row, not
  just the first.
- **A highlight is not the only way to mark a live row.** The live history row tinted its fill and
  border blue, so it read as a different kind of card; the human asked for the same card as every
  other round with only the glyph green. Colour the smallest marker that has to stand out, and keep
  selection (which round you are viewing) a separate signal. Then keep *both* signals working:
  dropping the outline from the live row too left `Current` looking like an idle card, and the
  human could not tell which round the canvas was showing (2026-10-31). The live *default* is a
  plain card; selection is the accent outline on whichever row is shown, live or archived. Test
  the unselected live row against a sibling's look and the selected one against the accent.

- **A centred icon is not an aligned icon.** A history row is two lines (title + timestamp) and
  `.ann-row` centres its glyph on the whole row, so the round number and the live bullet floated
  *between* "Round N" and the date instead of sitting beside the title (two annotations on the same
  column). Align the glyph to the title's line box, not to the card. And a centred *em box* is not a
  centred digit: digits have no descender, so the numeral sat ~2 px high inside its square — pad the
  top and assert the ink centre, not just the element's centre.

- **A grid column resize is not a canvas resize.** Moving `--rail` or `--inspector` reflows the
  layout, but the `<canvas>` bitmap keeps its old pixel size until `resize()` runs — so the stage
  is measured again on every pointermove of a divider drag. Sizes belong in CSS variables plus
  `localStorage`, never only in the DOM element's inline width.
- **A lock flag is not a lock.** The submitted lock set `Editor.locked` and guarded the canvas
  pointer plus `setTool`, but Delete/arrow keys, undo/redo, the inspector's align/layer/group
  buttons and the note field wrote anyway — the e2e only asserted `window.redline.locked`, so it
  passed while the human could still edit a submitted round. Enforce a lock at the single
  mutation choke point (`mutate`, and the few paths that bypass it), and test the *refusal*: draw,
  delete, nudge and click the write tools, then assert nothing changed. A control that looks armed
  but silently refuses clicks reads as a broken lock; disable it visibly.
- **A tool button is not the tool.** The e2e clicked the pen button and asserted the tool was
  selected, so the freehand badge/row regression survived: `renderInspector` read
  `annotation.start.x`, but a pen is `{type:"pen", points:[…]}`. The `TypeError` aborted `mutate`
  before `render()`, leaving the live stroke visible with no number and no inspector row. Draw
  with every tool, assert the annotation, its inspector row/number and its badge pixel, then
  submit it. `docs/TESTING.md` § "The tool matrix" makes this non-negotiable.
- **A spinner is a promise.** `progress` set `agent_state=working` unconditionally, so the agent's
  final narration line — written after `post_round` had handed the review back — re-armed the
  indicator. The card pulsed "Working" on a review the human already had, with a green Submit
  button beside it, and stayed like that until the next submit. A state flag must be derived from
  the round (`status == submitted`), not set by the last writer; the server refuses to revive a
  finished round, the UI checks both fields, and the card always shows the age of the last step so
  "is it really working?" has an answer that is not animation.
- **A reply is not the finish line.** The inverse trap: `reply` set `agent_state=replied`, the card
  called that "Run finished", yet the review was still `submitted`, the canvas was locked and the
  red STOP button was armed — and the agent kept working for minutes before resolving (2026-10-30).
  Only a round that left the agent's hands (`post_round` → open, or `resolve`) is finished; while
  submitted, say the agent replied and the run has not finished, and let a later `progress` (real
  evidence the process is alive) turn the indicator back to "Working". Never print "finished" and
  "STOP" in the same breath.
- **A failed run is not a reply, and a locked round is not a queue.** The bridge's failure path
  called `reply` like any answer, so a run that never started (a rejected OpenCode password, a
  broken repo) left the review `submitted` behind a *replied* label: the canvas locked, the
  working indicator gone, and nothing telling the human that nobody was coming back — *"I don't
  know if the task is being worked on or not… is it a bug?"* (2026-10-31). A failure is its own
  state: `reply {state:"failed"}` reopens the round (annotations and history stay) so the human can
  fix it and resubmit, and the rail/card say **agent failed** in red. And the inverse of the
  previous lesson: `agent_state` records who spoke last, not whether a process is alive, so the
  rail must take **working** from a live claim (lease + heartbeat, exposed as `agent_running`)
  as well — a mid-run reply must not erase the only sign of activity.
- **A live tail is not a conversation.** The agent's own lines are what the human asked for
  ("realtime output… like a regular opencode window"), but every line in the timeline buries the
  messages that need reading. Stream them into the same event log under `state: "log"` (throttled,
  ANSI stripped, bounded), render them only behind an explicit **output** toggle that remembers its
  state, and never render a `log` event as a conversation message.
- **A delete button is not a cancel button, and a UI badge is not a signal.** The rail's ✕ removed
  the review folder, so "cancel this feature" had no non-destructive action; the ✓ only set
  `resolved`, which told an agent nothing about shipping. Feature decisions now have their own
  additive stamps (`feature_confirmed_at` / `feature_cancelled_at`) exposed as `feature_state` in
  `GET /api/sessions`, `get_feedback`, `wait_for_feedback` and `list_reviews`; the ✓/✕ open
  explicit dialogs, ✕ offers Keep working / Cancel feature / Delete files… (Reopen + Delete once
  decided), and RedLine never touches git — the agent owns the PR/merge workflow. Cancel keeps
  the files and Delete is a deliberate extra choice, not a reflex click; make every human
  decision readable by the agent, not just visible in the UI.
- **A read-only viewer is not a dead image.** `viewRound` set `viewing`, and `pointerdown`
  returned early on it, so a historical screenshot could not be dragged, pinched or panned —
  read-only means "no edits", not "no navigation". Keep pan/zoom on their own guard and turn the
  main button into a drag (grab/grabbing cursor) when there is nothing to select. The "back to
  current" pill lives *inside* the stage, so it needs the same
  `closest(".options, .text-editor, .viewing-bar")` guard as the toolbar or pointer capture
  swallows its click. Space-pan required `event.target === document.body`; after clicking a
  history row the focus stays on the row, so the gesture silently died — arm space while the
  pointer hovers the stage, never while typing. Test the real gestures (drag, wheel, ctrl+wheel,
  space+drag under row focus), not only `viewing`/`locked` flags.
- **A queue with no listener is a silent dead end.** A submitted review with no bridge behind it
  looked exactly like one the agent was about to pick up; the human had no way to tell "nobody is
  listening" from "be patient". Record presence from the bridge's existing periodic repo sync
  (and every `next`/claim call), expose an online window (180 s), and put it where the human
  already looks: a header chip with words, plus a rail warning when the count is zero. Keep it
  non-blocking — never trap work behind a bridge that is down — but never let a dead queue look
  alive.
- **A permanent setup block is not a status.** The dashboard kept the MCP config on screen after
  the agent was already connected, so the most prominent thing on `/` was setup the human had
  finished. Move configuration into a dialog opened from the status chip, and leave only an
  invitation while the condition that needs it (no bridge online) is true. Test the dialog from
  both pages, the masked token, and the invitation appearing/disappearing with presence.
- **A poll that updates the rail does not update the gallery.** The rail already re-read the
  session list every 8 s, but the `/` gallery was painted only at boot and on a back-forward
  restore. A review cancelled elsewhere (or a restore whose refresh did not land) kept its card
  for as long as the dashboard stayed open; the e2e saw it as an intermittent failure of the
  cancel flow. Treat every derived view as a subscriber of the one poll — hand the fresh list to
  the gallery — and test the poll, not only a reload. A list that is right in the rail and stale
  beside it is still wrong.
- **A poll that repaints one field leaves the others stale.** The rail's 8 s poll redrew only when
  the *working* set changed, so a run that failed without ever being "working" (the bridge reports
  `thinking`, then `failed`) kept its row saying **waiting for agent** — the one badge that had to
  explain the dead end was the one that never repainted, and CI's e2e caught it while the review
  was being handed off. Compare a fingerprint of the fields a row actually paints (status,
  `agent_state`, `agent_running`, counts, feature/handoff state) rather than a proxy for them, and
  keep the "unchanged queue does not rebuild the list" guarantee with that fingerprint.
- **Hiding a list is not emptying it.** When the last pending review was cancelled, the dashboard
  set `#gallery.hidden = true` and left the old card in the DOM. The screen looked right, but
  `querySelector` still found the card, so the e2e's "cancelled review is gone" check failed
  intermittently depending on whether another pending review kept the gallery visible. Clear the
  container when you hide it; a visual empty state and an empty DOM are two different claims.
- **A local page's back link is not a session link.** The Servers and Setup pages are served by
  the desktop app's local server, but the window may be pointed at a remote profile. Their
  `href="/"` returned to the local dashboard, which has no reviews and no agent presence — the
  human saw "No agent connected" and had to switch servers twice to recover. Read the active
  profile (`active_url`) from the local API and follow that origin; test the remote and local
  cases, not only the headless page.
- **A submission is not automatically a new round.** Every `POST /annotations` archived its own
  copy, so *STOP* → *Submit again* with nothing changed left the same annotations and the same
  PNG twice in History, and a session the human thought of as "one task" read as two (2026-10-30).
  A round is what the human *changed*: collapse a run of byte-identical copies to its newest entry
  at the one place the list is built (`Store::history`), so already-duplicated sessions heal too,
  and let a round that really changed keep its own row. Test the collapsed count *and* the changed
  one.
- **A current round is not also a history row.** After the agent resolved a review, History showed
  the last submission twice — as *Round N* and again as the live *Current* row — because `current`
  was derived from `status == submitted`, while the canvas keeps the round after resolve/reopen
  (2026-10-30). Derive "current" from what the canvas actually shows (saved annotations + picture
  equal the newest archived row), let *Current* appear only when the canvas differs from every
  round, and test the resolved case *and* the agent-posted-picture case that still needs it.
- **A confirmation is permission, not a status label.** The review prompt let agents follow the
  repo's shipping rules during a review run, so overnight every review was already opened and
  merged into `dev` before the human woke up; by morning there was nothing left to confirm
  (2026-10-31). A review run must push the branch and reply, never open a PR or merge. The human's
  ✓ optionally queues a **generic handoff** (`handoff.status=requested`); the bridge advertises
  `jobs: ["review","handoff"]` on `/api/agent/next`, claims it after the review claim is waiting or
  released, and runs a prompt that explicitly grants the post-confirmation action. The reference
  git bridge opens/merges the PR and reports `handoff_status` with `state=done`, `label=merged`,
  the PR `url` and a `message`; the rail shows that label + link or `handoff failed ↻`. Another
  integration can use the same job for a deploy/publish, or confirm without a handoff. An old
  bridge that sends no `jobs` is never handed a handoff job. Test the review prompt *forbids*
  handing off, the confirm → `next(job=handoff)` path, the MCP report, and the rail badge; a
  status label that merely says "confirmed" is not a handoff.

## 8. Working rules

- Ask before destructive actions (see global AGENTS.md). Creating/updating the `redline` container
  is normal work; say what is lost first if a review is in flight.
- **`dev` and `main` are protected.** Do not merge or push either directly. Push a feature
  branch; when the work is done run `feature-ready "summary"` (global agent tool; the repo copy
  still works). It creates the ready PR and auto-merge lands it in `dev`. A release is a PR from
  `dev` to `main`, merged after the required checks and an explicit human yes; tag afterwards.
  See `docs/AUTO-MERGE.md`.
- Do not hand-edit `annotations.json`, `meta.json`, `review.md` or `annotated.png` in a session.
- Never commit screenshots, sessions, tokens or keys. `sessions/` and `target/` are gitignored.
- Keep the UI dependency-free (vanilla JS/CSS, served from the binary).
- Keep the API additive; breaking changes need a migration note in `CHANGELOG.md`.
- Update `docs/UI.md` (UI), `docs/DESKTOP.md` (desktop/deploy), `PLAN.md` when behaviour changes.
- **Every bug fix starts with a failing test.** Reproduce the bug, add the smallest automated
  test that fails on the old code, fix the root cause, then run the full suite. Keep the test as
  the regression fence.
- **Every feature ships with automated coverage of its user path** — click the control, assert the
  visible result and the persisted state. New drawing tools must be added to the tool matrix in
  `docs/TESTING.md` and actually drawn in the e2e; clicking the button is not a test.
- Run Rust tests **and** the browser e2e before tagging, against the release image, and walk the
  release checklist in `docs/TESTING.md`. If a check is missing, add it before tagging.

## 9. Common recipes

```bash
# health / version
curl -s https://redline.ibolya.cz/health; docker exec redline /redline --version

# create a review from a URL (agent path, server-side capture)
TOKEN=$(cat /mnt/user/appdata/redline/token)
curl -s -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"url":"http://localhost:5016/button","title":"Button / default","note":"hover + disabled"}' \
  https://redline.ibolya.cz/api/capture

# read feedback (what an agent sees)
curl -s -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_feedback","arguments":{"session_id":"<id>"}}}' \
  https://redline.ibolya.cz/mcp

# agent repo sync (what redline-agent does on startup and every 60 s)
curl -s -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"agent_id":"holly","repos":[{"key":"redline","name":"redline"}]}' \
  https://redline.ibolya.cz/api/agent/repos

# session lifecycle
curl -X POST -H "Authorization: Bearer $TOKEN" https://redline.ibolya.cz/api/sessions/<id>/resolve
curl -X POST -H "Authorization: Bearer $TOKEN" https://redline.ibolya.cz/api/sessions/<id>/reopen
curl -X DELETE -H "Authorization: Bearer $TOKEN" https://redline.ibolya.cz/api/sessions/<id>

# mark a feature ready (from its branch; no dirty files)
feature-ready "what changed and why"

# rebuild/redeploy
REDLINE_IMAGE=ghcr.io/erikibolya/redline:browser REDLINE_DOMAIN=redline.ibolya.cz bash desktop/install-unraid.sh
```

## 10. Known limitations / next

No code signing (SmartScreen warns), no macOS/Linux desktop bundles, no touch toolbar, no
multi-select across groups beyond membership, and the `/` page is still a start screen rather than
a gallery. Since v0.8 text notes live in the inspector, not on the canvas — there is nothing to
size or style, and legacy text annotations are read-only history.

Repo routing is single-agent: all agents watch the same queue, so run one `redline-agent` per
RedLine server for now. Project records already carry `agent`, and repo keys are portable, so a
claim/filter router can be added without migrating data.

See `PLAN.md` § "Next" for the ordered list.

## 11. If you are the next session

1. `git pull`, `docker exec redline /redline --version` — confirm what is actually deployed.
2. Read `PLAN.md` § Next and `docs/UI.md` for the UI state.
3. Reproduce the bug or implement the feature **with the e2e open in another window**; then ship
   with a version bump, a tag, and a redeploy.
4. Leave this file better than you found it — especially § 7.
