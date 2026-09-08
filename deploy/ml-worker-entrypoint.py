#!/usr/bin/env python3
"""Internal ml-worker packaging stub.

Serves the worker health contract. Predict and job endpoints abstain and
never fabricate probabilities. This is not a trained model service.
"""

from __future__ import annotations

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


BIND = ("0.0.0.0", 8091)


def _json(handler: BaseHTTPRequestHandler, code: int, body: dict) -> None:
    raw = json.dumps(body, separators=(",", ":")).encode("utf-8")
    handler.send_response(code)
    handler.send_header("content-type", "application/json")
    handler.send_header("content-length", str(len(raw)))
    handler.end_headers()
    handler.wfile.write(raw)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt: str, *args) -> None:  # noqa: A003
        print(f"ml-worker {self.command} {self.path} {fmt % args}")

    def do_GET(self) -> None:  # noqa: N802
        if self.path in ("/internal/health", "/health"):
            _json(
                self,
                200,
                {
                    "ready": True,
                    "service": "ml-worker",
                    "status": "MODEL NOT TRAINED",
                    "loaded_models": [],
                    "queue_depth": 0,
                    "note": "Python worker package is not implemented; this process is a packaging health stub.",
                },
            )
            return
        if self.path.startswith("/internal/models/"):
            _json(
                self,
                404,
                {
                    "error": {
                        "code": "MODEL_NOT_TRAINED",
                        "message": "No model artifact is loaded.",
                        "retryable": False,
                    }
                },
            )
            return
        _json(self, 404, {"error": {"code": "NOT_FOUND", "message": self.path}})

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("content-length", "0") or 0)
        _ = self.rfile.read(length) if length else b""
        if self.path == "/internal/predict":
            _json(
                self,
                200,
                {
                    "status": "ABSTAIN",
                    "reason": "MODEL NOT TRAINED",
                    "prediction_id": None,
                    "outputs": None,
                },
            )
            return
        if self.path == "/internal/jobs":
            _json(
                self,
                503,
                {
                    "error": {
                        "code": "WORKER_UNAVAILABLE",
                        "message": "Training/extraction worker is not implemented.",
                        "retryable": False,
                    }
                },
            )
            return
        _json(self, 404, {"error": {"code": "NOT_FOUND", "message": self.path}})


def main() -> None:
    server = ThreadingHTTPServer(BIND, Handler)
    print(f"ml-worker stub listening on {BIND[0]}:{BIND[1]} status=MODEL_NOT_TRAINED")
    server.serve_forever()


if __name__ == "__main__":
    main()
