# Model-listing milestone — 2026-09-18

Continues the [foundation follow-up](FOUNDATION.md) with the second operation in
the clean Python SDK 0.7.0 reference at
`2ce5c65f13646cab6e6f782328194c9d85f3300a`.

## Implemented

- `Client::list_models()` and `list_models_with(&RequestOptions)` return
  `Response<ListModelsResponse>`. `ModelMetadata` has required string fields
  `name`, `description`, and `release_date`. Dates remain server strings, without
  calendar validation; order and duplicate models are preserved in a `Vec`.
- Both operations use one private transport implementation for endpoint joining,
  authentication and identity headers, per-call options, retries, timeouts,
  response metadata and errors. The client stores the base URL, and each request
  selects its own method, path and decoder. No public transport abstraction or
  dependency was added.
- Listing sends a bodyless `GET /v1/models`. Unlike JSON POSTs, it adds no
  Content-Type; an explicit caller Content-Type is retained, matching Python.
  Base-path prefixes work. Redirects and reqwest's own retries remain disabled.
- Model decoding rejects missing/null/wrongly typed fields, ignores unknown
  fields, and reports the first failure in schema field order and array order.
  Errors include paths such as `models[1].name`, GET endpoint context, request ID,
  raw bytes, parsed body and a source. Ordinary formatting omits server values.
- Native concurrency, timeout recovery/opt-out and retry cancellation tests now
  exercise listing as well as System One. Invalid per-call options fail before
  network I/O. Public types support Serde round trips without HTTP metadata.

The existing total-attempt timeout, numeric limits, runtime and ownership
contracts remain in force. Phase timeouts, blocking use and custom transport do
not block this operation and have not been added implicitly.

## Independent compatibility evidence

`models_cases.json` contains 67 new input scenarios. `models_expected.json` was
recorded by executing Python's public `client.models.list()` against disposable
loopback servers before comparing Rust. Its provenance records the reference
revision, exact input hash and recording command. Neither earlier input suite nor
its expected observations was changed.

The Python adapter dispatches to the actual public operation; the Rust adapter
calls `Client::list_models_with`. The request recorder supports GET and records
its exact body bytes as `raw_hex`. A separate harness test demonstrates that an
absent body and literal JSON null remain distinguishable. Existing POST
observation formatting is unchanged.

| Cases / tests | Behavior checked |
|---|---|
| `models_round_trip`, `empty`, `extra_fields`, `date_is_string`, `empty_strings` | required strings, empty lists, order/duplicates, ignored extras, unparsed dates |
| `models_base_prefix`, `models_headers` | URL joining, protected identity/authentication, custom headers and bodyless GET |
| missing/wrong-type/root/entry/field-order/array-order cases | precise paths, strict validation and first-error selection |
| malformed JSON, empty body, status 201/202/204/299 | decoding across 2xx, including rejection of absent 204 bodies |
| status 301/400/401/403/404/408/418/422/429/500/529 | shared error mapping and metadata, no redirect following |
| retry/disconnect/custom-status/budget cases | real HTTP retry sequences, transport failures and policy overrides |
| `models_mixed_calls` | one client alternates GET/POST, resets retry headers and isolates per-call options |
| `tests/sdk.rs`, `tests/foundation.rs` | public metadata/types, redaction, invalid options, concurrent GET/POST, timeouts and cancellation |

## Verification

```sh
cargo build --locked --example compat_adapter
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --cases compat/foundation_cases.json --expected compat/foundation_expected.json
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --cases compat/models_cases.json --expected compat/models_expected.json
uv run --no-project python compat/demonstrate_mismatch.py --python-repo ../typesafe-sdk-python
uv run --no-project python compat/demonstrate_mismatch.py --python-repo ../typesafe-sdk-python --operation models
cargo fmt --check
cargo test --locked --all-targets --all-features
cargo test --locked --doc
cargo clippy --locked --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
cargo check --locked --no-default-features
uv run --locked --project ../typesafe-sdk-python pytest compat -v
uv run --locked --project ../typesafe-sdk-python ruff format --check compat
uv run --locked --project ../typesafe-sdk-python ruff check compat
uv run --locked --project ../typesafe-sdk-python ty check compat
env -u TYPESAFE_API_KEY -u TYPESAFE_BASE_URL -u TYPESAFE_DEFAULT_MODEL -u TYPESAFE_LOG_LEVEL uv run --locked --project ../typesafe-sdk-python pytest ../typesafe-sdk-python/tests
git diff --check
```

All **175 shared scenarios** passed (40 original + 68 foundation + 67 models),
with no skips. **19 Rust tests, 2 doctests and 16 Python characterization/harness
tests** passed. Formatting, Clippy, rustdoc, no-default-features compilation, Ruff,
Python type checking and whitespace checks passed. Both deliberate wrong-endpoint
mutations were rejected in disposable crate copies with production source and
oracles unchanged.

The unchanged upstream Python suite reported **564 passed, 52 skipped**: 35 need
live credentials and 17 need unavailable private sync tooling. Reference source
hashes and its clean Git status were checked; both existing input/expected suites
and Cargo.lock were verified byte-for-byte against Git HEAD. New logs are under
local, gitignored `migration/evidence/models-*`.

An independent Claude review was attempted using the installed `claude-review`
skill's read-only CLI fallback. It exited with the account's monthly spend-limit
error and produced no review findings. Independent review is therefore
unavailable for this milestone; it is not counted as a passed check. The failure
output is retained at `migration/evidence/models-claude-review.log`. The final
diff was inspected locally.

## Remaining scope

This completes model listing, not the entire SDK migration. The subsequent
[extension milestone](EXTENSIONS.md) adds custom Serde response decoding and raw
question construction. Optional blocking use and opt-in logging remain later
milestones. Broader malformed-input parity, advanced retry predicates,
phase timeouts/custom transport, expanded date/header handling, compression,
cookies and error-message extraction remain as documented in the foundation.
No pagination or streaming is introduced: neither pinned Python operation has it.
Only macOS arm64 with Rust 1.96/Tokio/Rustls has been exercised here; Linux/Windows,
local TLS verification and platform/feature CI remain open. No production API
requests, credentials, publication or push were used.
