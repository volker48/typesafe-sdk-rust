# Add validated JSON request construction with repairable diagnostics

Type: task
Status: resolved

Implement step 1 of the [plan](../../../docs/plans/request-ergonomics.md), using
the [design contract](../../../docs/request-ergonomics.md). The JSON constructor, local diagnostics, effective-body preview, and usage
example are implemented. The prototype remains historical evidence for the
interface decision.

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

The prototype branch is published alongside the implementation PR so reviewers
can retrieve the captured evidence without merging throwaway code into main.

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


## Implementation verification

Based on foundation commit `6a46c7d`; no dependency changes. Public entry points
are `SystemOneRequest::from_json`, `Error::input_details`, and
`Client::system_one_body`. The original typed constructor remains source-compatible.

Red/green evidence:

- The first JSON round-trip test failed to compile because `from_json` did not
  exist, then passed after implementation with equal typed/JSON HTTP bodies.
- The nested diagnostic test first failed on the missing diagnostic API. It now
  verifies escaped JSON Pointers and redaction while retaining Serde causes.
- The shared semantic-validation test failed because typed errors had no local
  details. Both entry points now produce the same reasons and pointers, including
  selecting an earlier empty score before a later malformed question.
- The preview test failed to compile before the method existed, then passed
  against captured HTTP with model, reserved-field, and null overrides.
- The example policy fixture initially returned Review for a confident billing
  judgment; it now passes both escalation branches and missing/wrong/uncertain
  answer cases.

Final checks:

| Check | Result |
|---|---|
| `cargo fmt --check` | Passed |
| `cargo check --locked --all-targets` | Passed |
| `cargo check --locked --no-default-features` | Passed |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo test --locked --all-targets --all-features` | 25 tests passed |
| `cargo test --locked --doc` | 2 doctests passed |
| `RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps` | Passed |
| Original Python/Rust compatibility oracle | 40 scenarios passed, no skips |
| Foundation Python/Rust compatibility oracle | 68 scenarios passed, no skips |
| Original vs final smoke HTTP comparison | Equal semantic bodies and endpoint |
| Local Markdown links and Git whitespace checks | Passed |

Compatibility used the unchanged pinned Python checkout through `compat/run.py`,
with `--cases compat/foundation_cases.json --expected compat/foundation_expected.json`
for the second run. No oracle was re-recorded. Checks used macOS arm64 and Rust
1.96; no live inference, platform expansion, or model-quality evaluation occurred.

## Standards review

No hard standards violations or actionable judgment calls. The independent
review checked precise request types, deterministic diagnostics, redaction,
preserved causes, behavioral tests, and the unchanged foundation semantics.

## Spec review

No actionable spec findings. Steps 1–3 are implemented; conditional typed
constructors/macros remain deferred. Preview/execution share rendering, presence
is preserved, and the usage example retains an explicit application review branch.

Both reviews used `git diff --cached 6a46c7d`, including newly staged files, so the
implementation could be committed after review. The local issue was the explicit
spec source. For automatic issue-tracker discovery in future skill runs, run
`/setup-matt-pocock-skills` to create `docs/agents/issue-tracker.md`.

Review summary: Standards 0 findings; Spec 0 findings; no worst issue on either axis.


## Independent Claude review

The `claude-review` skill supplied an additional read-only review of the staged
implementation. It reported no construction/transport correctness regressions and
four follow-ups:

1. Its claim that `--all-targets` omitted the example test was contradicted by
   both the captured full run and `cargo test --locked --all-targets -- --list`,
   which explicitly lists the support-triage policy fixture. No Cargo target
   configuration change was made.
2. The implementation verification record was finalized with the results above;
   the earlier one-doctest table remains clearly labeled as historical prototype
   verification.
3. `Error::Debug` now includes the redacted `InputError` reason. A regression
   assertion first failed, then passed while all secret-exclusion assertions
   continued to pass.
4. `InputError` is non-exhaustive, matching its reason enum and keeping the
   returned diagnostic extensible without requiring exhaustive downstream matches.

Focused checks and the full Rust suite were rerun after the diagnostic adjustment.
No unresolved actionable review finding remains. The compatibility runs above
preceded this Debug-only adjustment; request/response serialization was unchanged.
