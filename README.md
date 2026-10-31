# RedLine

**Mark the screenshot — or the frame. The agent writes the fix.**

RedLine is a local visual review loop for humans and AI agents. An agent shows you a screen or a
short recording; you draw on it, pause the video at the moment that is wrong, and write what
should change. The agent reads that as structured data and comes back with the next round — in
the same review, with a new screenshot or clip.

No accounts, no cloud, no JS build step. The server is a single static binary; the UI is a few
plain files served from it.

```
agent ──creates a review──▶ you annotate ──submit──▶ agent works ──new round──▶ you again
```

## Quick start

### Desktop app (no server)

1. Download the installer from [Releases](https://github.com/erikibolya/redline/releases).
2. Launch it — the app embeds the server on `127.0.0.1:8788`; the tray icon is the app.
3. Install [OpenCode](https://opencode.ai) and `opencode auth login`.
4. Tray → **Choose agent repository…**, then **Start local agent**.
5. Tray → **Copy MCP config** and paste it into your OpenCode MCP settings.

Everything (reviews, screenshots, history) stays on your machine, under `%APPDATA%\redline` (or
the platform equivalent).

### Server (Docker / NAS)

```bash
docker run -d --name redline --restart unless-stopped \
  -p 8788:8788 -v /mnt/user/appdata/redline:/data \
  ghcr.io/erikibolya/redline:browser
docker logs redline | grep -i token   # the access token
```

Open `http://<host>:8788/?t=<token>`. Data lives in `/data`; the `:browser` image adds Chromium so
reviews can be captured from a URL server-side. Traefik labels and a compose stack with the agent
are in [`docs/DESKTOP.md`](docs/DESKTOP.md).

### Connect an agent

MCP over HTTP at `/mcp` (Bearer token):

```json
{
  "mcpServers": {
    "redline": {
      "type": "http",
      "url": "http://127.0.0.1:8788/mcp",
      "headers": { "Authorization": "Bearer <token>" }
    }
  }
}
```

Tools: `review_screenshot`, `review_video`, `capture_review`, `get_feedback`,
`wait_for_feedback`, `list_reviews`, `list_projects`, `progress`, `reply`, `post_round`,
`capture_round`, `resolve`, `handoff_status`.

## The loop

1. **An agent opens a review** — `review_screenshot` (an image), `review_video` (a clip) or
   `capture_review` (a URL).
2. **You mark it up** — rectangles, ellipses, arrows, freehand; every mark carries a note
   ("this button is too small") written in the panel, not on the canvas. On a video, pause on the
   frame (or mark a range) and the annotation keeps its timecode.
3. **Submit** — the canvas locks, the agent is notified through its own runtime (MCP polling or
   the webhook/bridge), and the conversation shows *received* → *working* with a live activity
   feed → the reply.
4. **The agent posts the next round** — `post_round` or `capture_round` replaces the canvas image
   in the *same* review. One task is one session, not a pile of them.
5. **Repeat or close** — History is a read-only viewer of every submitted round; drafts survive
   switching between reviews; **STOP** takes a submission back and kills a running agent.
6. **✓ Confirm decides** — a review run never opens a PR. The confirm dialog offers an optional
   handoff; if checked and an agent supports it, the integration performs its own
   post-confirmation action (the reference bridge opens/merges a PR) and reports `handoff_status`.
   The rail shows the integration's label/link (`merged`, `deployed`, …) or a retry; without a
   handoff, the decision is just recorded.

## Features

| | |
|---|---|
| **Projects = repositories** | the agent syncs the git repos it can see; the rail groups reviews by repo and the agent runs each review in that project's repo |
| **Rounds** | every submit is archived with its source screenshot or clip and annotations; the canvas switches between rounds |
| **Video** | upload a clip or let an agent post one; scrub and pause in the canvas, annotate a moment or range, and submit a contact sheet plus full-resolution frames |
| **Video rounds** | a new clip is a new round; the previous clip, frames and annotations replay read-only from History |
| **Notes, not text on the canvas** | numbers tie a shape on the canvas to a paragraph in the panel, `annotated.png` and `review.md` |
| **Conversation** | request, prompt, agent activity (`progress`), replies and state (`received`/`working`/`replied`), polled every 5 s |
| **Drafts** | unsent annotations and the composer survive switching sessions; quitting warns first |
| **Capture** | server-side Chromium screenshots a URL (`:browser` image) |
| **Auth** | one token; Bearer header, `?t=` link or HttpOnly cookie; cross-site requests refused |
| **Desktop** | Tauri v2 tray app, auto-update with stable and dev channels, multiple server profiles (Production/Dev/This computer) with a one-click switcher |

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `REDLINE_PORT` | `8788` | listen port |
| `REDLINE_DATA_DIR` | platform data dir | where sessions, token and config live |
| `REDLINE_TOKEN` | generated in `/data/token` | access token |
| `REDLINE_CHROME` | auto-detected | Chromium binary for capture |
| `REDLINE_WEBHOOK_URL` | — | POSTed on every submission, to wake an agent |
| `REDLINE_AGENT_REPO` | current dir | default repo for legacy projects without a repo binding |
| `REDLINE_AGENT_REPO_ROOT` | parent of `REDLINE_AGENT_REPO` | root that project repo keys are resolved against |
| `REDLINE_AGENT_OPENCODE_ROOT` | same as repo root | path prefix the OpenCode server uses for the same repos |
| `REDLINE_AGENT_ID` | `default` | agent name sent with the repo sync |
| `REDLINE_AGENT_SYNC` | `60` | seconds between repo-list refreshes |
| `REDLINE_AGENT_MODEL` | `deepseek/deepseek-flash` | OpenCode model for the bridge |
| `REDLINE_AGENT_OPENCODE_SERVER` | — | dispatch into an already running OpenCode |

CLI: `redline serve | open | list | get | wait | remove | token | mcp-config | health`.
Unattended agent: `redline-agent` (see [`docs/LOCAL-AGENT.md`](docs/LOCAL-AGENT.md)).

## Releases

- **Stable**: `v*` tags on `main`, published as GitHub Releases.
- **Dev**: every push to `dev` publishes a rolling prerelease; the desktop app can install it from
  the tray and revert to the last stable version.

## Development

```bash
# Rust (core, CLI, agent)
docker run --rm -v "$PWD":/src -w /src/desktop rust:1-bookworm \
  sh -c 'rustup component add rustfmt; cargo fmt --all; cargo test --workspace'

# Browser end-to-end (72 checks, real Chromium)
docker build -q -f desktop/Dockerfile.browser -t redline:local .
REDLINE_IMAGE=redline:local bash desktop/scripts/e2e.sh
```

Docs: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), [`docs/API.md`](docs/API.md),
[`docs/UI.md`](docs/UI.md), [`docs/DESKTOP.md`](docs/DESKTOP.md),
[`docs/LOCAL-AGENT.md`](docs/LOCAL-AGENT.md), [`PLAN.md`](PLAN.md).

MIT licensed.
