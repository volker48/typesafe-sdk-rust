# Request construction and the agent workflow

Status: the smoke example, README, and crate example now use existing `json!` +
typed deserialization. The convenience methods below are **proposed**, not public
SDK methods. [Implementation plan](plans/request-ergonomics.md).

## Decision

Use familiar JSON literals to author nested data, precise Rust types to establish
request invariants, and the existing client to execute. Add one fallible JSON
construction method next; keep the current typed constructor. Do not introduce a
custom macro language for question definitions.

The useful abstraction is a locally validated, inspectable request. Removing
punctuation is valuable only if the caller can still understand what will be sent,
repair an error, and interpret the result. An agent should learn one progression:

```text
evidence + named questions
       │ author with JSON or typed Rust
       ▼
typed request ── local input error ──► repair the identified field
       │ resolve model and explicit overrides
       ▼
effective body ── inspect locally (proposed)
       │ execute with explicit time/retry policy
       ▼
typed answers + usage + transport metadata
       │ apply application policy, preserve uncertainty
       ▼
decision + a small, reusable example or evaluation case
```

[CONTEXT.md](../CONTEXT.md) defines state, question, criteria, answer, and decision.
These names should agree across Rust examples, JSON, documentation, and fixtures.

## Three interfaces considered

| Interface | What the caller writes | What the caller must learn | Tradeoff |
|---|---|---|---|
| JSON + typed conversion | `json!({"urgent": {"type": "noul"}})` | Existing wire vocabulary and one validation seam | Nested data is concise; schema errors occur at runtime |
| Typed constructors + pairs | `[("urgent", Question::noul("Urgent?"))]` | A constructor for each common shape plus iterable conversion | Compiler assistance and composition; heterogeneous content needs conversions |
| Small macro + typed expressions | `questions! { "urgent": Question::noul("Urgent?") }` | Macro grammar plus the typed constructors | Saves punctuation but leaves most smoke-example ceremony |

A full nested SDK DSL would also own parsing, optional-field syntax, dynamic-key
syntax, macro hygiene, diagnostics, and an escape route back to ordinary Rust.
That cost grows with every question kind. `json!` already solves the literal
problem. A generic map macro leaves `Field::Value`, `Content`, and `Some` at every
call site and offers little depth.

The selected construction module hides decoding, presence handling, and semantic
validation. Its dependencies are in-process: the standard library and existing
Serde dependencies. Transport keeps its existing interface and loopback test
adapter; construction needs neither a new dependency nor a transport trait.

## Working today

```rust
use serde_json::json;
use typesafe_sdk::SystemOneRequest;

let request = SystemOneRequest::new(
    serde_json::from_value(json!({"message": "Payment failed", "attempts": 3}))?,
    serde_json::from_value(json!({
        "urgent": {
            "type": "noul",
            "instructions": "Does this require a fast response?",
            "criteria": {
                "true": "Blocked payment or a time-sensitive deadline.",
                "false": "Informational request with no deadline."
            }
        }
    }))?,
)?;
```

The caller's return type must accommodate both Serde and SDK errors, as the smoke
binary's `Result<(), Box<dyn std::error::Error>>` does. This is why the intermediate
form is useful today but is not the final ergonomic interface. Raw Serde errors
also do not carry the SDK's default error-redaction contract.

## Proposed construction interface

```rust
impl SystemOneRequest {
    pub fn from_json(
        state: serde_json::Value,
        questions: serde_json::Value,
    ) -> Result<Self, Error>;
}
```

Usage becomes `SystemOneRequest::from_json(json!(state), json!({...}))?`.
Both `from_json` and existing `new(Content, BTreeMap<String, Question>)` converge
on the same semantic validation. The client continues to accept a typed request.
Do not accept arbitrary JSON directly in `Client::system_one`.

Two explicit arguments mirror `new` and separate evidence from questions.
`model` and `extra_body` retain their existing configuration semantics. A second
whole-envelope `TryFrom<Value>` would add ambiguity about configuration and final
wire overrides without addressing a demonstrated need.

Keep the exact signature of `new`. Generalizing it to `Into<Content>` and an
iterator would break inference in existing `.into()` and empty-map calls. If
real callers need typed iteration, add `from_pairs` separately; do not launch
both a builder family and a macro family with the JSON facade.

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

