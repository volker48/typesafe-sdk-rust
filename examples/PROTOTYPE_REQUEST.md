# Throwaway request-construction experiment

Question: can JSON-style authoring remove map/wrapper ceremony while preserving
the current typed request semantics, and what prevents it from being a complete
agent-friendly interface?

Baseline: `fd8d1a0`. Branch: `codex/prototype-request-ergonomics`.
Production SDK files on this branch are unchanged. All added files are disposable
experimental evidence. The implementation plan lives in the main task worktree at
`docs/plans/request-ergonomics.md`, with the issue under
`.scratch/request-ergonomics/issues/01-construction.md`.

Open `prototype_request.html` directly for free play and four guided walkthroughs.
It embeds recorded Rust observations and needs no server, dependencies, credentials,
or storage. It does not validate arbitrary new inputs or run model inference.

Reproduce construction observations from the repository root:

```sh
cargo run --locked --example prototype_request
```

`prototype_request_observations.json` captures that output. The HTML embeds the
same JSON in its `observations` script element. To refresh the HTML after changing
the Rust experiment, replace that element's content with newly captured output;
escape any literal `</` sequence as `<\/` when embedding JSON in HTML.

Compare the original smoke binary on this branch with the simplified binary in
another checkout (substitute its directory for `SDK_WORKTREE`):

```sh
uv run --no-project python examples/prototype_smoke_compare.py . SDK_WORKTREE
```

The helper builds/runs both binaries against fresh loopback servers and synthetic
credentials, then compares their captured paths and semantic JSON bodies.
`prototype_smoke_observations.json` records equal bodies for all three questions.

Verdict: reuse `json!` plus typed conversion. Fourteen captured scenarios preserve
presence and score order and reject unsupported shapes. JSON duplicate names
collapse before validation. A simple pair constructor can reject duplicates; a
macro alone does not hide question construction. Nested Serde errors are too coarse
for a polished facade, even with `serde_path_to_error`.

Limits: debug-format equality is exploratory evidence only. The tiny macro covers
literal keys; dynamic expressions and downstream hygiene are unverified. The typed
choice helper does not check duplicate labels. Prototype errors print synthetic
fixture details and do not establish production redaction. No production tests
were added to this throwaway branch.

Verification: Rust example ran successfully; all 14 observations captured;
original/simplified smoke HTTP bodies matched; JavaScript syntax passed `node
--check`. Browser policy blocked the local-file preview, so visual rendering and
interactive browser behavior were not verified. No live service call occurred.
