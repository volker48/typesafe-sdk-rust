"""Prove the oracle rejects a wrong endpoint in an isolated disposable crate copy."""

import argparse
import hashlib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--python-repo", type=Path, required=True)
    parser.add_argument(
        "--operation", choices=["system_one", "models"], default="system_one"
    )
    args = parser.parse_args()
    models = args.operation == "models"
    endpoint = "/v1/models" if models else "/v1/systemone"
    case = "models_round_trip" if models else "round_trip_raw"
    cases = ROOT / ("compat/models_cases.json" if models else "compat/cases.json")
    expected = ROOT / (
        "compat/models_expected.json" if models else "compat/expected.json"
    )
    source = ROOT / "src/client.rs"
    before = hashlib.sha256(source.read_bytes()).hexdigest()
    oracle = hashlib.sha256(expected.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix="typesafe-mismatch-") as directory:
        root = Path(directory)
        for name in ("Cargo.toml", "Cargo.lock"):
            shutil.copy2(ROOT / name, root / name)
        for name in ("src", "examples"):
            shutil.copytree(ROOT / name, root / name)
        changed = root / "src/client.rs"
        text = changed.read_text()
        assert text.count(endpoint) == 1
        changed.write_text(text.replace(endpoint, "/v1/intentional-mismatch"))
        target = ROOT / "target/mismatch"
        subprocess.run(
            [
                "cargo",
                "build",
                "--locked",
                "--manifest-path",
                str(root / "Cargo.toml"),
                "--target-dir",
                str(target),
                "--example",
                "compat_adapter",
            ],
            check=True,
        )
        result = subprocess.run(
            [
                sys.executable,
                str(ROOT / "compat/run.py"),
                "--python-repo",
                str(args.python_repo.resolve()),
                "--rust-bin",
                str(target / "debug/examples/compat_adapter"),
                "--case",
                case,
                "--cases",
                str(cases),
                "--expected",
                str(expected),
            ],
            text=True,
            capture_output=True,
            check=False,
        )
        print(result.stdout + result.stderr)
        assert result.returncode != 0
        assert f"Compatibility mismatch: {case}" in result.stderr
        assert "/v1/intentional-mismatch" in result.stderr
    assert hashlib.sha256(source.read_bytes()).hexdigest() == before
    assert hashlib.sha256(expected.read_bytes()).hexdigest() == oracle
    print(
        "PASS: wrong-path mutation rejected; disposable source deleted; production source and oracle unchanged"
    )


if __name__ == "__main__":
    main()
