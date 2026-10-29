"""Reference LSP bridge.

Speaks the model-gateway's LSP-bridge HTTP contract (POST /lsp/query
with JSON {operation, file_path, line, column}) and translates each
call into LSP JSON-RPC against a language server picked from the file
extension.

This is intentionally minimal: it covers the 4 operations the gateway
exposes (diagnostics / hover / definition / completion) for a handful
of common languages (TypeScript, Python, Go, Rust). Production
operators can:

- Add more languages: extend ``LANGUAGE_SERVERS``.
- Use multilspy (https://github.com/microsoft/multilspy) for a much
  richer manager; this file is a self-contained 200-line reference
  that runs language servers directly via stdio.
- Run on a writable mount so the LSP servers can index the workspace.

Listen port: 9202 (override via PORT env var).
Workspace root: cwd (override via LSP_WORKSPACE_ROOT).
"""

from __future__ import annotations

import json
import os
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

PORT = int(os.environ.get("PORT", "9202"))
WORKSPACE = Path(os.environ.get("LSP_WORKSPACE_ROOT", os.getcwd())).resolve()

# Server registry. Each entry: (file-extension predicate, language id, spawn
# command). Add more as needed; the bridge starts the server lazily on the
# first request for that language.
LANGUAGE_SERVERS = {
    "typescript": (
        lambda p: p.suffix in {".ts", ".tsx", ".js", ".jsx"},
        ["typescript-language-server", "--stdio"],
    ),
    "python": (
        lambda p: p.suffix == ".py",
        ["pylsp"],
    ),
    "go": (
        lambda p: p.suffix == ".go",
        ["gopls"],
    ),
    "rust": (
        lambda p: p.suffix == ".rs",
        ["rust-analyzer"],
    ),
}


def log(level: str, msg: str, **fields):
    print(json.dumps({"ts": time.strftime("%Y-%m-%dT%H:%M:%SZ"), "level": level,
                       "msg": msg, **fields}), flush=True)


class LspProc:
    """One language-server subprocess plus its request/response plumbing."""

    def __init__(self, command: list[str]):
        self.command = command
        self.proc = subprocess.Popen(
            command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        self.lock = threading.Lock()
        self.next_id = 1
        self.responses: dict[int, dict] = {}
        self.diagnostics_by_file: dict[str, list[dict]] = {}
        threading.Thread(target=self._reader, daemon=True).start()
        self._initialize()

    def _send(self, payload: dict) -> int | None:
        body = json.dumps(payload).encode("utf-8")
        header = f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8")
        with self.lock:
            self.proc.stdin.write(header + body)
            self.proc.stdin.flush()
        return payload.get("id")

    def _reader(self):
        # Tiny LSP framing parser: Content-Length: N\r\n\r\n followed by N
        # bytes of JSON. We're permissive about extra headers.
        stream = self.proc.stdout
        while True:
            header = b""
            while b"\r\n\r\n" not in header:
                chunk = stream.read(1)
                if not chunk:
                    return
                header += chunk
            length = 0
            for line in header.split(b"\r\n"):
                if line.lower().startswith(b"content-length:"):
                    length = int(line.split(b":")[1].strip())
            body = stream.read(length)
            try:
                msg = json.loads(body)
            except Exception:
                continue
            if "id" in msg and "method" not in msg:
                self.responses[msg["id"]] = msg
            elif msg.get("method") == "textDocument/publishDiagnostics":
                uri = msg.get("params", {}).get("uri", "")
                self.diagnostics_by_file[uri] = msg["params"].get("diagnostics", [])

    def _initialize(self):
        # Minimal LSP initialize handshake. Skip capabilities that we don't
        # use to keep the surface tight.
        self._send({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {
                "processId": os.getpid(),
                "rootUri": WORKSPACE.as_uri(),
                "capabilities": {},
            },
        })
        # Wait briefly for the response.
        for _ in range(50):
            if 0 in self.responses:
                break
            time.sleep(0.1)
        self._send({"jsonrpc": "2.0", "method": "initialized", "params": {}})

    def request(self, method: str, params: dict, timeout: float = 5.0) -> dict | None:
        req_id = self.next_id
        self.next_id += 1
        self._send({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params})
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if req_id in self.responses:
                return self.responses.pop(req_id)
            time.sleep(0.02)
        return None

    def open_file(self, path: Path):
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            return
        self._send({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": path.as_uri(), "languageId": "plaintext",
                    "version": 1, "text": text,
                },
            },
        })


PROCS: dict[str, LspProc] = {}
PROC_LOCK = threading.Lock()


