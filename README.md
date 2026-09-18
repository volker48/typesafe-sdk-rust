# TypeSafe Rust SDK

A standalone, safe Rust implementation of `POST /v1/systemone`. Production code has
no Python dependency. This is **not yet a complete port** of the Python SDK.

```rust,no_run
use serde_json::json;
use typesafe_sdk::{Client, SystemOneRequest};

async fn example() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().build()?; // TYPESAFE_API_KEY
    let request = SystemOneRequest::new(
        "My payment failed".into(),
        serde_json::from_value(json!({
            "urgent": {"type": "noul", "instructions": "Is this urgent?"}
        }))?,
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

`json!` supplies familiar literal syntax; `from_value` decodes it into typed
questions, and `SystemOneRequest::new` checks request invariants before HTTP.
For structured state, pass `serde_json::from_value(json!({"message": "Help"}))?`
as the first argument. Typed `Question` construction remains available for Rust
code that assembles questions programmatically.

Omit a key to omit a field; `null` (including interpolated `None`) sends explicit
null. Duplicate JSON keys keep the last value. For custom fallible serialization,
use `serde_json::to_value(data)?`; [`json!` can panic on serialization failures](https://docs.rs/serde_json/1.0.151/serde_json/macro.json.html).
The full three-question example is in [sdk_smoke.rs](src/bin/sdk_smoke.rs).

For construction tradeoffs, validation contracts, and the proposed inspection
workflow, read the [interface design](docs/request-ergonomics.md). To implement
the next changes, follow the [plan and acceptance gates](docs/plans/request-ergonomics.md).
Domain terms are defined in [CONTEXT.md](CONTEXT.md). Proposed methods in these
documents are explicitly marked; they are not shipped APIs.

Score keys accept integer decimal spellings, surrounding whitespace, and digit
separators without rounding. Counts and score keys remain limited to `i64`;
nonstandard NaN/Infinity JSON literals are rejected. HTTP validation reports nested
paths and chooses errors in Python schema/wire order. `ApiError::retry_after`
exposes the server's requested wait as an optional `Duration`.

See the [compatibility checks](compat/README.md) and
[foundation follow-up record](compat/FOUNDATION.md) for verified behavior and
remaining work.
