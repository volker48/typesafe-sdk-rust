# TypeSafe Rust SDK

A standalone, safe Rust implementation of `POST /v1/systemone`. Production code has
no Python dependency. This is **not yet a complete port** of the Python SDK.

```rust,no_run
use serde_json::json;
use typesafe_sdk::{Client, SystemOneRequest};

async fn example() -> Result<(), Box<dyn std::error::Error>> {
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

Requires Rust 1.96+ and a Tokio runtime with I/O and time enabled. The client owns
and shares its connection pool across cheap clones. Dropping a call's future
cancels it; no SDK worker keeps retrying in the background.

Supports Noul, Choice and Score, structured state, explicit omitted/null fields,
client and per-call configuration, protected authentication headers, retries,
HTTP metadata, and structured errors. SDK errors expose server payloads explicitly;
ordinary error formatting excludes them. Treat raw bodies and headers as sensitive.

`SystemOneRequest::from_json` converts JSON literals into typed questions and
validates locally before HTTP. Structured state uses the same syntax:
`json!({"message": "Help", "attempts": 3})`. Existing typed construction through
`SystemOneRequest::new` remains available.

Input failures have `ErrorKind::Input`. Use `error.input_details()` for a stable
reason and a JSON Pointer such as `/questions/team/criteria/billing`; default
error formatting excludes submitted values and user-defined keys. Pointers and
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

Read the [interface design](docs/request-ergonomics.md) for contracts and tradeoffs,
the [implementation record](docs/plans/request-ergonomics.md) for acceptance gates,
and [CONTEXT.md](CONTEXT.md) for domain terms.

Score keys accept integer decimal spellings, surrounding whitespace, and digit
separators without rounding. Counts and score keys remain limited to `i64`;
nonstandard NaN/Infinity JSON literals are rejected. HTTP validation reports nested
paths and chooses errors in Python schema/wire order. `ApiError::retry_after`
exposes the server's requested wait as an optional `Duration`.

See the [compatibility checks](compat/README.md) and
[foundation follow-up record](compat/FOUNDATION.md) for verified behavior and
remaining work.
