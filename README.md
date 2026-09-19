# TypeSafe Rust SDK

A standalone, safe Rust implementation of `POST /v1/systemone` and `GET /v1/models`.
Production code has no Python dependency. This is **not yet a complete port** of
the Python SDK.

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

List the available models with the same client:

```rust,no_run
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

For API extensions, use `SystemOneRequest::from_raw_json(state, questions)`.
It preserves unknown question kinds, extra fields, and explicit nulls. State and
question object shapes still validate locally; each question needs a nonempty
string `type`, choice/score need `criteria`, and score criteria must be nonempty
under Python's raw-question rules. Remaining field validation belongs to the API.
Typed questions can be interpolated into the same `json!` object.

Decode a response into your own Serde type with
`client.system_one_as::<T>(&request).await?`, or use
`system_one_as_with::<T>(&request, &options)` for per-call options. The result is
`Response<T>` with the same metadata, errors, and retry behavior. `T` describes
the complete wire JSON: its Serde implementation controls validation and unknown
fields. No standard answer filtering or Python-style answer lifting is applied.
Use `system_one` for the standard decoder, including when the desired type is
`SystemOneResponse`. See the [extension contract and evidence](compat/EXTENSIONS.md).

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
paths and chooses errors in Python schema/wire order for standard responses.
Custom responses use Serde paths and error selection; missing fields identify
their containing object. `ApiError::retry_after`
exposes the server's requested wait as an optional `Duration`.

See the [compatibility checks](compat/README.md),
[foundation follow-up record](compat/FOUNDATION.md), and
[model-listing record](compat/MODELS.md) for verified behavior and
remaining work.
