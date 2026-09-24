# TypeSafe Rust SDK

A native async Rust client for the TypeSafe API: System One judgments
(`POST /v1/systemone`) and model listing (`GET /v1/models`). Behavior follows the
Python SDK where [characterized](compat/README.md). Blocking calls, SDK logging,
custom transports, custom retry predicates, pagination and streaming are not
implemented. [CONTEXT.md](CONTEXT.md) defines the domain terms.

## Setup

Requires Rust 1.96+ and a Tokio runtime with I/O and time enabled. To use a local
checkout from another crate, add these dependencies (adjust the SDK path):

```toml
[dependencies]
typesafe-sdk = { path = "../typesafe-sdk-rust" }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "time"] }
```

Set `TYPESAFE_API_KEY` in your environment, then run:

```rust,no_run
use serde_json::json;
use typesafe_sdk::{Client, SystemOneRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().build()?; // TYPESAFE_API_KEY
    let request = SystemOneRequest::from_json(
        json!("My payment failed"),
        json!({
            "urgent": {"type": "noul", "instructions": "Is this urgent?"}
        }),
    )?;
    let response = client.system_one(&request).await?;
    for (name, probability) in response.data.nouls() {
        println!("{name}: {probability}");
    }
    println!("Request ID: {:?}", response.metadata.request_id());
    Ok(())
}
```

List the available models with the same client:

```rust,no_run
use typesafe_sdk::Client;

async fn list_models(client: &Client) -> Result<(), typesafe_sdk::Error> {
    let response = client.list_models().await?;
    for model in response.data.models {
        println!("{}: {} ({})", model.name, model.description, model.release_date);
    }
    Ok(())
}
```

Listing sends no request body, preserves the server's model order and duplicates,
and ignores unknown fields. All three metadata fields are required strings;
`release_date` is not parsed as a date.

## Building requests

`SystemOneRequest::from_json(state, questions)` converts JSON literals into typed
Noul, Choice and Score questions and validates them before any HTTP. State is text,
an object or an array, such as `json!({"message": "Help", "attempts": 3})`.
`SystemOneRequest::new` builds the same request from typed values.

For API extensions, use `SystemOneRequest::from_raw_json(state, questions)`. It
preserves unknown question kinds, extra fields and explicit nulls; only minimal
shape checks run locally, and the API validates the rest. Typed questions can be
interpolated into the same `json!` object.

Omit a key to omit a field; `null` (including interpolated `None`) sends explicit
null. Duplicate JSON keys keep the last value. For custom fallible serialization,
use `serde_json::to_value(data)?`, because [`json!` can panic on serialization
failures](https://docs.rs/serde_json/1.0.151/serde_json/macro.json.html).
Nonfinite floats can become null during serialization, so validate them first
when the distinction matters.

`request.model` overrides the client's model, and `request.extra_body` adds shallow
final overrides that can replace validated fields. `client.system_one_body(&request)?`
previews the effective body without HTTP or transport headers. The preview contains
your evidence and is never logged automatically.

The [request guide](docs/requests.md) covers validation rules, input error paths
and how to act on answers.

## Reading responses

`client.system_one` returns the standard `SystemOneResponse`. Look answers up by
name in `answers`, or iterate them with `nouls()`, `choices()` and `scores()`.
Future answer kinds are omitted from `answers` but kept in `metadata.body`.
Score-map keys accept integer decimal spellings, surrounding whitespace and digit
separators without rounding. Counts and score keys are limited to `i64` and
probabilities use `f64`; nonstandard NaN/Infinity literals are rejected.
Validation errors report the failing field path, choosing the first error in the
Python SDK's schema/wire order.

To decode into your own Serde type, call `client.system_one_as::<T>(&request)` or
`system_one_as_with`. `T` describes the complete wire JSON, and its Serde
implementation controls validation: no standard answer filtering or Python-style
answer lifting applies, and field paths follow Serde, so missing fields identify
their containing object. Use `system_one` for the standard decoder, even when the
desired type is `SystemOneResponse`.

Every `Response<T>` carries `metadata` with the HTTP status, headers, raw body and
`request_id()`.

## Errors

Every failure is an `Error` with an `ErrorKind`. Request-construction failures have
`ErrorKind::Input`, and `error.input_details()` gives a stable reason and a JSON
Pointer such as `/questions/team/criteria/billing`. HTTP and response-validation
failures carry an `ApiError` in `error.api`, with metadata, the parsed body, the
field path and `retry_after`, the server's requested wait.

Default `Display` and `Debug` output excludes submitted values, user-defined keys
and server payloads. Pointers, error sources, raw bodies and headers are available
explicitly; treat them as sensitive.

## Configuration

Client builder options take precedence over trimmed, nonempty environment values:

| Setting | Environment variable | Default |
|---|---|---|
| API key | `TYPESAFE_API_KEY` | Required |
| Base URL | `TYPESAFE_BASE_URL` | `https://api.typesafe.ai` |
| Model | `TYPESAFE_DEFAULT_MODEL` | `jev-latest` |

`ClientBuilder::timeout` limits each attempt, including the response body, and
defaults to 10 seconds. The default `RetryPolicy` allows two additional attempts on
connection errors, timeouts, HTTP 408/429 and HTTP 5xx, honoring server-requested
delays, with a 30-second retry admission budget. That budget does not interrupt an
in-flight attempt. Use `RetryPolicy::disabled()` to disable retries, or wrap a call
in `tokio::time::timeout` for a total deadline. `system_one_with`,
`system_one_as_with` and `list_models_with` accept per-call `RequestOptions` for
headers, timeout and retries; authentication and SDK identity headers cannot be
overridden.

Clients are cheap to clone and share one connection pool. Dropping a call's future
cancels it; no SDK task keeps retrying in the background.

## Examples

[question_kinds.rs](examples/question_kinds.rs) asks one question of each kind.
[support_triage.rs](examples/support_triage.rs) shows stable criteria,
deadline/retry controls, usage reporting and application-owned decisions with a
review branch. Both make live calls when run with `cargo run --example <name>`;
their automated checks use local fixtures.

## Development

These checks need no API credentials. CI runs them on Linux, macOS and Windows
with Rust 1.96:

```sh
cargo fmt --check
cargo check --locked --all-targets
cargo test --locked --all-targets --all-features
cargo test --locked --doc
cargo clippy --locked --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
```

`tests/tls.rs` checks HTTPS against loopback servers without modifying the system
trust store. Untrusted certificates are rejected on every platform; trusted-CA
and hostname checks run on Linux only. The [fixture notes](tests/fixtures/tls/README.md)
explain how to regenerate the certificates.

The [compatibility harness](compat/README.md) compares this SDK with the pinned
Python SDK and needs a separate Python checkout.
