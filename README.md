# TypeSafe Rust SDK — migration milestone 1

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

See [migration decisions and remaining scope](migration/MIGRATION.md),
[comparison rules and fixture provenance](compat/README.md), and
[executed commands/results](migration/VERIFICATION.md).

## Local verification

Use a clean Python checkout at revision
`2ce5c65f13646cab6e6f782328194c9d85f3300a` in `../typesafe-sdk-python`.
Python is only needed by the development oracle harness. The existing reference
checkout must remain unchanged.

```sh
uv sync --locked --project ../typesafe-sdk-python
cargo build --locked --example compat_adapter
uv run --no-project python compat/run.py --python-repo ../typesafe-sdk-python
uv run --no-project python compat/demonstrate_mismatch.py --python-repo ../typesafe-sdk-python
cargo test --locked --all-targets --all-features
cargo test --locked --doc
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
```

The harness uses loopback servers and disposable credentials, clears SDK/proxy
environment variables, and executes both SDKs against the same scripts. Normal
checks never write the oracle. No production API call is needed.
