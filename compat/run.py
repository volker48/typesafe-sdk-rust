"""Local cross-language oracle runner. Recording is explicit and never part of checks."""

import argparse
import copy
import hashlib
import json
import os
import shlex
import subprocess
import sys
import threading
from decimal import Decimal
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import parse_qsl, urlsplit

ROOT = Path(__file__).resolve().parents[1]
REVISION = "2ce5c65f13646cab6e6f782328194c9d85f3300a"


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"))


class NumberToken(str):
    """Keep numeric tokens independent of binary floating point parsing."""


def numbers(text):
    result = []

    def walk(value, path):
        if isinstance(value, NumberToken):
            sign, digits, exponent = Decimal(value).as_tuple()
            assert isinstance(exponent, int), "JSON numeric tokens must be finite"
            digits = list(digits)
            while len(digits) > 1 and digits[-1] == 0:
                digits.pop()
                exponent += 1
            if digits == [0]:
                exponent = 0
            result.append(
                {
                    "path": path,
                    "value": f"{sign}:{''.join(map(str, digits))}e{exponent}",
                }
            )
        elif isinstance(value, dict):
            for key in sorted(value):
                walk(value[key], [*path, key])
        elif isinstance(value, list):
            for index, item in enumerate(value):
                walk(item, [*path, index])

    walk(json.loads(text, parse_int=NumberToken, parse_float=NumberToken), [])
    return result


