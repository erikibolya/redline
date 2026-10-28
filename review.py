#!/usr/bin/env python3
"""
au-visual-review: tiny local screenshot annotation server.

The AI agent creates a session with a screenshot; the human draws notes in the browser;
the result is written back as annotations.json + annotated.png + review.md.

No third-party dependencies. Python 3.10+.
"""
from __future__ import annotations

import argparse
import base64
import json
import mimetypes
import re
import shutil
import sys
import time
import uuid
import webbrowser
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse

ROOT = Path(__file__).resolve().parent
DEFAULT_SESSIONS = ROOT / "sessions"
STATIC = ROOT / "static"
DEFAULT_PORT = 8788
ID_RE = re.compile(r"^[a-zA-Z0-9][a-zA-Z0-9._-]{0,120}$")
MAX_BODY = 60 * 1024 * 1024


def now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def slugify(value: str) -> str:
    value = value.lower().strip()
    value = re.sub(r"[^a-z0-9]+", "-", value)
    return value.strip("-")[:60] or "review"


def session_dir(sessions: Path, session_id: str) -> Path:
    if not ID_RE.match(session_id):
        raise ValueError("invalid session id")
    return sessions / session_id


def write_json(path: Path, data: object) -> None:
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def read_json(path: Path, default=None):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return default


def load_meta(session_path: Path) -> dict | None:
    return read_json(session_path / "meta.json")


def create_session(
    sessions: Path,
    image: Path,
    title: str,
    url: str = "",
    note: str = "",
    session_id: str | None = None,
) -> dict:
    if not image.is_file():
        raise FileNotFoundError(image)
    sessions.mkdir(parents=True, exist_ok=True)

    if session_id:
        if not ID_RE.match(session_id):
            raise ValueError("invalid session id")
        sid = session_id
    else:
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
        sid = f"{stamp}-{slugify(title)}-{uuid.uuid4().hex[:4]}"

    target = sessions / sid
    if target.exists():
        raise FileExistsError(target)

    target.mkdir()
    extension = image.suffix.lower() or ".png"
    shutil.copyfile(image, target / f"screenshot{extension}")

    meta = {
        "id": sid,
        "title": title or sid,
        "url": url,
        "note": note,
        "status": "open",
        "created_at": now(),
        "submitted_at": None,
        "image": f"screenshot{extension}",
    }
    write_json(target / "meta.json", meta)
    write_json(target / "annotations.json", {"session": sid, "annotations": []})
    return meta


def list_sessions(sessions: Path) -> list[dict]:
    if not sessions.exists():
        return []
    result = []
    for path in sessions.iterdir():
        if not path.is_dir():
            continue
        meta = load_meta(path)
        if meta:
            result.append(meta)
    return sorted(result, key=lambda item: item.get("created_at", ""), reverse=True)


def review_markdown(meta: dict, annotations: list[dict]) -> str:
    lines = [f"# Visual review: {meta['title']}", ""]
    if meta.get("url"):
        lines += [f"- Page: {meta['url']}"]
    if meta.get("note"):
        lines += [f"- Request: {meta['note']}"]
    lines += [f"- Status: {meta.get('status')}", f"- Submitted: {meta.get('submitted_at') or '-'}", ""]
    lines += ["## Annotations", ""]

    text_notes = [a for a in annotations if a.get("type") == "text" and a.get("text")]
    if text_notes:
        lines += ["### Text notes", ""]
        lines += [f"- ({a.get('x')}, {a.get('y')}): {a['text']}" for a in text_notes]
        lines += [""]

    shapes = [a for a in annotations if a.get("type") != "text"]
    if shapes:
        lines += ["### Marked regions", ""]
        for index, item in enumerate(shapes, 1):
            if item.get("type") == "rect":
                start, end = item.get("start", {}), item.get("end", {})
                lines.append(
                    f"- {index}. rectangle ({start.get('x')}, {start.get('y')}) → "
                    f"({end.get('x')}, {end.get('y')}), color {item.get('color')}, "
                    f"width {item.get('width')}"
                )
            else:
                lines.append(f"- {index}. {item.get('type')} (see annotations.json)")
        lines += [""]

    lines += ["## For the agent", "", "Use `annotations.json` for coordinates and text; "
              "`annotated.png` is the visual reference.", ""]
    return "\n".join(lines)


def submit_annotations(
    sessions: Path,
    session_id: str,
    annotations: list[dict],
    png_base64: str | None = None,
    width: int | None = None,
    height: int | None = None,
) -> dict:
    path = session_dir(sessions, session_id)
    meta = load_meta(path)
    if not meta:
        raise FileNotFoundError(session_id)

    meta["status"] = "submitted"
    meta["submitted_at"] = now()
    if width:
        meta["image_width"] = width
    if height:
        meta["image_height"] = height

    payload = {
        "session": session_id,
        "title": meta.get("title"),
        "url": meta.get("url"),
        "image": {"file": meta.get("image"), "width": width, "height": height},
        "annotations": annotations,
        "submitted_at": meta["submitted_at"],
    }
    write_json(path / "annotations.json", payload)
    write_json(path / "meta.json", meta)
    (path / "review.md").write_text(review_markdown(meta, annotations), encoding="utf-8")

    if png_base64:
        raw = png_base64.split(",", 1)[-1]
        (path / "annotated.png").write_bytes(base64.b64decode(raw))

    return {"meta": meta, "annotations": payload}


