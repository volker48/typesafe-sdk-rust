# Compatibility harness

Runs this SDK and the Python SDK against the same scripted loopback HTTP server
and requires identical observations. It needs a checkout of the Python reference;
ordinary Rust development and CI do not use it.

## Files

Each suite pairs scenario inputs with observations recorded from Python:

| Suite | Cases | Expected | Scenarios |
|---|---|---|---|
| Base | `cases.json` | `expected.json` | 40 |
| Foundation | `foundation_cases.json` | `foundation_expected.json` | 68 |
| Model listing | `models_cases.json` | `models_expected.json` | 67 |
| Extensions | `extensions_cases.json` | `extensions_expected.json` | 40 |

Case files hold inputs and scripted server responses, never expected SDK output.
Expected files are recorded only by executing the Python public API. Each records
the reference revision, the SHA-256 of its case file and the recording command,
and verification refuses changed inputs.

- `run.py` runs a suite: it re-executes Python to detect oracle drift, then runs
  Rust and diffs the observations.
- `python_adapter.py` and `../examples/compat_adapter.rs` translate scenario JSON
  into public SDK calls and report observations. All request preparation,
  transport, decoding and retries happen in the SDKs.
- `test_harness.py` checks the comparison rules themselves.

The reference is Python SDK 0.7.0 at `2ce5c65f13646cab6e6f782328194c9d85f3300a`;
`run.py` refuses any other revision or a dirty checkout. The `round_trip_*`
scenarios use the `RESULT` literal and question/state examples from Python's
`tests/test_clients.py::test_round_trip`; the rest derive from documented public
behavior.

## Running

```sh
cargo build --locked --example compat_adapter
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --case round_trip_raw
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --cases compat/foundation_cases.json --expected compat/foundation_expected.json
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --cases compat/models_cases.json --expected compat/models_expected.json
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --cases compat/extensions_cases.json --expected compat/extensions_expected.json
uv run --locked --project ../typesafe-sdk-python pytest compat -v
```

Re-recording an oracle is a separate, reviewed change made after inspecting the
new Python observations. Never record to make a Rust mismatch pass. Add
`--record` to a full-suite command, for example:

```sh
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --cases compat/foundation_cases.json --expected compat/foundation_expected.json --record
```

## Execution

Each SDK gets a fresh loopback server, process and client per scenario. Calls
within a scenario share one client and consume a single ordered response script;
unexpected requests and unconsumed responses fail. Each adapter process has a
30-second deadline. The runner fixes the hash seed, controls the SDK environment
and proxies, and passes only fixture credentials.

Calls with `operation: "list_models"` use Python's `client.models.list` and Rust's
`list_models_with`; other calls use `system_one` and `system_one_with`. Call flags
select alternate paths:

- `typed` builds Python questions with its typed constructors. Rust always builds
  typed questions unless `raw_questions` is set.
- `raw_questions` selects Rust's `from_raw_json`. Python already accepts
  dictionaries.
- `custom_response` decodes into equivalent caller-defined Serde and strict
  Pydantic models.
- `observe_retry_after` records the rate-limit delay in milliseconds.

## Comparison contract

Compare the ordered request sequence and the ordered call outcomes.

For each request, compare method, path, ordered query pairs, request headers and
the semantic JSON body. Duplicate header values are kept in order. GET requests
also record exact body bytes, so an absent body differs from JSON null. Object
member order and insignificant whitespace in request bodies are not contractual.

Response object order is kept in fixtures, because it decides which invalid map
entry is reported first and which colliding score key wins. Numbers are compared
as decimal tokens, without binary-float rounding or tolerance. Trailing decimal
zeros are normalized, and the sign of zero is kept. Booleans stay distinct from
numbers.

For successes, compare serialized response data, status, every response header
and the raw body bytes. For errors, compare the category, status, body, headers,
request ID, field path and sanitized endpoint; error prose is not compared.
Custom-response successes compare data only, because standalone Pydantic models
expose no HTTP metadata; Rust tests cover that metadata natively. Local
construction failures compare only the error category.

Only these details are normalized:

- The random local port becomes `<origin>` in error endpoints, and `Host` is
  excluded.
- User-Agent, SDK and runtime identity headers are first checked against each
  language's required values, then replaced with a common marker.
- Request `Content-Length`, `Connection` and `Accept-Encoding` are HTTP-library
  details and are excluded.

Retries use zero backoff or fixed Retry-After values, and retry budgets leave a
wide margin (a 3-second delay against a 0.1-second budget), so no comparison
depends on wall-clock timing. The future-date Retry-After fixture in the
foundation suite uses 2099 and must be updated before then.

Shared coverage excludes randomized backoff timing, timeout-triggered retries,
cancellation, TLS, compression, cookies, streaming, duplicate or non-ASCII
response headers, and query-bearing base URLs. Rust-native tests cover timeouts,
retries, cancellation and TLS.

## Deliberate differences from Python

- Counts and score-map keys are limited to `i64`, and probabilities use `f64`.
  Rust rejects nonstandard NaN/Infinity literals; Python accepts them and larger
  integers.
- Score keys accept integer decimal spellings, surrounding whitespace and
  underscores between digits, converted exactly. Colliding normalized keys keep
  the last wire entry.
- `serde_json`'s `preserve_order` feature is enabled because response map order
  affects error selection and score-key collisions. Through Cargo feature
  unification, this also makes `serde_json::Map` insertion-ordered for consumers
  that share the dependency, including `Content::Object` and `extra_body`.
- Per-attempt timeouts cover total elapsed time through body receipt, unlike
  Python's per-phase timeouts.
- Retry-After parsing reads the first value of each header and standard HTTP dates
  only. Python's numeric separators and broader email-date formats are not
  supported, and unrepresentable delays become `None`.
- A missing request ID returns `None`; Python's accessor raises.
- Custom responses do not lift named answers into top-level fields as Python
  subclasses can. Describe the wire `answers` object in a nested Rust type instead.
