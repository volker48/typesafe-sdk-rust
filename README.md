# TypeSafe Rust SDK

A standalone, safe Rust implementation of `POST /v1/systemone`. Production code has
no Python dependency. This is **not yet a complete port** of the Python SDK.

```rust,no_run
use std::collections::BTreeMap;
use typesafe_sdk::{Client, Question, SystemOneRequest};

async fn example() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().build()?; // TYPESAFE_API_KEY
    let request = SystemOneRequest::new(
        "My payment failed".into(),
        BTreeMap::from([("urgent".into(), Question::noul("Is this urgent?"))]),
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
HTTP metadata, and structured errors. Errors expose server payloads explicitly;
ordinary error formatting excludes them. Treat raw bodies and headers as sensitive.

Score keys accept integer decimal spellings, surrounding whitespace, and digit
separators without rounding. Counts and score keys remain limited to `i64`;
nonstandard NaN/Infinity JSON literals are rejected. HTTP validation reports nested
paths and chooses errors in Python schema/wire order. `ApiError::retry_after`
exposes the server's requested wait as an optional `Duration`.

See the [compatibility checks](compat/README.md) and
[foundation follow-up record](compat/FOUNDATION.md) for verified behavior and
remaining work.
