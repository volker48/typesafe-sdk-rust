"""Verify Rust against the recorded oracle without a Python SDK checkout."""

import argparse
import difflib
import hashlib
import json
import os

from run import REVISION, ROOT, canonical, execute

SUITES = {
    "systemone": ("cases.json", "expected.json"),
    "foundation": ("foundation_cases.json", "foundation_expected.json"),
    "models": ("models_cases.json", "models_expected.json"),
    "extensions": ("extensions_cases.json", "extensions_expected.json"),
}


def main() -> None:
    """Execute one scenario and compare every recorded observation.

    Raises:
        AssertionError: Inputs, provenance, or SDK observations differ.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", required=True, choices=SUITES)
    parser.add_argument("--case", required=True)
    args = parser.parse_args()
    cases_name, expected_name = SUITES[args.suite]
    cases_path = ROOT / "compat" / cases_name
    saved = json.loads((ROOT / "compat" / expected_name).read_text())
    provenance = saved["provenance"]
    if (
        provenance["revision"] != REVISION
        or provenance["format"] != 3
        or provenance["cases_sha256"]
        != hashlib.sha256(cases_path.read_bytes()).hexdigest()
    ):
        raise AssertionError("Oracle provenance changed; characterize against Python")
    cases = json.loads(cases_path.read_text())
    matches = [case for case in cases if case["id"] == args.case]
    if len(matches) != 1:
        raise AssertionError(f"Expected exactly one scenario: {args.case}")
    scenario = matches[0]
    # The SDK process receives fixture credentials only, even on a developer's machine.
    env = {
        key: value
        for key, value in os.environ.items()
        if key.upper() in {"PATH", "HOME", "TMPDIR", "TEMP", "TMP", "SYSTEMROOT"}
    }
    env.update(
        {
            "NO_PROXY": "*",
            "PYTHONHASHSEED": "0",
            "TYPESAFE_API_KEY": "  environment-test-key  ",
            "TYPESAFE_DEFAULT_MODEL": " environment-model ",
        }
    )
    env.update(scenario.get("env", {}))
    suffix = ".exe" if os.name == "nt" else ""
    actual = execute(
        scenario,
        [str(ROOT / "target" / "debug" / "examples" / f"compat_adapter{suffix}")],
        env,
        "rust",
    )
    expected = saved["observations"][args.case]
    if canonical(actual) != canonical(expected):
        diff = "\n".join(
            difflib.unified_diff(
                json.dumps(expected, indent=2, sort_keys=True).splitlines(),
                json.dumps(actual, indent=2, sort_keys=True).splitlines(),
                fromfile="recorded Python",
                tofile="Rust",
            )
        )
        raise AssertionError(f"SDK mismatch: {args.case}\n{diff}")
    print(f"PASS {args.case}")


if __name__ == "__main__":
    main()
