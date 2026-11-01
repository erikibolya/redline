# RedLine without a server

RedLine is two independent halves:

- the **server + UI** (a Docker container, or the embedded server in the desktop app),
- the **agent runtime** (OpenCode, wherever it runs).

They talk over MCP for pull (`get_feedback`, `wait_for_feedback`) and over `redline-agent` for
push (a submission becomes an agent run). A user with no home server runs both on one PC.

## Two topologies

**Server on a NAS (the author's setup)** — repositories live there, agents connect to it:

```
desktop OpenCode ──▶ OpenCode server on the NAS
                          ├─ repositories
                          └─ redline (Docker)  ◀── https://redline.example (LAN/VPN)
```

**Everything on one Windows PC** — the desktop app *is* the server:

```
RedLine desktop app ──▶ embedded server 127.0.0.1:8788
      ▲                          ▲
   browser                  OpenCode (MCP at /mcp)
                                   │
                              local repository
```

No Docker, no domain, no NAS. The data lives in `%APPDATA%\redline`, the token is generated on
first start.

## Desktop app (local agent)

The app always starts its embedded server, so a single install is enough:

1. Install RedLine from [Releases](https://github.com/erikibolya/redline/releases) and OpenCode.
2. `opencode auth login` (one provider is enough).
3. Tray menu → **Choose agent repository…** → pick the project folder.
4. Tray menu → **Start local agent**.
5. In the app: **Copy MCP config** and paste it into your OpenCode MCP settings.
6. Submit a review. It shows `received`, then `working` with activity, then the reply.

The agent uses the repository's own `AGENTS.md` and the user's OpenCode permissions; it can run
tests, build and — if the review asks for a visual result — deploy, but it never rewrites
published git history.

### Settings

`agent.json` next to `config.json` (created when you pick a repository):

```json
{ "repo": "C:\\src\\app", "model": "deepseek/deepseek-flash", "opencode_server": "" }
```

- `model` — any OpenCode model id (`opencode models`).
- `opencode_server` — set to e.g. `http://127.0.0.1:4096` to dispatch into the OpenCode that is
  already running instead of starting a private one.

The local agent treats the **parent of the chosen repository** as its repo root, scans it for git
repos and syncs them to RedLine as projects. Choosing `C:\src\app` therefore offers
`app`, sibling repos and nested repos in the New review select. Reviews in a repo project run with
that project's path; legacy label-only projects use the chosen repository.

## Headless (server-side)

Download `redline-agent` from Releases or run the container:

```bash
REDLINE_URL=https://redline.example \
REDLINE_TOKEN=… \
REDLINE_AGENT_REPO=/repos/app \
REDLINE_AGENT_REPO_ROOT=/repos \
REDLINE_AGENT_ID=holly \
REDLINE_AGENT_WORKERS=4 \
REDLINE_AGENT_IDLE_TIMEOUT=900 \
REDLINE_AGENT_TIMEOUT=21600 \
redline-agent
```

`REDLINE_AGENT_IDLE_TIMEOUT` (default 15 min) kills a run that has produced no output; a long
build that keeps printing progress survives. `REDLINE_AGENT_TIMEOUT` is only the hard cap
(default 6 h) for a run that never stops producing output. A timeout is reported as
`agent_failure.code=timeout_idle` or `timeout_hard` and the UI says **agent timed out**.
A stopped or timed-out run kills its whole process group (on Unix), so shell commands and
`docker build` children cannot outlive the bridge and hold the Docker builder.

```bash
docker run -d --name redline-agent \
  --network traefik_network \
  -e REDLINE_URL=http://redline:8788 \
  -e REDLINE_AGENT_REPO=/src \
  -v /srv/repos/app:/src \
  -v /srv/opencode-auth:/root/.local/share/opencode \
  -v /usr/local/bin/opencode:/usr/local/bin/opencode:ro \
  redline-agent
```

The token can also be mounted at `/data/token` instead of passed in the environment.

The bridge runs `REDLINE_AGENT_WORKERS` claim workers (default 4) and asks the server for work.
The server's **Agent settings** caps actual parallelism (default 2 global, 1 per project). Each
claim gets a git worktree under `<REDLINE_AGENT_REPO_ROOT>/.redline-worktrees/`; the OpenCode
server must see that directory just like the repositories themselves. Worktrees on resolved
rounds are removed when clean; dirty ones are left for inspection.

## What RedLine does *not* do

- It never runs a model itself; the agent is always the user's.
- It does not sandbox the agent. The container/mounts decide what the agent can touch — that is the
  operator's choice, not a RedLine setting.
- It does not push over MCP; `redline-agent` and `REDLINE_WEBHOOK_URL` are the push mechanisms.
- It does not route reviews between multiple agents yet. Run one agent per RedLine server; the
  `agent_id` and repo keys are stored so a router can be added without migrating projects.
