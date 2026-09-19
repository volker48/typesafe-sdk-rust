# TypeSafe Rust SDK

A standalone, safe Rust implementation of `POST /v1/systemone` and `GET /v1/models`.
Production code has no Python dependency. This is **not yet a complete port** of
the Python SDK.

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

`list_models_with(&RequestOptions)` accepts per-call headers, timeout and retry
overrides. Listing sends no request body and preserves the server's model order
and duplicates. All three metadata fields are required strings; `release_date`
is not parsed or validated as a date. Unknown response fields are ignored.

The client owns and shares its connection pool across cheap clones. Dropping a
call's future cancels it; no SDK worker keeps retrying in the background.

Supports Noul, Choice and Score, structured state, explicit omitted/null fields,
client and per-call configuration, protected authentication headers, retries,
HTTP metadata, and structured errors. SDK errors expose server payloads explicitly;
ordinary error formatting excludes them. Treat raw bodies and headers as sensitive.

`SystemOneRequest::from_json` converts JSON literals into typed questions and
validates locally before HTTP. Structured state uses the same syntax:
`json!({"message": "Help", "attempts": 3})`. Existing typed construction through
`SystemOneRequest::new` remains available.

Request-construction failures have `ErrorKind::Input`. Use `error.input_details()`
for a stable reason and a JSON Pointer such as `/questions/team/criteria/billing`.
Default error formatting excludes submitted values and user-defined keys. Pointers and
underlying Serde sources may contain sensitive input and are accessed explicitly.

`client.system_one_body(&request)?` previews the effective body without HTTP or
transport headers. It includes model selection and `extra_body` overrides; those
raw overrides can replace validated fields. The preview contains your evidence
and is never automatically logged.

Omit a key to omit a field; `null` (including interpolated `None`) sends explicit
null. Duplicate JSON keys keep the last value. For custom fallible serialization,
use `serde_json::to_value(data)?`; [`json!` can panic on serialization failures](https://docs.rs/serde_json/1.0.151/serde_json/macro.json.html).
Nonfinite floats can become null during serialization, so validate those before
conversion when the distinction matters.

See [sdk_smoke.rs](src/bin/sdk_smoke.rs) for all three question kinds, and
[support_triage.rs](examples/support_triage.rs) for stable criteria, deadline/retry
controls, usage reporting, and application-owned decisions with a review branch.
The examples make live calls when run; their automated checks use local fixtures.

Read the [request construction guide](docs/request-ergonomics.md) for validation
and execution contracts, and [CONTEXT.md](CONTEXT.md) for domain terms.

Score keys accept integer decimal spellings, surrounding whitespace, and digit
separators without rounding. Counts and score keys remain limited to `i64`;
nonstandard NaN/Infinity JSON literals are rejected. HTTP validation reports nested
paths and chooses errors in Python schema/wire order. `ApiError::retry_after`
exposes the server's requested wait as an optional `Duration`.

Client builder options take precedence over trimmed, nonempty environment values:

| Setting | Environment variable | Default |
|---|---|---|
| API key | `TYPESAFE_API_KEY` | Required |
| Base URL | `TYPESAFE_BASE_URL` | `https://api.typesafe.ai` |
| Model | `TYPESAFE_DEFAULT_MODEL` | `jev-latest` |

`ClientBuilder::timeout` defaults to 10 seconds for each attempt, including the
response body. The default `RetryPolicy` allows two additional attempts on
connection errors, timeouts, HTTP 408/429, and HTTP 5xx, with a 30-second retry
admission budget. That budget does not interrupt an in-flight attempt. Use
`RetryPolicy::disabled()` to disable retries, or wrap a call in
`tokio::time::timeout` for a total caller deadline. `system_one_with` and
`list_models_with` accept per-call `RequestOptions`.

The SDK currently provides async System One and model listing. Blocking calls,
custom response models, raw question extensions, and SDK logging are not
implemented. Counts and score-map keys use `i64`; probabilities use `f64`.
See the [compatibility guide](compat/README.md) for coverage and deliberate
Python differences.

For local development, the following checks need no API credentials:

```sh
cargo fmt --check
cargo check --locked --all-targets
cargo test --locked --all-targets --all-features
cargo test --locked --doc
cargo clippy --locked --all-targets --all-features -- -D warnings
```

The [compatibility guide](compat/README.md#running-and-updating) documents the
separate Python reference checkout needed for cross-language checks.