class Handler(BaseHTTPRequestHandler):
    server_version = "au-visual-review/1.0"
    sessions: Path = DEFAULT_SESSIONS
    server_port: int = DEFAULT_PORT
    host: str = "127.0.0.1"

    def log_message(self, fmt, *args):
        sys.stderr.write("%s %s\n" % (self.log_date_time_string(), fmt % args))

    def send_json(self, data: object, status: int = 200) -> None:
        body = json.dumps(data, ensure_ascii=False).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def send_file(self, path: Path, content_type: str | None = None) -> None:
        if not path.is_file():
            self.send_error(404)
            return
        body = path.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", content_type or mimetypes.guess_type(path.name)[0] or "application/octet-stream")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def send_static(self, name: str) -> None:
        candidate = (STATIC / name).resolve()
        if STATIC not in candidate.parents:
            self.send_error(404)
            return
        self.send_file(candidate)

    def read_body(self) -> bytes:
        length = int(self.headers.get("Content-Length", "0"))
        if length <= 0 or length > MAX_BODY:
            raise ValueError("invalid body size")
        return self.rfile.read(length)

    def do_GET(self):
        parsed = urlparse(self.path)
        path = unquote(parsed.path)

        if path in ("/", "/index.html", "/sessions"):
            self.send_static("list.html")
            return
        if path == "/editor" or path == "/editor.html":
            self.send_static("index.html")
            return
        if path.startswith("/s/"):
            self.send_static("index.html")
            return
        if path.startswith("/sessions/") and path.endswith("/image"):
            sid = path[len("/sessions/"):-len("/image")].strip("/")
            if not ID_RE.match(sid):
                self.send_error(404)
                return
            meta = load_meta(session_dir(self.sessions, sid))
            if not meta:
                self.send_error(404)
                return
            self.send_file(self.sessions / sid / meta.get("image", "screenshot.png"))
            return
        if path.startswith("/sessions/") and path.endswith("/annotated.png"):
            sid = path[len("/sessions/"):-len("/annotated.png")].strip("/")
            if not ID_RE.match(sid):
                self.send_error(404)
                return
            self.send_file(session_dir(self.sessions, sid) / "annotated.png", "image/png")
            return
        if path.startswith("/static/"):
            self.send_static(path[len("/static/"):])
            return
        if path == "/api/sessions":
            self.send_json({"sessions": list_sessions(self.sessions)})
            return
        if path.startswith("/api/sessions/"):
            rest = path[len("/api/sessions/"):].strip("/")
            if "/" in rest:
                self.send_error(404)
                return
            sid = rest
            if not ID_RE.match(sid):
                self.send_error(404)
                return
            folder = session_dir(self.sessions, sid)
            meta = load_meta(folder)
            if not meta:
                self.send_error(404)
                return
            annotations = read_json(folder / "annotations.json", {"session": sid, "annotations": []})
            self.send_json({"meta": meta, "annotations": annotations})
            return
        if path == "/health":
            self.send_json({"ok": True})
            return
        self.send_error(404)

    def do_POST(self):
        parsed = urlparse(self.path)
        path = unquote(parsed.path)
        if path.startswith("/api/sessions/") and path.endswith("/annotations"):
            sid = path[len("/api/sessions/"):-len("/annotations")].strip("/")
            if not ID_RE.match(sid):
                self.send_error(404)
                return
            try:
                data = json.loads(self.read_body().decode("utf-8"))
                result = submit_annotations(
                    self.sessions,
                    sid,
                    data.get("annotations", []),
                    data.get("png"),
                    data.get("width"),
                    data.get("height"),
                )
            except FileNotFoundError:
                self.send_error(404)
                return
            except (ValueError, KeyError, json.JSONDecodeError) as exc:
                self.send_json({"error": str(exc)}, 400)
                return
            self.send_json({"ok": True, **result})
            return
        self.send_error(404)


def cmd_serve(args) -> int:
    sessions = Path(args.sessions).resolve() if args.sessions else DEFAULT_SESSIONS
    sessions.mkdir(parents=True, exist_ok=True)
    Handler.sessions = sessions
    Handler.host = args.host
    Handler.server_port = args.port
    httpd = ThreadingHTTPServer((args.host, args.port), Handler)
    url = f"http://{args.host}:{args.port}/"
    print(f"au-visual-review: {url}")
    print(f"sessions: {sessions}")
    if args.open:
        webbrowser.open(url)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nstopped")
    return 0


