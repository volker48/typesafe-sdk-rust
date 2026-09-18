---
status: accepted
---

# Reuse JSON syntax and validate into typed requests

Use `serde_json::json!` with typed conversion for nested question authoring, retain
typed Rust construction, and provide a fallible convenience facade with precise local
diagnostics. A custom question DSL would duplicate a familiar grammar and create
another public compatibility surface; a map macro alone would leave most of the
wire-presence boilerplate exposed. JSON sacrifices compile-time schema checking
and duplicate-key detection, so local validation and explicit conversion limits
are part of the decision. See the [design](../request-ergonomics.md) and
[implementation gates](../plans/request-ergonomics.md); these interfaces are implemented.
