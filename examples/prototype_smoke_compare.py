"""THROWAWAY: compare two smoke binaries against a local HTTP fixture.

Run with: uv run --no-project python examples/prototype_smoke_compare.py BASE NEW
Both arguments are repository directories. Only synthetic credentials are used.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer


def capture(repository: Path) -> dict:
    """Build and run a smoke binary, returning its captured request body."""
    captured = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            captured.append(
                {
                    "path": self.path,
                    "body": json.loads(
                        self.rfile.read(int(self.headers["Content-Length"]))
                    ),
                }
            )
            body = b'{"model":"fixture","usage":{"input_tokens":1},"answers":{}}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args):
            pass

    with HTTPServer(("127.0.0.1", 0), Handler) as server:
        server.timeout = 60
        worker = threading.Thread(target=server.handle_request, daemon=True)
        worker.start()
        env = {
            k: v for k, v in os.environ.items() if not k.startswith("TYPESAFE_")
        }
        env.update(
            TYPESAFE_API_KEY="prototype-fixture-key",
            TYPESAFE_BASE_URL=f"http://127.0.0.1:{server.server_port}",
            TYPESAFE_DEFAULT_MODEL="prototype-model",
            NO_PROXY="127.0.0.1",
        )
        subprocess.run(
            ["cargo", "run", "--locked", "--quiet", "--bin", "sdk_smoke"],
            cwd=repository,
            env=env,
            check=True,
            capture_output=True,
            timeout=60,
        )
        worker.join(timeout=5)
    if len(captured) != 1:
        raise RuntimeError(f"Expected one request; observed {len(captured)}")
    return captured[0]


before = capture(Path(sys.argv[1]).resolve())
after = capture(Path(sys.argv[2]).resolve())
if before != after:
    raise RuntimeError("Smoke request changed")
print(json.dumps({"smoke_http_bodies_equal": True, "path": after["path"],
                  "question_names": sorted(after["body"]["questions"])}))
