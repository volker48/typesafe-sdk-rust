# Building requests and using answers

This guide covers the rules behind `SystemOneRequest`, how local input errors
identify bad input, and how applications should act on answers. The
[design decision](adr/0001-json-construction.md) records why construction reuses
JSON syntax, and [CONTEXT.md](../CONTEXT.md) defines the domain terms.

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

## Validation rules

`from_json` and `new` apply the same checks. `from_raw_json` checks only the
minimum described in its API documentation and leaves the rest to the API.

| Concern | Rule |
|---|---|
| State and content | Top-level text, object or array; nested JSON may contain scalars and null |
| Question kinds | `noul`, `choice` and `score`; unknown kinds and fields are rejected |
| Presence | A missing key is omitted, explicit null is sent as null, anything else is a value |
| Choice criteria | Required object; each label maps to content or null; an empty object is allowed |
| Score criteria | Required nonempty array, sent in order; a single level is allowed |
| Questions | At least one; empty names are allowed; names are sorted, and order carries no priority |
| Duplicate names | JSON parsing and map construction keep the last value |
| Model | Inherited from the client unless `request.model` is set |
| `extra_body` | Shallow final overrides, including reserved fields and null |

No pass strips nulls: `json!({"instructions": optional_text})` sends null when
`optional_text` is `None`, so omit the key to omit the field. When generated
question names must be unique, check for duplicates before building the map;
`from_json` cannot recover duplicates that JSON parsing has already discarded.

`json!` interpolates any `Serialize` value but panics on some serialization
failures ([documentation](https://docs.rs/serde_json/1.0.151/serde_json/macro.json.html)).
Use `serde_json::to_value(data)?` for fallible serialization. Neither preserves
nonfinite floats, which JSON cannot represent, so validate numeric data first when
that matters.

## Input errors

Construction failures have `ErrorKind::Input`. `Error::input_details()` returns an
`InputError` with an `InputErrorKind` reason and an RFC 6901 JSON Pointer in `path`.
For example, invalid content in a choice criterion is reported at
`/questions/q/criteria/billing` with reason `invalid_content`. Pointers escape `~`
and `/` in user-defined names. Missing fields identify the missing member, and
array failures identify the index. When Serde detected the problem, the error's
`source()` keeps its cause.

The first error is selected deterministically: state, then questions sorted by
name. Each question checks object shape, type, sorted unknown fields, instructions
and then criteria, including semantic checks, before the next question. Noul
criteria check sorted unknown fields, `true` and then `false`; choice labels are
checked in name order and score levels in array order.

`Display` and `Debug` describe the failure without echoing supplied values or
user-defined keys. The pointer and source can contain caller data, so read them
explicitly.

## Previewing and sending

`Client::system_one_body(&request)` renders the JSON body a call would send,
including the resolved model and `extra_body` overrides, without HTTP or
authorization headers. Request options affect transport, not the body. The preview
is a snapshot: later changes to the request affect only later previews and calls.
It contains your evidence and is never logged automatically.

Validation covers the typed state and questions. Once `extra_body` replaces a
reserved field, the effective body is no longer guaranteed to be valid.

Reuse one client across calls. Keep questions stable when only the evidence
changes, and ask related questions together when they share state. Record the
returned model, usage and request ID alongside application outcomes. Pin a model
identifier when an application needs consistency across model updates, knowing
that this does not make judgments deterministic.

Per-attempt timeouts and the retry admission budget are separate controls, and the
budget never stops an in-flight attempt. For a total deadline, wrap the call in
`tokio::time::timeout`. Dropping a future cancels local work but does not prove the
server did not process, or charge for, an earlier attempt. Retry settings alone
cannot guarantee exactly-once inference or a cost ceiling.

## Interpreting answers

Look answers up by name and match the expected kind before acting, as the
[support triage example](../examples/support_triage.rs) does. A missing answer or
an unexpected kind must not silently become false, zero or a default route. The
`nouls()`, `choices()` and `scores()` iterators are convenient for inspection but
do not show that every requested answer arrived. Future answer kinds are omitted
from `answers` and remain in the raw response body.

Probabilities describe judgments; they do not authorize actions or prove
calibration. Application code owns thresholds, abstention and human review.
Changing score order, question meaning or criteria changes application behavior
and should be evaluated with representative labeled cases.