def execute(scenario, command, env, language):
    unknown = set(scenario) - {"id", "config", "calls", "responses", "base_path", "env"}
    if unknown:
        raise ValueError(f"Unsupported scenario options: {sorted(unknown)}")
    requests = []
    pending = copy.deepcopy(scenario["responses"])
    problems = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            self.handle_request()

        def do_GET(self):
            self.handle_request()

        def handle_request(self):
            raw = self.rfile.read(int(self.headers.get("content-length", "0")))
            relevant = {
                k.lower(): v
                for k, v in self.headers.items()
                if k.lower()
                not in {"host", "content-length", "connection", "accept-encoding"}
            }
            header_values = {name: self.headers.get_all(name, []) for name in relevant}
            identity = (
                "typesafe-sdk/0.7.0"
                if language == "python"
                else "typesafe-sdk-rust/0.1.0"
            )
            for key in ("user-agent", "x-typesafe-sdk", "x-typesafe-runtime"):
                value = relevant.get(key, "")
                valid = (
                    value == identity
                    if key != "x-typesafe-runtime"
                    else (
                        value.startswith("python/")
                        if language == "python"
                        else value == "rust"
                    )
                )
                if not valid or len(header_values.get(key, [])) != 1:
                    problems.append(f"Invalid protected identity {key}: {value}")
                relevant[key] = "<sdk-identity>"
                header_values[key] = ["<sdk-identity>"]
            path = urlsplit(self.path)
            requests.append(
                {
                    "method": self.command,
                    "path": path.path,
                    "query": parse_qsl(path.query, keep_blank_values=True),
                    "headers": relevant,
                    "header_values": header_values,
                    "body": json.loads(raw) if raw else None,
                    "numbers": numbers(raw) if raw else [],
                    # Retain exact GET bytes so absent content cannot equal JSON null.
                    **({"raw_hex": raw.hex()} if self.command == "GET" else {}),
                }
            )
            if not pending:
                problems.append("Unexpected extra request")
                response: dict[str, Any] = {
                    "status": 599,
                    "body": {"error": "script exhausted"},
                }
            else:
                response = pending.pop(0)
            if response.get("disconnect"):
                self.close_connection = True
                return
            body = response.get("raw", None)
            body = (
                body.encode()
                if body is not None
                else json.dumps(
                    response["body"], ensure_ascii=False, separators=(",", ":")
                ).encode()
            )
            self.send_response_only(response["status"])
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            for key, value in response.get("headers", {}).items():
                self.send_header(key, value)
            self.end_headers()
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError) as error:
                problems.append(
                    f"Client closed during scripted response: {type(error).__name__}"
                )

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(
        target=lambda: server.serve_forever(poll_interval=0.01), daemon=True
    )
    thread.start()
    origin = f"http://127.0.0.1:{server.server_port}"
    payload = {"config": scenario.get("config", {}), "calls": scenario["calls"]}
    payload["origin"] = origin
    payload["base_url"] = origin + scenario.get("base_path", "")
    try:
        result = subprocess.run(
            command,
            input=json.dumps(payload),
            text=True,
            capture_output=True,
            check=False,
            env=env,
            timeout=30,
            cwd=ROOT,
        )
        if result.returncode:
            raise AssertionError(f"Adapter failed: {result.stderr}")
        if pending or problems:
            raise AssertionError(f"Unconsumed responses: {len(pending)}; {problems}")
        return {
            "calls": json.loads(result.stdout),
            "numbers": numbers(result.stdout),
            "requests": requests,
        }
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--python-repo", type=Path, required=True)
    parser.add_argument("--record", action="store_true")
    parser.add_argument("--case")
    parser.add_argument("--cases", type=Path, default=ROOT / "compat/cases.json")
    parser.add_argument("--expected", type=Path, default=ROOT / "compat/expected.json")
    parser.add_argument(
        "--rust-bin", type=Path, default=ROOT / "target/debug/examples/compat_adapter"
    )
    args = parser.parse_args()
    repo = args.python_repo.resolve()
    revision = subprocess.check_output(
        ["git", "-C", str(repo), "rev-parse", "HEAD"], text=True
    ).strip()
    if (
        revision != REVISION
        or subprocess.check_output(
            ["git", "-C", str(repo), "status", "--porcelain"], text=True
        ).strip()
    ):
        raise SystemExit("Reference must be the clean pinned revision")
    env = {
        k: v
        for k, v in os.environ.items()
        if not k.startswith("TYPESAFE_")
        and k.lower()
        not in {"http_proxy", "https_proxy", "all_proxy", "no_proxy", "pythonpath"}
    }
    env.update(
        {
            "NO_PROXY": "*",
            "PYTHONHASHSEED": "0",
            "TYPESAFE_API_KEY": "  environment-test-key  ",
            "TYPESAFE_DEFAULT_MODEL": " environment-model ",
        }
    )
    cases_path = args.cases
    cases = json.loads(cases_path.read_text())
    expected_path = args.expected
    saved = {} if args.record else json.loads(expected_path.read_text())
    cases_hash = hashlib.sha256(cases_path.read_bytes()).hexdigest()
    if not args.record and saved["provenance"]["cases_sha256"] != cases_hash:
        raise SystemExit(
            "Case inputs changed: characterize changes against Python explicitly"
        )
    expected = saved.get("observations", {})
    if args.case:
        if args.record:
            raise SystemExit("Cannot record a partial oracle")
        cases = [case for case in cases if case["id"] == args.case]
        if not cases:
            raise SystemExit("Unknown case")
    for scenario in cases:
        scenario_env = env | scenario.get("env", {})
        python = execute(
            scenario,
            [str(repo / ".venv/bin/python"), str(ROOT / "compat/python_adapter.py")],
            scenario_env,
            "python",
        )
        if args.record:
            expected[scenario["id"]] = python
        elif canonical(python) != canonical(expected[scenario["id"]]):
            raise AssertionError(f"Python oracle drift: {scenario['id']}")
        if not args.record:
            rust = execute(
                scenario, [str(args.rust_bin.resolve())], scenario_env, "rust"
            )
            if canonical(rust) != canonical(expected[scenario["id"]]):
                import difflib

                diff = "\n".join(
                    difflib.unified_diff(
                        json.dumps(
                            expected[scenario["id"]], indent=2, sort_keys=True
                        ).splitlines(),
                        json.dumps(rust, indent=2, sort_keys=True).splitlines(),
                        fromfile="python",
                        tofile="rust",
                    )
                )
                raise AssertionError(
                    f"Compatibility mismatch: {scenario['id']}\n{diff}"
                )
        print(f"PASS {scenario['id']}", flush=True)
    if args.record:
        expected_path.write_text(
            json.dumps(
                {
                    "provenance": {
                        "revision": revision,
                        "cases_sha256": hashlib.sha256(
                            cases_path.read_bytes()
                        ).hexdigest(),
                        "command": "uv run --no-project python " + shlex.join(sys.argv),
                        "format": 3,
                        "source": "actual Python public API calls to scripted local HTTP server; decimal tokens preserve precision; header value lists preserve duplicate request headers",
                    },
                    "observations": expected,
                },
                indent=2,
                ensure_ascii=False,
            )
            + "\n"
        )
    print(
        f"{len(cases)} scenarios {'recorded from Python' if args.record else 'matched Python and Rust'}; no skips"
    )


if __name__ == "__main__":
    main()
