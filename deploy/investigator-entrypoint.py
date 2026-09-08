#!/usr/bin/env python3
"""Investigator packaging stub.

Does not call an LLM, does not invent citations, and does not change
predictions, policies, ledgers or orders.
"""

from __future__ import annotations

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


BIND = ("0.0.0.0", 8092)


def _json(handler: BaseHTTPRequestHandler, code: int, body: dict) -> None:
    raw = json.dumps(body, separators=(",", ":")).encode("utf-8")
    handler.send_response(code)
    handler.send_header("content-type", "application/json")
    handler.send_header("content-length", str(len(raw)))
    handler.end_headers()
    handler.wfile.write(raw)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt: str, *args) -> None:  # noqa: A003
        print(f"investigator {self.command} {self.path} {fmt % args}")

    def do_GET(self) -> None:  # noqa: N802
        if self.path in ("/internal/health", "/health"):
            _json(
                self,
                200,
                {
                    "ready": True,
                    "service": "investigator",
                    "status": "UNIMPLEMENTED",
                    "provider": None,
                    "note": "Optional investigator package is not implemented; this process is a packaging health stub.",
                },
            )
            return
        _json(self, 404, {"error": {"code": "NOT_FOUND", "message": self.path}})

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("content-length", "0") or 0)
        _ = self.rfile.read(length) if length else b""
        _json(
            self,
            503,
            {
                "error": {
                    "code": "INVESTIGATOR_UNAVAILABLE",
                    "message": "Investigator is not implemented. Core capture and paper continue without it.",
                    "retryable": False,
                }
            },
        )


def main() -> None:
    server = ThreadingHTTPServer(BIND, Handler)
    print(f"investigator stub listening on {BIND[0]}:{BIND[1]} status=UNIMPLEMENTED")
    server.serve_forever()


if __name__ == "__main__":
    main()