def cmd_open(args) -> int:
    sessions = Path(args.sessions).resolve() if args.sessions else DEFAULT_SESSIONS
    meta = create_session(sessions, Path(args.image), args.title or Path(args.image).stem, args.url, args.note, args.id)
    url = f"http://{args.host}:{args.port}/s/{meta['id']}"
    if args.json:
        print(json.dumps({**meta, "dir": str(sessions / meta["id"]), "review_url": url}, indent=2, ensure_ascii=False))
    else:
        print(f"session: {meta['id']}")
        print(f"dir:     {sessions / meta['id']}")
        print(f"url:     {url}")
    if args.open_browser:
        webbrowser.open(url)
    return 0


def cmd_list(args) -> int:
    sessions = Path(args.sessions).resolve() if args.sessions else DEFAULT_SESSIONS
    items = list_sessions(sessions)
    if args.json:
        print(json.dumps(items, indent=2, ensure_ascii=False))
        return 0
    for meta in items:
        print(f"{meta['id']:45} {meta['status']:10} {meta.get('title', '')}")
    return 0


def cmd_get(args) -> int:
    sessions = Path(args.sessions).resolve() if args.sessions else DEFAULT_SESSIONS
    folder = session_dir(sessions, args.id)
    meta = load_meta(folder)
    if not meta:
        print("session not found", file=sys.stderr)
        return 1
    annotations = read_json(folder / "annotations.json", {"session": args.id, "annotations": []})
    print(json.dumps({"meta": meta, "annotations": annotations}, indent=2, ensure_ascii=False))
    return 0


def cmd_wait(args) -> int:
    sessions = Path(args.sessions).resolve() if args.sessions else DEFAULT_SESSIONS
    deadline = time.time() + args.timeout
    while time.time() < deadline:
        meta = load_meta(session_dir(sessions, args.id))
        if meta and meta.get("status") == "submitted":
            return cmd_get(args)
        time.sleep(args.interval)
    print(f"timeout waiting for {args.id}", file=sys.stderr)
    return 2


def cmd_remove(args) -> int:
    sessions = Path(args.sessions).resolve() if args.sessions else DEFAULT_SESSIONS
    folder = session_dir(sessions, args.id)
    if not folder.exists():
        print("session not found", file=sys.stderr)
        return 1
    if not args.yes:
        answer = input(f"remove {folder}? [y/N] ").strip().lower()
        if answer not in ("y", "yes"):
            return 1
    shutil.rmtree(folder)
    print(f"removed {folder}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="review", description="Local screenshot review queue for the AI agent + human.")
    sub = parser.add_subparsers(dest="command", required=True)

    def common(p):
        p.add_argument("--sessions", help="sessions directory (default: ./sessions)")
        p.add_argument("--json", action="store_true", help="machine-readable output")

    serve = sub.add_parser("serve", help="serve the review UI")
    serve.add_argument("--host", default="127.0.0.1")
    serve.add_argument("--port", type=int, default=DEFAULT_PORT)
    serve.add_argument("--sessions")
    serve.add_argument("--open", action="store_true", help="open the browser")
    serve.set_defaults(func=cmd_serve)

    open_cmd = sub.add_parser("open", help="create a session from a screenshot")
    open_cmd.add_argument("image")
    open_cmd.add_argument("--title", default="")
    open_cmd.add_argument("--url", default="")
    open_cmd.add_argument("--note", default="")
    open_cmd.add_argument("--id")
    open_cmd.add_argument("--host", default="127.0.0.1")
    open_cmd.add_argument("--port", type=int, default=DEFAULT_PORT)
    open_cmd.add_argument("--sessions")
    open_cmd.add_argument("--json", action="store_true")
    open_cmd.add_argument("--open-browser", action="store_true")
    open_cmd.set_defaults(func=cmd_open)

    list_cmd = sub.add_parser("list", help="list sessions")
    common(list_cmd)
    list_cmd.set_defaults(func=cmd_list)

    get_cmd = sub.add_parser("get", help="print a session and its annotations")
    get_cmd.add_argument("id")
    get_cmd.add_argument("--sessions")
    get_cmd.add_argument("--json", action="store_true")
    get_cmd.set_defaults(func=cmd_get)

    wait_cmd = sub.add_parser("wait", help="wait until the human submits")
    wait_cmd.add_argument("id")
    wait_cmd.add_argument("--sessions")
    wait_cmd.add_argument("--json", action="store_true")
    wait_cmd.add_argument("--timeout", type=int, default=600)
    wait_cmd.add_argument("--interval", type=int, default=2)
    wait_cmd.set_defaults(func=cmd_wait)

    rm_cmd = sub.add_parser("remove", help="delete a session")
    rm_cmd.add_argument("id")
    rm_cmd.add_argument("--sessions")
    rm_cmd.add_argument("--yes", action="store_true")
    rm_cmd.set_defaults(func=cmd_remove)

    return parser


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
