# Request construction and execution

Use `SystemOneRequest::from_json` to turn JSON literals into validated typed
questions, or `SystemOneRequest::new` for typed Rust construction. The
[design decision](adr/0001-json-construction.md) records why these interfaces reuse
JSON syntax. [CONTEXT.md](../CONTEXT.md) defines the domain terms.

## Working today

```rust
use serde_json::json;
use typesafe_sdk::SystemOneRequest;

let request = SystemOneRequest::from_json(
    json!({"message": "Payment failed", "attempts": 3}),
    json!({
        "urgent": {
            "type": "noul",
            "instructions": "Does this require a fast response?",
            "criteria": {
                "true": "Blocked payment or a time-sensitive deadline.",
                "false": "Informational request with no deadline."
            }
        }
    }),
)?;
```

The constructor returns the SDK's `Error`, so callers need not combine Serde
and SDK errors just to author a literal. Custom fallible serialization remains
explicit through `serde_json::to_value(data)?`.

## Construction interface

```rust
impl SystemOneRequest {
    pub fn from_json(
        state: serde_json::Value,
        questions: serde_json::Value,
    ) -> Result<Self, Error>;
}
```

Usage is `SystemOneRequest::from_json(json!(state), json!({...}))?`.
Both `from_json` and existing `new(Content, BTreeMap<String, Question>)` converge
on the same semantic validation. `Client::system_one` accepts a typed request.

## Semantic contract

| Concern | Required behavior |
|---|---|
| State and content | Top-level text/object/array; nested JSON may contain scalars and null |
| Question kinds | Known `noul`, `choice`, `score`; reject unknown kinds and fields locally |
| Presence | Missing key = omitted; explicit null = null; supplied content = value |
| Choice criteria | Required object; each label may map to content or null |
| Score criteria | Required nonempty array; preserve order; one level is accepted |
| Empty questions | Reject before HTTP |
| Other restrictions | Preserve today's acceptance of empty choice maps and empty names; syntax convenience must not invent service policy |
| Object order | Do not infer priority from names or insertion order; named typed maps currently sort keys |
| Duplicate names | JSON and already-built maps have discarded duplicates; last value survives |
| Model | Missing request override inherits client configuration |
| `extra_body` | Preserve existing shallow, final overrides, including reserved fields and null |

`json!({"instructions": optional_text})` with `None` emits null. Omit the key when
omission is intended. There is no global “strip null” pass.

For generated question sets requiring unique names, check for duplicates before
collecting them into a map. `from_json(Value, Value)` cannot recover duplicates
that JSON parsing or map construction has already discarded.

[`json!` documentation](https://docs.rs/serde_json/1.0.151/serde_json/macro.json.html)
specifies that interpolated values implement `Serialize`, keys implement
`Into<String>`, and some serialization failures panic. Use
`serde_json::to_value(custom_data)?` for fallible serialization before conversion.
This does not preserve nonfinite floats: JSON cannot represent them, and Serde
can encode them as null. Applications needing to distinguish those values must
validate their numeric data before serialization.

## Error repair is part of the interface

`Error::input_details()` exposes an `InputError`, separate from `ApiError`, with
an `InputErrorKind` reason and an RFC 6901 JSON Pointer in `path`. For example,
invalid content in a choice criterion is identified by
`/questions/q/criteria/billing` with reason `invalid_content`.
Pointers escape `~` and `/` in user-defined names. Missing fields identify the
missing member; array failures identify the index. First-error selection is state,
then questions sorted by name. Each question checks object shape, type, sorted
unknown fields, instructions, then criteria, including semantic validation before
the next question. Noul criteria check sorted unknown fields, true, then false;
choice labels sort by name; score levels follow array order. This local input
order is separate from the existing Python-compatible HTTP response error order.

Default Display/Debug describe the failure without echoing supplied values
or user-defined keys. Access to the pointer and underlying source is explicit:
both can contain caller-controlled data. Keep `ErrorKind::Input` for local
failures, preserve a Serde source where one exists, and do not fabricate HTTP
metadata. Existing HTTP validation paths and compatibility behavior stay intact.

The private `request` module decodes fields individually to avoid enum buffering
while retaining Serde error causes for typed conversions. Semantic checks are
shared with `new`; integration fixtures compare the two construction paths across
presence and content shapes. Failures detected without Serde, such as a missing
field or empty criteria, have no fabricated underlying source.

## Inspection and execution

Effective-body interface:

```rust
impl Client {
    pub fn system_one_body(&self, request: &SystemOneRequest)
        -> Result<serde_json::Value, Error>;
}
```

This computes the effective JSON body without HTTP and without authorization
headers. `system_one_with` calls this same renderer, so preview includes the
resolved model and final `extra_body` overrides. Integration tests compare it
against the actual captured HTTP body. The preview contains evidence and is
explicitly requested, never automatically logged. Request options affect
transport, not this body. Preview is a snapshot; mutation afterward changes what
execution sends.

“Validated request” describes the typed state/questions. It does not guarantee
the effective body is valid after `extra_body` replaces reserved fields. The
existing override behavior is compatibility-sensitive; document it and display
the effective body rather than silently weakening or prohibiting overrides.

Reuse the client across calls. Keep questions stable when changing only evidence;
ask related judgments together when they share state. These reduce setup and
repetition; no claim is made about model accuracy or token savings without an
evaluation. Measure returned model and usage, request ID, and application outcomes.
Use a fixed model identifier when an application needs reproducibility across
model updates, while recognizing that fixed identifiers do not guarantee
deterministic judgments.

Per-attempt timeouts and retry admission budgets are different controls. The
existing default permits two additional attempts; the retry budget does not stop
an in-flight attempt. Callers needing a total deadline can wrap the future in
`tokio::time::timeout`; dropping the future cancels local work, but does not prove
the server did not process or charge for an earlier attempt. Never promise
exactly-once inference or a token/cost ceiling from retry settings alone.

## Interpreting answers

The [support triage example](../examples/support_triage.rs) uses `answers.get(name)`
and matches expected kinds before escalating. Absence or an unexpected kind must
not silently become false, zero, or a default route. Current group iterators are
useful for inspection but do not establish that every requested answer arrived. Future answer kinds are
currently omitted from typed HTTP results and retained in raw metadata.

Probabilities describe judgments; they do not authorize actions or prove
calibration. Application code owns thresholds, abstention, and human review.
Changing score order, question meaning, or criteria is an application behavior
change and should be evaluated with representative labeled cases.
