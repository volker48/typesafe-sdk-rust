# Custom responses and raw questions

This milestone adds the two extension boundaries identified after
[model listing](MODELS.md), using the pinned Python SDK 0.7.0 at
`2ce5c65f13646cab6e6f782328194c9d85f3300a` as the behavioral reference.
No dependency or transport policy changed.

## Public contract

`SystemOneRequest::from_raw_json(state, questions)` is an explicit alternative
to strict `from_json` and `new`. State still accepts only text, objects and
arrays. Questions must be a nonempty object of objects. Every question requires
a nonempty string `type`; choice and score require `criteria`. Score rejects
null, false, numeric zero, and empty strings, arrays or objects, matching Python's
raw-question normalization. Unknown kinds, extra fields, and all remaining
field contents pass through unchanged, including explicit nulls. Question names
are sorted as in existing Rust construction; order is not a wire contract.

Raw and typed questions can be mixed by serializing `Question` values inside
the `json!` input. JSON construction has already collapsed duplicate names;
nonfinite numbers can become null, as documented for the strict constructor.
There is no implicit fallback from strict parsing to raw parsing.

Local errors retain `ErrorKind::Input`, structured JSON Pointers, and Serde
causes where applicable. `InputErrorKind::EmptyQuestionKind` identifies an empty
type string. Validation checks state, question-map shape/emptiness, then each
question in sorted name order: object shape, type, required criteria, empty
score criteria. Model inheritance, final `extra_body` overrides, and effective
body preview work with either construction path.

`Client::system_one_as<T: DeserializeOwned>` and `system_one_as_with` decode
the entire response JSON directly into `T` and return `Response<T>`. Both use the
same request rendering and shared transport as standard System One calls.
HTTP status handling, metadata, headers, retries, timeout options and future
cancellation follow that transport. The default policy does not retry validation
failures. Malformed JSON, incomplete JSON and trailing content are rejected.

`T` owns its Serde validation, including required/unknown fields and answer
representation. Custom responses do not undergo standard-envelope checks or
unknown-answer filtering. They do not lift named answers into top-level fields
as Python subclasses can; describe the wire `answers` object in an ordinary
nested Rust struct instead. `system_one_as::<SystemOneResponse>` likewise uses
that type's Serde implementation. Use `system_one`/`system_one_with` for the
standard decoder's Python-compatible behavior.

Custom validation errors preserve status, headers, raw bytes, parsed body,
request ID, endpoint, and an underlying Serde source. Syntax errors have the
empty root path. Data errors use Serde paths: missing fields locate the containing
object, and untagged/custom deserializers can lose nested path detail. Error
selection follows Serde rather than Python schema ordering. Source errors and
raw metadata may contain sensitive data; normal SDK error formatting excludes
response values. These are explicit adaptations, not claims of arbitrary
Pydantic-model parity.

## Evidence

`extensions_cases.json` has 40 scenarios recorded by executing the pinned
Python public API against local fixture servers. Its independent
`extensions_expected.json` records the input hash, reference revision and command.
All previous input/expected files remain unchanged.

- Raw cases cover future kinds, known extensions, nulls, rich state, mixed
  questions, model/body overrides, API validation, and local rejection of
  missing/invalid types or required/empty criteria.
- Custom cases use equivalent caller-defined Serde and strict Pydantic models.
  They cover future answers, absent standard usage/discriminators, nested type
  failures, malformed/trailing/empty JSON, success/error statuses, retries,
  protected headers, and mixed standard/custom/model-listing calls.
- Custom success observations compare response data only: Python's standalone
  Pydantic models do not expose HTTP metadata. HTTP and validation errors still
  compare all metadata. Native tests independently check successful Rust metadata.
- Native public-API tests cover strict/raw separation, preview versus captured
  HTTP bytes, input reasons and escaped paths, missing response fields, literal
  dot keys, cause retention, redaction, retry-option isolation, and invalid options
  rejected before I/O. Existing transport tests cover cancellation and timeouts.

The first public tests failed to compile before their methods existed. A further
raw-validation regression failed before adding Python's minimal question checks.
The new oracle exposed malformed JSON reporting `?` instead of the root path;
a native regression reproduced that failure before the decoder fix. Expected
observations were not changed to satisfy Rust failures.

## Verification

```sh
cargo fmt --check
cargo check --locked --all-targets
cargo check --locked --no-default-features
cargo test --locked --all-targets --all-features
cargo test --locked --doc
cargo clippy --locked --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
cargo build --locked --example compat_adapter
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --cases compat/foundation_cases.json --expected compat/foundation_expected.json
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --cases compat/models_cases.json --expected compat/models_expected.json
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --cases compat/extensions_cases.json --expected compat/extensions_expected.json
uv run --locked --project ../typesafe-sdk-python pytest compat -v
uv run --locked --project ../typesafe-sdk-python ruff format --check compat
uv run --locked --project ../typesafe-sdk-python ruff check compat
uv run --locked --project ../typesafe-sdk-python ty check compat
git diff --check
```

All **215 compatibility scenarios** passed (40 original, 68 foundation,
67 models, 40 extensions), with no skips. **34 Rust tests, 5 doctests and
16 Python characterization/harness tests** passed. Formatting, type checks,
Clippy, rustdoc, no-default-features compilation, Ruff and whitespace checks
passed. The existing oracles and dependency lockfile were checked unchanged,
and the pinned Python checkout remained clean. These checks ran locally on
macOS arm64 with Rust 1.96; other platforms and live service behavior were not
exercised.

Independent Standards and Spec reviews compared the change against starting
commit `ddfaf4f84730c4d607744465ede4a188bd97fe6e` and found no actionable issues
on either axis. Clippy's large-enum warning in the test adapter was resolved by
boxing its System One payload; the adapter test and lint checks then passed.

## Remaining scope

Blocking use, opt-in logging, advanced retry predicates, phase timeouts/custom
transport, broader date/header/error extraction parity, compression/cookies,
platform CI and local TLS verification remain outside this milestone. No live
service calls, production credentials, publication or push are needed for these
checks. Existing numeric limits and async Tokio runtime requirements remain.
