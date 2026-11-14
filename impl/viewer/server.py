#!/usr/bin/env python3
"""Local server for the design viewer (impl/viewer).

Serves the viewer page and, read-only, the game's own engine-neutral data: the part catalog
and designs (`content/data/`) and the part models (`assets/export/models/`). The viewer saves
only in the game's formats and folders, so whatever it writes the game loads as is:

    POST /api/save    {"path": "content/data/vehicles/<id>.json", "text": ...}
                      {"path": "assets/export/models/<model>.glb", "base64": ...}
                      {"path": ".viewer-shots/<name>.png", "base64": ...}

    GET  /api/list     designs, part files and models
    GET  /api/version  a stamp that changes whenever any of them changes (live reload)

Binds to localhost only. Usage: tools/viewer.sh [port]
"""

import base64
import json
import re
import sys
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VIEWER = Path(__file__).resolve().parent
CONTENT = ROOT / "content" / "data"
MODELS = ROOT / "assets" / "export" / "models"
SHOTS = ROOT / ".viewer-shots"

# Paths the viewer may write (relative to the repo root), and nothing else.
WRITABLE = [
    (re.compile(r"^content/data/vehicles/[a-z0-9_]+\.json$"), "text"),
    (re.compile(r"^assets/export/models/[a-z0-9_.]+\.glb$"), "base64"),
    (re.compile(r"^\.viewer-shots/[A-Za-z0-9_.-]+\.png$"), "base64"),
]


def watched():
    files = list((CONTENT / "parts").glob("*.json"))
    files += list((CONTENT / "vehicles").glob("*.json"))
    files += list(MODELS.glob("*.glb"))
    return files


class Handler(SimpleHTTPRequestHandler):
    def log_message(self, fmt, *args):  # quiet
        pass

    def send_json(self, value, status=200):
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def translate_path(self, path):
        path = path.split("?", 1)[0].split("#", 1)[0]
        if path in ("/", "/index.html"):
            return str(VIEWER / "index.html")
        for prefix, base in (
            ("/viewer/", VIEWER),
            ("/content/", ROOT / "content"),
            ("/assets/", ROOT / "assets"),
        ):
            if path.startswith(prefix):
                target = (base / path[len(prefix):]).resolve()
                if base.resolve() in target.parents or target == base.resolve():
                    return str(target)
        return str(VIEWER / "__missing__")

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def do_GET(self):
        if self.path.startswith("/api/list"):
            self.send_json(
                {
                    "designs": sorted(p.stem for p in (CONTENT / "vehicles").glob("*.json")),
                    "partFiles": sorted(p.name for p in (CONTENT / "parts").glob("*.json")),
                    "models": sorted(p.stem for p in MODELS.glob("*.glb")),
                }
            )
            return
        if self.path.startswith("/api/version"):
            stamp = max((p.stat().st_mtime_ns for p in watched()), default=0)
            self.send_json({"version": str(stamp)})
            return
        super().do_GET()

    def do_POST(self):
        if not self.path.startswith("/api/save"):
            self.send_json({"error": "unknown endpoint"}, 404)
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            request = json.loads(self.rfile.read(length))
            rel = request["path"]
            kind = next((k for pattern, k in WRITABLE if pattern.match(rel)), None)
            if kind is None:
                self.send_json({"error": f"not writable: {rel}"}, 403)
                return
            data = (
                request["text"].encode() if kind == "text" else base64.b64decode(request["base64"])
            )
            if kind == "text":
                json.loads(data)  # designs must stay valid JSON
            target = ROOT / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            self.send_json({"saved": rel, "bytes": len(data)})
        except Exception as e:  # report, don't crash the server
            self.send_json({"error": str(e)}, 400)


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"design viewer: http://localhost:{port}/  (Ctrl+C stops)")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
