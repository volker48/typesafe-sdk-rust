# SDK flow verification

[TesterArmy e2e](https://github.com/tester-army/e2e) runs this SDK's tests with a
`custom` target. No browser engine is needed: the product exposes Rust calls and
command-line examples. All API responses come from local loopback HTTP servers.
No TypeSafe or model-provider credentials are required.

From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test:e2e
```

Prerequisites: Rust 1.96+, Node 22.12+, pnpm 11.17.0, and `uv` with an available
Python 3.10+. Dependency installation disables lifecycle scripts and enforces a
24-hour minimum release age. The Python helper uses only the standard library.

| Tests                                            | Flows and assertions                                                                                                                                                                                                                                                               |
| ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `compatibility.e2e.ts`: System One, 40 scenarios | Noul/Choice/Score, typed requests, structured state, omitted/null fields, environment and explicit configuration, header protection, raw body overrides, success/error metadata, HTTP status categories, malformed responses, retry recovery/exhaustion/budgets and per-call reset |
| Foundation, 68 scenarios                         | Exact score-key parsing and collisions, response validation paths and error ordering, alternate successful statuses, retry headers/delay precedence and request sequences                                                                                                          |
| Models, 67 scenarios                             | Bodyless GET, server order and duplicate models, required metadata strings, unknown fields, per-call headers, malformed envelopes, errors and retries                                                                                                                              |
| Extensions, 40 scenarios                         | Raw question extensions, local rejection, custom Serde response schemas, validation, metadata, status errors and mixed operation sequences                                                                                                                                         |
| `examples.e2e.ts`: 7 tests                       | Question-kinds example's three answer kinds and metadata; support triage's billing, technical, low-confidence and unknown-team decisions plus routine-case review; both programs fail without credentials                                                                          |
| `native.e2e.ts`: 4 tests                         | Runs the native request, response, client and TLS integration suites, including typed/JSON/raw constructors, body preview, input diagnostics/redaction, custom response metadata, concurrent options, per-attempt timeouts, cancellation and certificate rejection                 |

The 215 compatibility scenarios reuse the existing recorded Python oracle through
`compat/e2e.py`. Every scenario starts a fresh server and compiled Rust adapter.
The helper checks provenance and the input-file SHA-256, exact decimal tokens,
duplicate request headers, ordered requests and outcomes, raw response bytes,
and unconsumed or unexpected responses. It never updates the oracle. This is a
check against recorded Python behavior; live Python re-characterization still
uses the [cross-language harness](../../compat/README.md).

Native suites retain their own assertions and also contribute verdicts to the
e2e report. TLS trust and hostname tests run on Linux; certificate rejection runs
on every supported platform. This suite does not establish live service behavior
or coverage for unimplemented blocking, streaming, compression or logging APIs.

Reports are written to `.e2e/report.json`. CI runs the suite on Linux alongside
the existing Rust platform matrix. To run a single file or scenario after building:

```sh
cargo build --locked --examples
pnpm exec e2e run tests/e2e/examples.e2e.ts
pnpm exec e2e run tests/e2e/compatibility.e2e.ts --grep retry_recover
```

Set `E2E_TELEMETRY_DISABLED=1` to disable the runner's anonymous telemetry.
The SDK subprocesses receive only fixture credentials and a controlled environment.
See [compatibility limits](../../compat/README.md#deliberate-differences-from-python)
for the characterized contract and deliberate Python differences.
