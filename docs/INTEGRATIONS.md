# Integrating an agent with RedLine

RedLine owns the review, the annotations and the human's decision. It does **not** own git,
deployments or publishing: those are the integration's job. This document defines the MCP/HTTP
contract so a new agent can plug in without copying the reference git bridge.

## The decision is the contract

A session has a `feature_state`:

| State | Meaning for an integration |
|---|---|
| `pending` | No human decision yet. Do not perform a post-confirmation action. |
| `confirmed` | The human accepted the feature. This is permission to proceed according to the integration's own workflow. |
| `cancelled` | The human rejected it. Do not perform the action. |

`feature_state` is derived from `feature_confirmed_at` / `feature_cancelled_at`. It is the same
over MCP (`get_feedback`, `wait_for_feedback`, `list_reviews`) and over HTTP
(`GET /api/sessions`).

## The optional handoff

A confirmation can carry an optional, **generic** handoff:

```json
{
  "handoff": {
    "status": "requested",
    "requested_at": "2026-10-31T10:00:00+00:00",
    "started_at": null,
    "finished_at": null,
    "label": null,
    "url": null,
    "message": null
  }
}
```

| Status | Who sets it | Meaning |
|---|---|---|
| `requested` | `POST /api/sessions/<id>/confirm-feature` with `{"handoff": true}`, or `POST /api/sessions/<id>/handoff` | Waiting for a capable integration |
| `running` | The server, when an integration claims the job | In progress |
| `done` | The integration via MCP `handoff_status` | It finished its action |
| `failed` | The integration via MCP `handoff_status` | It could not finish; the rail offers a retry |

`label`, `url` and `message` are the integration's vocabulary, not RedLine's. Examples:

- git: `label: "merged"`, `url: "https://github.com/o/r/pull/46"`, `message: "merge commit abc123"`
- deploy: `label: "deployed"`, `url: "https://staging.example.com"`, `message: "release 1.4.2"`
- failed: `label: "PR #46 open"`, `url: ".../pull/46"`, `message: "checks are red"`

The rail renders the label with the URL as a link; a `failed` handoff gets a row `↻` action (and
**Retry handoff** in the feature menu). `done` is final; only a `failed` handoff can be retried
with `POST /api/sessions/<id>/handoff`, and the next attempt keeps the previous label/url/message.

Confirming **without** a handoff records only the decision and closes the review. That is the
right choice for an integration that has no post-confirmation action, or for a human who does not
want an agent to do anything else.

## Claiming a handoff

The bridge advertises the job kinds it understands:

```http
POST /api/agent/next
{"agent_id": "my-agent", "jobs": ["review", "handoff"]}
```

The response is one of:

```json
{"session": {"…": "meta"}, "claim": {"…": "claim"}, "job": "review", "limits": {"…": "…"}}
{"session": {"…": "meta"}, "claim": {"…": "claim"}, "job": "handoff", "limits": {"…": "…"}}
{"session": null, "reason": "empty|at_capacity|branch_locked|claimed", "limits": {"…": "…"}}
```

- `review` means a submitted round: read the annotations, fix, push, reply/post a round.
- `handoff` means a confirmed feature with a claimable `handoff`; the meta's
  `handoff.status` is already `running`.
- A bridge that sends no `jobs` is review-only and is **never** handed a handoff job. This keeps
  old integrations from receiving a job they do not understand.
- The claim's `work_key` is the branch/lock key the review actually used; it can be a legacy
  `session/<id>` even when `meta.branch` reads as `feat/…`. Use it when you need the exact branch.
- Heartbeat the claim and release it when done; the server's limits, leases and one-owner-per-branch
  rules are the same for both jobs. A waiting claim (the human has the ball) does not consume a
  run slot.
- While a running claim is alive the rail shows **working**, whatever the last `progress`/`reply`
  said: the claim is the evidence, the label is only a message. If the bridge dies, the lease
  expires and the spinner stops instead of promising work that is not happening.

