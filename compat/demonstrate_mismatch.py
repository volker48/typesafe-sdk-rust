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
    args = parser.parse_args()
    source = ROOT / "src/client.rs"
    before = hashlib.sha256(source.read_bytes()).hexdigest()
    oracle = hashlib.sha256((ROOT / "compat/expected.json").read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix="typesafe-mismatch-") as directory:
        root = Path(directory)
        for name in ("Cargo.toml", "Cargo.lock"):
            shutil.copy2(ROOT / name, root / name)
        for name in ("src", "examples"):
            shutil.copytree(ROOT / name, root / name)
        changed = root / "src/client.rs"
        text = changed.read_text()
        assert text.count("/v1/systemone") == 1
        changed.write_text(text.replace("/v1/systemone", "/v1/intentional-mismatch"))
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
                "round_trip_raw",
            ],
            text=True,
            capture_output=True,
            check=False,
        )
        print(result.stdout + result.stderr)
        assert result.returncode != 0
        assert "Compatibility mismatch: round_trip_raw" in result.stderr
        assert "/v1/intentional-mismatch" in result.stderr
    assert hashlib.sha256(source.read_bytes()).hexdigest() == before
    assert (
        hashlib.sha256((ROOT / "compat/expected.json").read_bytes()).hexdigest()
        == oracle
    )
    print(
        "PASS: wrong-path mutation rejected; disposable source deleted; production source and oracle unchanged"
    )


if __name__ == "__main__":
    main()
