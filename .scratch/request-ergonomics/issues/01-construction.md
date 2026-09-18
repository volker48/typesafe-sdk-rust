# Add validated JSON request construction with repairable diagnostics

Type: task
Status: open

Implement step 1 of the [plan](../../../docs/plans/request-ergonomics.md), using
the [design contract](../../../docs/request-ergonomics.md). The prototype question
is resolved; the proposed public facade and structured input diagnostics remain
unimplemented. The existing smoke example already uses supported JSON conversion.

## Prototype answer

Reuse `serde_json::json!` and convert into typed requests. A map macro addresses
only punctuation; a nested DSL creates another grammar. The construction seam
must also report precise, redacted, machine-readable local failures.

Primary source: local branch `codex/prototype-request-ergonomics`, commit
`c855a53e840c8b1d2ea60d6623288e5e76130442`, based on `fd8d1a0`.

Files on that branch:

- `examples/PROTOTYPE_REQUEST.md`: run instructions, verdict, limitations.
- `examples/prototype_request.rs`: compiled alternatives and 14 fixture scenarios.
- `examples/prototype_request_observations.json`: captured construction outcomes.
- `examples/prototype_request.html`: self-contained interactive walkthrough.
- `examples/prototype_smoke_compare.py` and `prototype_smoke_observations.json`:
  actual original/simplified smoke request comparison through loopback HTTP.

Retrieve without switching branches:

```sh
git show codex/prototype-request-ergonomics:examples/PROTOTYPE_REQUEST.md
git show codex/prototype-request-ergonomics:examples/prototype_request.html > /tmp/prototype_request.html
```

The prototype branch is local and has not been pushed. Retain or publish that
branch alongside this issue when sharing its evidence with another checkout.

## Verification of the example-and-design change

Run on 2026-09-18, Rust 1.96, macOS arm64:

| Check | Observation |
|---|---|
| `cargo fmt --check` | Passed |
| `cargo check --locked --all-targets` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo test --locked --doc` | One compiled example passed |
| Prototype Rust example | Completed; 14 observations; typed/JSON question serialization equal |
| Original vs simplified smoke binary | Actual HTTP paths and semantic bodies equal; three question names preserved |
| Prototype JavaScript | `node --check` passed |
| HTML browser review | Not verified: browser security policy blocked the local-file URL |

The HTTP comparison used local servers, fixed model configuration, and synthetic
credentials. No live inference or model-quality claim is involved. The broader
Python parity suites were not rerun: SDK model and transport implementations are
unchanged. Future facade work must satisfy the plan's production acceptance gates.
