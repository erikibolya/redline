#!/usr/bin/env python3
"""RedLine agent bridge: turn a human submission into an unattended agent run.

Watches a RedLine server for submitted reviews that no agent has answered yet, then runs
`opencode run` in a repository with the review baked into the prompt. The nested agent reports
progress, replies and resolves through the RedLine MCP endpoint (curl), so the human sees the
whole state machine in the editor.

Stdlib only. One loop, one repo, no queue: a review that fails is answered with the error and
left for a human instead of being retried forever.

Environment:
  REDLINE_URL        http://127.0.0.1:8788 (default)
  REDLINE_TOKEN      required
  REDLINE_AGENT_REPO /src (default)
  REDLINE_AGENT_MODEL deepseek/deepseek-flash (default)
  REDLINE_AGENT_POLL 10 (seconds)
  REDLINE_AGENT_TIMEOUT 1800 (seconds per run)
  OPENCODE_BIN       opencode (default)
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

URL = os.environ.get("REDLINE_URL", "http://127.0.0.1:8788").rstrip("/")
TOKEN = os.environ["REDLINE_TOKEN"]
REPO = os.environ.get("REDLINE_AGENT_REPO", "/src")
MODEL = os.environ.get("REDLINE_AGENT_MODEL", "deepseek/deepseek-flash")
POLL = int(os.environ.get("REDLINE_AGENT_POLL", "10"))
TIMEOUT = int(os.environ.get("REDLINE_AGENT_TIMEOUT", "1800"))
OPENCODE = os.environ.get("OPENCODE_BIN", "opencode")

SEEN: set[str] = set()


def log(message: str) -> None:
    print(f"[bridge {time.strftime('%H:%M:%S')}] {message}", flush=True)


def api(path: str, method: str = "GET", body: dict | None = None) -> dict:
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(URL + path, data=data, method=method)
    request.add_header("Authorization", f"Bearer {TOKEN}")
    if data is not None:
        request.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(request, timeout=120) as response:
        return json.loads(response.read() or b"{}")


def mcp(tool: str, arguments: dict) -> dict:
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
    }
    result = api("/mcp", "POST", payload)
    content = result.get("result", {}).get("content", [{}])
    text = content[0].get("text", "") if content else ""
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return {"text": text}


def session_meta(session_id: str) -> dict:
    return api(f"/api/sessions/{session_id}").get("meta", {})


def prompt_for(meta: dict, feedback: dict) -> str:
    annotations = json.dumps(feedback.get("annotations") or [], ensure_ascii=False, indent=2)
    markdown = feedback.get("markdown") or "(no markdown)"
    session_id = meta["id"]
    return f"""You are working unattended in the repository at {REPO} on a visual review from RedLine.

Review id: {session_id}
Review URL: {feedback.get('review_url') or ''}
Project: {meta.get('project') or 'Inbox'}
Prompt from the human: {meta.get('prompt') or meta.get('note') or '(none)'}

The human's annotations (JSON):
{annotations}

The rendered feedback:
{markdown}

Your job:
1. Read the repository's AGENTS.md and follow it.
2. Implement what the annotations ask for. Keep the change minimal and in the style of the
   codebase. Run the tests you can run. Do not commit, push, deploy, restart containers, or touch
   files outside the repository.
3. Narrate as you work through the RedLine MCP endpoint with curl — one line per meaningful step,
   at most six:
   curl -s -H "Authorization: Bearer $REDLINE_TOKEN" -H 'content-type: application/json' \\
     -d '{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"progress","arguments":{{"session_id":"{session_id}","message":"...","state":"tool"}}}}}}' \\
     "$REDLINE_URL/mcp"
4. When done, send a short reply for the human in their language (tool `reply`, session
   `{session_id}`) and mark the review resolved (tool `resolve`). If you can produce a screenshot
   of the result, create a follow-up review with `capture_review` and put its review URL in the
   reply.

Do not ask questions: nobody is watching. If something is ambiguous, pick the smallest sensible
interpretation and say what you assumed in the reply."""


def run_agent(meta: dict, feedback: dict) -> subprocess.CompletedProcess:
    environment = dict(os.environ)
    environment["REDLINE_URL"] = URL
    environment["REDLINE_TOKEN"] = TOKEN
    environment["REDLINE_REVIEW_ID"] = meta["id"]
    command = [
        OPENCODE,
        "run",
        "--standalone",
        "--model",
        MODEL,
        "--title",
        f"redline {meta['id']}",
        prompt_for(meta, feedback),
    ]
    log(f"running {MODEL} for {meta['id']}")
    return subprocess.run(
        command,
        cwd=REPO,
        env=environment,
        capture_output=True,
        text=True,
        timeout=TIMEOUT,
    )


def handle(meta: dict) -> None:
    session_id = meta["id"]
    SEEN.add(session_id)
    log(f"picked up {session_id} ({meta.get('title')})")
    try:
        feedback = mcp("get_feedback", {"session_id": session_id})
        mcp("progress", {"session_id": session_id, "message": "Picked up by the agent bridge", "state": "thinking"})
        result = run_agent(meta, feedback)
    except Exception as error:  # noqa: BLE001 - one review must not kill the loop
        log(f"run failed for {session_id}: {error}")
        try:
            mcp("reply", {
                "session_id": session_id,
                "message": f"The agent bridge could not finish this review: {error}",
            })
        except Exception:  # noqa: BLE001
            pass
        return

    after = session_meta(session_id)
    if after.get("status") == "resolved":
        log(f"{session_id} resolved by the agent")
        return

    tail = (result.stdout or result.stderr or "").strip().splitlines()
    summary = " ".join(tail[-3:])[:600] or "no output"
    log(f"{session_id} finished without resolve; replying with a summary")
    try:
        mcp("reply", {
            "session_id": session_id,
            "message": "The agent finished but did not resolve the review. Last output:\n\n" + summary,
        })
    except Exception as error:  # noqa: BLE001
        log(f"reply failed for {session_id}: {error}")


def main() -> int:
    log(f"watching {URL} for submitted reviews; repo {REPO}; model {MODEL}")
    while True:
        try:
            sessions = api("/api/sessions").get("sessions", [])
        except Exception as error:  # noqa: BLE001
            log(f"cannot reach RedLine: {error}")
            time.sleep(POLL)
            continue
        for meta in sorted(sessions, key=lambda item: item.get("created_at", "")):
            if meta.get("status") != "submitted":
                continue
            if meta.get("agent_state") in ("working", "replied"):
                continue
            if meta["id"] in SEEN:
                continue
            handle(meta)
        time.sleep(POLL)


if __name__ == "__main__":
    sys.exit(main())
