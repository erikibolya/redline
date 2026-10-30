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

## Agent concurrency (v0.22)

- Rust `tests/agent.rs`: global/project capacity, waiting frees a run slot but keeps the branch,
  one owner per branch, lease expiry, heartbeat/release ownership, persistence, settings
  validation.
- Rust `tests/api.rs`: settings round-trip and per-project overrides, claim/next, branch locks,
  `waiting` transitions, `next` skipping locked branches.
- `redline-agent` tests: a worktree is created on the declared branch and reused; two claims run
  in two separate worktrees (the bridge integration test starts a real server and a fake
  `opencode`); the prompt carries the branch.
- Browser e2e: the Agent settings dialog saves global + per-project values, and the rail shows the
  session's branch next to the relative time.

## Every UI control

- A test must **click** the control, not only call the method or press the shortcut.
- Assert the visible result (text, class, geometry, pixel) **and** the state/server result.
- Cover both directions: lock/unlock, confirm/reopen, cancel/reopen, select/deselect, open/close.
- Dialogs: every button plus Escape/cancel.
- Destructive actions: assert the confirmation and the state after it.
- If the control is visual, measure it. A border is not a separator just because the class exists.

## Bug-fix protocol

1. Reproduce the bug in the real UI (or the real API) and capture the failing state.
2. Write the smallest automated test that fails on the current code and passes after the fix.
3. Fix the root cause, not the symptom.
4. Run the **full** suite, not only the new test.
5. Add a §7 lesson to `AGENTS.md` if the bug exposed a process gap.
6. Keep the test forever; it is the regression fence.

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
