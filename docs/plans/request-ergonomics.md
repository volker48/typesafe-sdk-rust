# Request ergonomics implementation plan

This plan follows the [interface design](../request-ergonomics.md). Complete one
slice before adding another public surface. Baseline: tracked commit `fd8d1a0`.

## Completed here: validate the direction

- Compare JSON, typed iterable construction, and a small literal macro using
  existing types and pinned dependencies.
- Preserve the throwaway Rust experiment and interactive HTML on
  `codex/prototype-request-ergonomics`; record its commit in the
  [construction issue](../../.scratch/request-ergonomics/issues/01-construction.md).
- Simplify `sdk_smoke.rs`, README, and the compiled crate example with existing
  `json!` + `serde_json::from_value`. Preserve all smoke prompts and score order.
- Record the domain vocabulary, decision, known conversion limits, and follow-up
  gates. A recommendation is accepted; new SDK methods are not yet shipped.

## 1. Ship one JSON construction seam with repairable errors

Implement proposed `SystemOneRequest::from_json(Value, Value) -> Result<Self, Error>`
and structured local input diagnostics together. Keep the existing `new` signature.
Use a private request-decoding module only if needed to own precise decoding;
centralize semantic checks with the typed constructor. Add no dependencies.

Completion gates:

- Paired typed and JSON requests produce equal semantic HTTP bodies for all three
  smoke questions, rich content, dynamic names, and omitted/null/value cases.
- Scalar state, unknown fields/kinds, malformed criteria, empty questions, and
  empty score criteria fail locally with `ErrorKind::Input`; no HTTP attempt occurs.
- Empty choice maps and one-level scores preserve baseline acceptance. No new
  validation of probabilities, name spelling, or blank prose is introduced.
- Local diagnostics expose stable reason codes and JSON Pointers for nested
  failures, missing fields, and array indices. Names containing `/`, `~`, dots,
  quotes, or Unicode remain unambiguous. Two simultaneous faults verify documented
  first-error order. Default formatting exposes neither content nor arbitrary keys;
  source errors remain available explicitly.
- Both entry points use the same semantic checks. Test direct typed enum values,
  not just convenience constructors. Existing `.into()`/empty-map callers compile.
- Rustdoc demonstrates JSON authoring, explicit null versus omission, custom
  fallible serialization, and the duplicate-key limit. Replace the interim
  two-stage snippets only after the facade passes these checks.

Use normal public-interface tests and the existing real loopback HTTP fixture.
Extend compatibility scenarios only for observable behavior requiring comparison;
keep Python-generated expectations independent of the Rust implementation.

## 2. Make effective requests inspectable

After step 1, add proposed `Client::system_one_body(&SystemOneRequest)` returning
the effective body as `Result<Value, Error>`. Extract the existing body assembly
once and use it for both preview and execution.

Completion gates:

- Preview equals captured HTTP body with inherited model, request model override,
  additional fields, and reserved-field/null `extra_body` overrides.
- Preview performs no HTTP, retry, credential logging, or automatic output. It
  contains no transport headers and accurately documents sensitive body content.
- A request mutated after preview produces a newly rendered body on execution;
  there is no hidden stale cache.
- Tests preserve protected headers, retries, metadata, cancellation, and existing
  raw override semantics. Do not claim the final body is validated after overrides.

## 3. Complete the usage loop in executable examples

Add one focused example showing stable questions across changed state, explicit
total deadline and retry choices, usage/request-ID inspection, and named answer
matching with an application-owned abstention branch. Keep this example separate
from the small quickstart. Use synthetic evidence and loopback fixtures for CI.

Completion gates:

- Each documented snippet compiles. Missing and unexpected answer kinds are
  handled explicitly; no default action is inferred from absent data.
- Every advertised control names its scope: attempt timeout, retry admission
  budget, total caller deadline. No exactly-once or cost-cap claim is made.
- Fixture checks demonstrate the example's policy independently of model output.
  No fabricated quality improvement is attributed to syntax changes.
- A question/criteria revision can be compared on the same application-owned
  evaluation cases, with model and usage recorded and sensitive evidence opt-in.

## Conditional follow-up: typed composition

Add `from_pairs` and typed `choice`/`score`/Noul-criteria conveniences only when
actual caller code needs repeated programmatic composition. Revisit a macro only
if those callers still have a demonstrated readability problem. This is not a
dependency of steps 1–3.

Require duplicate-name/label rejection before collection, evaluation once per
expression, ordinary iterator support, explicit empty-iterator inference examples,
and mixed-content examples. If a macro is justified, require downstream compilation
with a renamed crate, dynamic keys, empty input, trailing commas, nested `?`, and
invalid values. Keep it a thin expansion through the same typed constructors.

## Integration and verification

The main checkout also contains ignored `migration/MIGRATION.md` and
`migration/VERIFICATION.md`, plus uncommitted foundation work. They were consulted
as context, not copied as verified facts about this baseline or edited across
worktrees. This tracked plan adds the construction/inspection track to the
foundation roadmap; it does not replace pending transport-compatibility decisions,
model listing, raw extensions, or release checks. Reconcile against the foundation
branch before implementing: its response ordering and decoding changes must remain
intact. Keep this plan as the authoritative construction roadmap.

For production implementation run `cargo fmt --check`,
`cargo check --locked --all-targets`, `cargo test --locked --all-targets`,
`cargo test --locked --doc`, and
`cargo clippy --locked --all-targets -- -D warnings`. Run the pinned Python
compatibility harness for any model/transport changes; never re-record its oracle
to resolve a Rust failure. Inspect the final diff and report any unavailable check.

For this example-and-design change, verification is bounded to compilation,
formatting/lint, the crate example, offline prototype observations, and a loopback
comparison of the original and simplified smoke binary. No live inference is
needed to settle construction ergonomics.