## A run that cannot start

`reply` is not only a message — its `state` says whether the agent answered or could not work:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "reply",
    "arguments": {
      "session_id": "20261031-120000-feature-1a2b",
      "message": "The agent run failed: Server at http://opencode:4096 rejected the password",
      "state": "failed"
    }
  }
}
```

`state` is `replied` (default: the agent answered and may still be working) or `failed` (the run
could not happen). A failure sets `agent_state=failed` and **hands the round back**
(`status=open`, annotations and history intact), so the human can fix the problem and resubmit
instead of staring at a locked review. Release the claim afterwards, as usual. Do not use
`failed` for a normal answer: the round must stay submitted while the agent still intends to
finish it.

## Reporting the result

Call the MCP tool `handoff_status` after the integration's action:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "handoff_status",
    "arguments": {
      "session_id": "20261031-120000-feature-1a2b",
      "state": "done",
      "label": "merged",
      "url": "https://github.com/o/r/pull/46",
      "message": "merge commit abc123"
    }
  }
}
```

`state` is `done` or `failed`. `url` must be `http(s)`; `label`/`message` are optional. The server
stores the result and the rail updates. A report for a feature the human reopened or cancelled in
the meantime is refused; do not retry it blindly.

The tool is in the MCP tool list, and `handoff` also appears in `get_feedback`,
`wait_for_feedback`, `list_reviews` and `GET /api/sessions`.

## Three examples

### 1. Git (the reference `redline-agent`)

1. A review run implements the annotations and **pushes the branch**, but does not open a PR.
2. The human confirms with **Hand off** checked.
3. The bridge claims `job=handoff` and runs a second prompt that may open/reuse the PR, wait for
   the required checks and merge.
4. It reports `state=done, label=merged, url=<PR>`; the rail shows `merged` with a link.
5. If checks are red, it reports `state=failed, label=PR #46 open, url=<PR>, message=<check>`;
   the rail shows the link and `↻`; a human retry can reuse the PR.

The review prompt must **not** ship: opening the PR during the review would hand off a feature the
human has not accepted. This is an integration rule, not a server enforcement; RedLine cannot stop
a client from running git.

### 2. Deploy

1. The review run builds a preview and posts a screenshot; it does not deploy to production.
2. The human confirms with **Hand off** checked.
3. The bridge claims `job=handoff` and deploys the confirmed revision.
4. It reports `state=done, label=deployed, url=https://staging.example.com, message=release 42`.

No PR and no git are involved. The same `handoff` job and rail badge are used.

### 3. No handoff

1. The review run implements the change and replies.
2. The human confirms **without** ticking the handoff.
3. The server records `feature_state=confirmed` and closes the review. No `handoff` exists, so
   there is no queued job and no stuck badge.

An integration that wants to act on every confirmation can poll `GET /api/sessions` for
`feature_state=confirmed` and do its own work, but it should record its own idempotency: RedLine's
handoff job exists precisely to dedupe and retry that work.

## What RedLine will never do

- Run `git`, `gh`, a deployment or a publish.
- Store a GitHub token or any integration credential.
- Decide what "done" means for your workflow.
- Guarantee that a client obeys the review/handoff split. The reference bridge does it with two
  prompts and a forbidden-merge instruction; a hard guarantee needs credential separation (a
  review token that can push but not merge) outside RedLine.

## Implementing a bridge (checklist)

- [ ] Advertise `jobs: ["review","handoff"]` (or just `["review"]`) on `/api/agent/next`.
- [ ] On `job=review`: never perform the post-confirmation action.
- [ ] On `job=handoff`: read `meta.handoff` (label/url/message from a previous attempt), perform
      the action, report `handoff_status` with `done`/`failed`.
- [ ] Heartbeat/lease/release the claim; use the claim's `work_key`.
- [ ] Keep the failed attempt's label/url/message so a retry can reuse it.
- [ ] Do not report `done` before the action is actually done (a merged PR, a live deployment).
