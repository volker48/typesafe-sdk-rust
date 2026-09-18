# Foundation follow-up — 2026-09-18

Continues the next milestone identified by **Establish Rust SDK foundation**:
correctness of the existing operation before adding model listing. The Python
reference remains clean at `2ce5c65f13646cab6e6f782328194c9d85f3300a` (SDK 0.7.0).
The original 40 inputs and recorded observations are unchanged.

## Implemented

- Exact score-key conversion within `i64`: signs, surrounding whitespace,
  underscores between integer digits, and zero fractional parts. No float-based
  integer conversion. Scientific notation, nonzero fractions, malformed separators
  and overflow fail. When keys normalize to the same integer, the last wire entry
  wins, including when its spelling sorts before an earlier entry.
- HTTP response validation in Python's order: check all answer discriminators,
  then schema fields in declaration order, and map entries in wire order. Nested
  score-key errors retain the original key; invalid legend scalars include the
  first union branch (`.str`). Typed successful results retain their existing API.
- `ApiError::retry_after: Option<Duration>` captures the server delay and supplies
  the retry loop. It is available on all HTTP error statuses; invalid or
  out-of-range values are `None`, with nanosecond rounding. This is the Rust equivalent of Python's
  rate-limit `retry_after_ms`, without millisecond arithmetic at call sites.
- Regression evidence for concurrent call options/retry headers, timeout retries
  and opt-out, cancellation of pending retries, and deterministic backoff/date
  calculations.

