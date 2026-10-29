# agent-bridge

Turns a RedLine submission into an unattended agent run.

RedLine is pull-only for agents: a submission is state on disk, not a request to a running model.
This bridge owns the loop instead. It watches `/api/sessions` for `submitted` reviews that no agent
has answered, runs `opencode run` in a repository with the annotations baked into the prompt, and
lets the nested agent report through RedLine (`progress`, `reply`, `capture_review`, `resolve`).

The human never leaves the review UI: they submit, see `received` / `working` with activity, get
the reply, and — if the agent produced a screenshot — a follow-up review to look at.

## Run

```bash
REDLINE_URL=https://redline.ibolya.cz \
REDLINE_TOKEN="$(cat /mnt/user/appdata/redline/token)" \
REDLINE_AGENT_REPO=/mnt/user/Development/repos/redline \
python3 bridge.py
```

| Variable | Default | Meaning |
|---|---|---|
| `REDLINE_URL` | `http://127.0.0.1:8788` | RedLine base URL |
| `REDLINE_TOKEN` | — (required) | API token |
| `REDLINE_AGENT_REPO` | `/src` | repository the agent works in |
| `REDLINE_AGENT_MODEL` | `deepseek/deepseek-flash` | OpenCode model id |
| `REDLINE_AGENT_POLL` | `10` | seconds between polls |
| `REDLINE_AGENT_TIMEOUT` | `1800` | seconds per run |
| `OPENCODE_BIN` | `opencode` | OpenCode binary |

Needs `opencode` on `PATH` with provider auth already configured (`opencode auth list`).

## Rules it enforces on the agent

- Work only inside `REDLINE_AGENT_REPO`; no commit, push, deploy or container restarts.
- Narrate with `progress` (max six lines), reply in the human's language, then `resolve`.
- A failed run is answered with the error and left for a human; the review id is not retried in
  the same process.

## Limits

- One repository and one model per bridge process.
- No queue and no persistence: restarting the bridge forgets in-flight ids (the session state in
  RedLine still protects against re-running something already answered).
- The nested agent runs with the bridge's permissions. Run it in a container that mounts only the
  repository you want it to touch.

## Container sketch

```bash
docker run -d --name redline-agent \
  --network traefik_network \
  -e REDLINE_URL=http://redline:8788 \
  -e REDLINE_TOKEN=... \
  -e REDLINE_AGENT_REPO=/src \
  -v /mnt/user/Development/repos/redline:/src \
  -v /mnt/user/appdata/agent-tools/opencode:/root/.local/share/opencode \
  redline-agent
```