def language_for(path: Path) -> tuple[str, list[str]] | None:
    for lang, (predicate, command) in LANGUAGE_SERVERS.items():
        if predicate(path):
            return lang, command
    return None


def get_proc(path: Path) -> LspProc | None:
    match = language_for(path)
    if not match:
        return None
    lang, command = match
    with PROC_LOCK:
        if lang not in PROCS:
            try:
                PROCS[lang] = LspProc(command)
            except FileNotFoundError:
                log("warn", "language server binary not installed",
                    language=lang, command=command)
                return None
        return PROCS[lang]


def project(diag: dict) -> dict:
    rng = diag.get("range", {})
    start = rng.get("start", {})
    end = rng.get("end", {})
    sev_map = {1: "error", 2: "warning", 3: "info", 4: "hint"}
    return {
        "severity": sev_map.get(diag.get("severity", 3), "info"),
        "line": start.get("line", 0),
        "column": start.get("character", 0),
        "end_line": end.get("line", 0),
        "end_column": end.get("character", 0),
        "message": diag.get("message", ""),
        "code": str(diag.get("code", "")),
    }


def handle_query(payload: dict) -> tuple[int, dict]:
    op = payload.get("operation", "")
    file_path = payload.get("file_path", "")
    line = payload.get("line", 0)
    column = payload.get("column", 0)
    if not file_path:
        return 400, {"error_message": "file_path is required"}
    path = Path(file_path)
    if not path.exists():
        return 404, {"error_message": f"file not found: {file_path}"}

    proc = get_proc(path)
    if proc is None:
        return 404, {"error_message": f"no language server for {path.suffix}"}

    proc.open_file(path)
    uri = path.as_uri()
    pos = {"line": line, "character": column}

    if op == "diagnostics":
        # Give the server a moment to publish. Real-world setups rely on
        # didOpen + a server-side debounce — 1.5s is a pragmatic default.
        time.sleep(1.5)
        diags = proc.diagnostics_by_file.get(uri, [])
        return 200, {"diagnostics": [project(d) for d in diags]}
    if op == "hover":
        resp = proc.request("textDocument/hover",
                            {"textDocument": {"uri": uri}, "position": pos})
        contents = (resp or {}).get("result", {}).get("contents", "")
        if isinstance(contents, dict):
            contents = contents.get("value", "")
        elif isinstance(contents, list):
            contents = " ".join(c.get("value", "") if isinstance(c, dict) else str(c)
                                for c in contents)
        return 200, {"hover_text": contents or ""}
    if op == "definition":
        resp = proc.request("textDocument/definition",
                            {"textDocument": {"uri": uri}, "position": pos})
        result = (resp or {}).get("result", []) or []
        if isinstance(result, dict):
            result = [result]
        locations = []
        for loc in result:
            target_uri = loc.get("uri", "")
            target_path = target_uri.replace("file://", "")
            rng = loc.get("range", {}).get("start", {})
            locations.append({
                "file_path": target_path,
                "line": rng.get("line", 0),
                "column": rng.get("character", 0),
            })
        return 200, {"locations": locations}
    if op == "completion":
        resp = proc.request("textDocument/completion",
                            {"textDocument": {"uri": uri}, "position": pos})
        items = (resp or {}).get("result", {}).get("items", [])
        if isinstance(resp.get("result"), list):
            items = resp.get("result", [])
        completions = []
        for item in items[:20]:
            completions.append({
                "label": item.get("label", ""),
                "detail": item.get("detail", ""),
                "kind": item.get("kind", 0),
            })
        return 200, {"completions": completions}
    return 400, {"error_message": f"unknown operation: {op}"}


class Handler(BaseHTTPRequestHandler):
    def _json(self, code: int, body: dict):
        encoded = json.dumps(body).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def do_GET(self):  # noqa: N802
        if self.path in ("/health", "/"):
            self._json(200, {"ok": True, "workspace": str(WORKSPACE)})
            return
        self._json(404, {"error_message": "not found"})

    def do_POST(self):  # noqa: N802
        if self.path != "/lsp/query":
            self._json(404, {"error_message": "not found"})
            return
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        try:
            payload = json.loads(body or b"{}")
        except json.JSONDecodeError as e:
            self._json(400, {"error_message": f"bad json: {e}"})
            return
        try:
            code, response = handle_query(payload)
        except Exception as e:
            log("error", "lsp handler crashed", error=str(e))
            code, response = 500, {"error_message": f"internal: {e}"}
        self._json(code, response)

    def log_message(self, *_):
        return  # quiet the default access log; use the structured logger


def main():
    log("info", "lsp bridge listening", port=PORT, workspace=str(WORKSPACE))
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()


if __name__ == "__main__":
    main()