Duplicate detection belongs before map construction. A future `from_pairs`
interface should reject repeated question names and typed choice constructors
should reject repeated labels. `from_json(Value, Value)` cannot recover that
information. For generated question sets requiring uniqueness, use a checked
entry loop today. Parsing raw text with strict duplicate rejection would be a
separate requirement, not an implied feature of this facade.

[`json!` documentation](https://docs.rs/serde_json/1.0.151/serde_json/macro.json.html)
specifies that interpolated values implement `Serialize`, keys implement
`Into<String>`, and some serialization failures panic. Use
`serde_json::to_value(custom_data)?` for fallible serialization before conversion.
This does not preserve nonfinite floats: JSON cannot represent them, and Serde
can encode them as null. Applications needing to distinguish those values must
validate their numeric data before serialization.

## Error repair is part of the interface

The prototype found `q: data did not match any variant of untagged enum Content`
for a number at `questions.q.criteria.billing`. Adding `serde_path_to_error`
around the existing enum decode did not recover the nested location. Do not ship
the facade with an unsupported claim that Serde already provides useful paths.

Require an input diagnostic on `Error`, separate from `ApiError`, containing a
machine-readable reason and an RFC 6901 JSON Pointer. For this example it should
identify `/questions/q/criteria/billing` with reason `invalid_content`. Escape
`~` and `/` in user-defined names. Missing fields identify the missing member;
array failures identify the index. Document first-error ordering (state, question
names in sorted order, then fields in a fixed order) so fixes are reproducible.

Default Display/Debug must describe the failure without echoing supplied values
or user-defined keys. Access to the pointer and underlying source is explicit:
both can contain caller-controlled data. Keep `ErrorKind::Input` for local
failures, preserve a Serde source where one exists, and do not fabricate HTTP
metadata. Existing HTTP validation paths and compatibility behavior stay intact.

Implement kind-specific decoding behind one private construction module if enum
buffering prevents precise paths. Centralize semantic checks with `new`; avoid a
second validator in a macro, example, or client method. Integration fixtures must
establish that typed and JSON construction accept the same domain values.

## Inspection and execution

Proposed next interface:

```rust
impl Client {
    pub fn system_one_body(&self, request: &SystemOneRequest)
        -> Result<serde_json::Value, Error>;
}
```

This computes the effective JSON body without HTTP and without authorization
headers. Share its private rendering implementation with `system_one_with` so
preview includes the resolved model and final `extra_body` overrides. Test equality
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

## Interpretation and accumulating useful knowledge

Examples should use `answers.get(name)` and match the expected kind when an action
depends on one answer. Absence or an unexpected kind must not silently become
false, zero, or a default route. Current group iterators are useful for inspection
but do not establish that every requested answer arrived. Future answer kinds are
currently omitted from typed HTTP results and retained in raw metadata.

Probabilities describe judgments; they do not authorize actions or prove
calibration. Application code owns thresholds, abstention, and human review.
Changing score order, question meaning, or criteria is an application behavior
change and should be evaluated with representative labeled cases.

Accretion means each correction leaves reusable evidence: a focused fixture for
a construction failure, a tested example for a common pattern, or a dated decision
for a real tradeoff. Keep domain definitions in `CONTEXT.md`, interface rules in
this document, ordered work in the plan, and observed results in fixtures. Do not
create an automatic memory service, prompt registry, generated-schema framework,
or output-policy DSL for this small SDK. Add a reusable question bundle only when
a caller actually repeats one; ordinary Rust functions and versioned data suffice.

## Prototype evidence and limits

The [implementation issue](../.scratch/request-ergonomics/issues/01-construction.md)
locates the throwaway branch, Rust experiment, captured observations, and HTML
walkthrough. Fourteen fixture scenarios exercised all question kinds, structured
state, dynamic names, omitted/null fields, invalid content, and invalid shapes.
The smoke question serialization matched the typed form; duplicate JSON names
collapsed, while the prototype pair constructor rejected duplicates.

This establishes syntax and current local semantics, not model quality, public
macro hygiene, performance, or complete wire parity. The prototype is a primary
source for the decision, not a replacement for production acceptance tests.
