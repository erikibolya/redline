# redline-agent

The unattended half of RedLine: a submission becomes an OpenCode run.

RedLine is pull-only for agents — a submission is state on disk, not a request to a running
model. This binary owns the loop instead:

1. polls `/api/sessions` for `submitted` reviews nobody has answered,
2. reads the feedback (`get_feedback`, which marks the review `received`),
3. runs `opencode run` in a repository with the annotations in the prompt,
4. lets that run report back through RedLine (`progress`, `reply`, `capture_review`, `resolve`).

It runs **next to OpenCode**, wherever that is. On a desktop the RedLine app starts it for you
(tray → *Start local agent*); on a server it is a plain binary.

## Binary

```bash
REDLINE_URL=https://redline.example \
REDLINE_TOKEN=… \
REDLINE_AGENT_REPO=~/src/app \
redline-agent
```

| Variable | Default | Meaning |
|---|---|---|
| `REDLINE_URL` | `http://127.0.0.1:8788` | RedLine base URL |
| `REDLINE_TOKEN` | `/data/token`, `~/.config/redline/token` | API token |
| `REDLINE_AGENT_REPO` | current directory | repository the agent works in |
| `REDLINE_AGENT_MODEL` | `deepseek/deepseek-flash` | OpenCode model id |
| `REDLINE_AGENT_OPENCODE_SERVER` | — | dispatch into an already running OpenCode instead of a private `--standalone` run |
| `REDLINE_AGENT_POLL` | `10` | seconds between polls |
| `REDLINE_AGENT_TIMEOUT` | `1800` | seconds per run |
| `OPENCODE_BIN` | `opencode` | OpenCode binary |

Needs `opencode` with provider auth configured (`opencode auth list`).

## Desktop app

The Tauri shell embeds this crate. Tray menu:

- **Choose agent repository…** — folder the agent edits (saved in `agent.json` beside `config.json`)
- **Start local agent** / **Stop local agent**

`agent.json` is optional and only needed for non-default models:

```json
{ "repo": "C:\\src\\app", "model": "deepseek/deepseek-flash", "opencode_server": "" }
```

## Behaviour

- One review at a time, no queue. A failed run is answered with the error and left open for a
  human; the id is remembered for the lifetime of the process.
- The agent inherits the user's OpenCode permissions. This binary does not sandbox it: run it as a
  user (or in a container) that has exactly the access the agent should have.
- The prompt tells the agent to leave the repository reviewable: no history rewrites, no pushes.