JSON is parsed with Serde's `preserve_order` feature because map order affects
observable error selection and normalized-key collisions. This adds locked
transitive dependencies indexmap 2.14.2, hashbrown 0.17.1 and equivalent 1.0.2;
versions were checked with `cargo info` against crates.io. No interpreter or new
runtime is introduced. The feature is documented in the
[official serde_json docs](https://docs.rs/serde_json/1.0.151/serde_json/map/index.html).
Cargo unifies dependency features: consumers also get insertion-ordered
`serde_json::Map` in other crates sharing this dependency, including this SDK's
public `Content::Object` and `extra_body`. This observable consequence is accepted
for the current API. A private decoder that retains order without enabling this
feature would avoid that effect, but would require a larger boundary redesign.

## Independent evidence

`foundation_cases.json` is a separate 68-case suite. Its expected observations
were obtained by executing the pinned Python public API against the local server,
before validating Rust. The first comparison failed on `score_key_decimal`:
Python returned an answer while Rust returned a validation error at
`answers.q.legend`. The repaired decoder then passed all 41 initial response
cases. Adding accepted/rejected separator spellings, reversed key collision order,
retry metadata and retry sequences brought the suite to 63 cases. A final probe
found that a literal `"."` choice-probability key was confused with the root path.
That mismatch was reproduced before fixing root detection to inspect structured
path segments. Five punctuation-key cases brought the total to 68; all previous
63 foundation observations were checked unchanged when recording the additions.

The expanded oracle was recorded for those additional inputs, not to make a Rust
failure pass. The runner records the actual command and case-file hash. Original
cases/expected files remain byte-for-byte unchanged. Adapter changes only expose
the SDK's parsed delay; they do not parse headers or implement retry behavior.

| Evidence | Behavior |
|---|---|
| `score_key_*`, `legend_*`, `key_before_value_error` | accepted/rejected key spellings, exact conversion, collision order, nested key/union paths |
| `*_field_order`, `answer_wire_order`, `type_preflight_*` | schema ordering versus wire map ordering, missing versus invalid fields |
| `choice_probability_*` | empty, dot, dotted and bracketed keys retain their validation path |
| `root_*`, `usage_*`, `unknown_type_before_invalid`, `score_probability_type` | strict boundaries and future-answer filtering |
| `success_201/202/204/299` | all 2xx statuses enter decoding; 204's absent body is a validation error |
| `retry_metadata_*`, `retry_sequence_*` | delay exposure, numeric precedence/fallback, invalid/negative/nonfinite headers, past/future dates, retry counts |
| `tests/foundation.rs` | numeric limits/raw bytes, direct Serde keys, response-secret redaction, concurrency, timeout recovery/opt-out, cancellation |
| retry module tests | fixed-clock dates, exponential growth/cap/zero settings, controlled jitter |
| `test_foundation_characterization.py` | six Python observations establishing deliberate numeric differences, including oversized retry delays |

## Verification commands

Run from the Rust repository; SDK requests use loopback HTTP. Documentation and
dependency metadata were read, and Claude was used for code review. No production
credentials, API calls, push or publication.

```sh
cargo build --locked --example compat_adapter
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python --cases compat/foundation_cases.json --expected compat/foundation_expected.json
uv run --no-project python compat/demonstrate_mismatch.py --python-repo ../typesafe-sdk-python
uv run --locked --project ../typesafe-sdk-python pytest compat -v
env -u TYPESAFE_API_KEY -u TYPESAFE_BASE_URL -u TYPESAFE_DEFAULT_MODEL -u TYPESAFE_LOG_LEVEL uv run --locked --project ../typesafe-sdk-python pytest ../typesafe-sdk-python/tests --junitxml=migration/evidence/foundation-python-baseline.xml
cargo fmt --check
cargo test --locked --all-targets --all-features
cargo test --locked --doc
cargo clippy --locked --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
cargo check --locked --no-default-features
uv run --locked --project ../typesafe-sdk-python ruff format --check compat
uv run --locked --project ../typesafe-sdk-python ruff check compat
uv run --locked --project ../typesafe-sdk-python ty check compat
git diff --check
```

Results: **108 shared scenarios passed** (40 original + 68 foundation), with no
skips. **16 Rust tests + 1 doctest** and **15 separate Python tests** passed.
Formatting, Clippy, rustdoc, no-default-features compilation, Ruff, type checking
and whitespace checks passed. The isolated wrong-endpoint mutation was rejected;
the real source and original oracle remained unchanged.

The original Python suite again reported **564 passed, 52 skipped**, zero
failures: 35 skips require live credentials, and 17 require unavailable private
sync tooling. The Python source manifest hashes and clean Git status were checked.
The two original compatibility files were compared byte-for-byte with Git HEAD.

Independent Claude review completed successfully. It independently passed both
compatibility suites, the native tests and 14 additional temporary decoding
probes. Its actionable findings were addressed:

- Reproduced an oversized `retry-after-ms` falling through to a seconds header
  (`Some(2s)` instead of the documented `None`); added a failing public-API native
  regression, then fixed precedence. A separate Python characterization records
  that Python can represent the larger delay, while Rust intentionally cannot.
- Moved the future-date budget fixture from 2030 to 2099. It will need replacement
  before that date. Re-recorded from Python and verified the other 67 foundation
  observations were unchanged.
- Documented the downstream Cargo feature-unification consequence above.

The read-only review used the installed `claude-review` skill's Claude CLI
fallback with Bash/Read/Glob/Grep/LSP tools. Its full output is retained locally at
`migration/evidence/foundation-claude-review.log`. No second review was run after
these focused fixes; the applicable automated checks were rerun.

## Remaining scope and decisions

Counts and score keys intentionally remain `i64`, probabilities `f64`. Rust rejects
nonstandard NaN/Infinity literals and reports explicit validation errors with raw
bytes preserved. Python accepts larger integers and nonfinite literals: native
rejection tests and separate Python characterizations document this difference,
not cross-language parity. Supporting arbitrary integers would change public
types and remains a separate API decision.

The per-attempt timeout remains total elapsed time, rather than Python's separate
HTTP-phase timeouts. Broader Python email-date parsing and numeric separators in
retry headers, arbitrary retry predicates,
duplicate/non-ASCII response-header convenience access, full error-message
extraction, custom transport, cookies/compression and broader lifecycle coverage
remain open. There is no claim of exhaustive malformed-input compatibility.

Only macOS arm64 with Rust 1.96 and Tokio/Rustls is verified. Linux/Windows, local
TLS verification, optional blocking use, custom response types, raw questions and
logging remain later milestones. Model listing is the next operation after the
remaining shared-transport API decisions. No pagination/streaming exists in the
pinned Python operations.
