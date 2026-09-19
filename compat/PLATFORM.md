# Platform and local TLS verification

This milestone adds repeatable platform checks and local HTTPS evidence without
changing the public client API, transport policy, or Python oracle files.

## Gates

`.github/workflows/ci.yml` runs Rust 1.96.0 (the declared minimum version) on
Ubuntu 24.04, macOS 14 and Windows 2025 for pushes, pull requests and manual runs.
Each job checks formatting, all-target compilation, no-default-features
compilation, all-target/all-feature tests, doctests, Clippy and rustdoc with
warnings denied. Jobs have a 20-minute limit and read-only repository permission;
checkout is pinned to a full commit and does not persist credentials.
No live service, Python reference checkout or API credentials are required.

## TLS contract under test

`tests/tls.rs` uses the public `Client::list_models` API and a real loopback
Rustls server. `tokio-rustls` is an exact-pinned development dependency on the
version already in the lockfile; no package versions or production dependencies
changed. It provides the server boundary rather than mocking SDK logic.

- Every platform rejects the fixture's untrusted CA, returning a connection
  error with a cause and no HTTP metadata; the server also observes a rejected
  TLS handshake.
- Linux additionally trusts that CA through a child process's `SSL_CERT_FILE`,
  completes authenticated HTTPS model listing, and checks returned data and HTTP
  metadata. The same trusted certificate is rejected for the IP address because
  it is valid only for `localhost`.
- Child processes isolate certificate and proxy settings. Tests remove inherited
  proxy/certificate environment overrides and disable proxies. Neither the host
  trust store nor the parent process environment changes. Retries are disabled,
  and each exchange has a ten-second deadline.

macOS and Windows use native trust stores, so trusted local certificates and
hostname rejection are covered only on Linux. This is not a claim of Python TLS
parity, exhaustive TLS coverage, or live service compatibility. Certificate
expiry, revocation, client certificates and TLS-version negotiation are outside
this slice. Fixture formats, validity dates and regeneration instructions are in
`tests/fixtures/tls/README.md`.

## Local evidence

On macOS arm64, the untrusted-certificate test passed. Temporarily disabling
client certificate validation made it fail with `client accepted an invalid
certificate`; restoring the original client made it pass. The mutation was
removed and production sources are unchanged.

Local checks passed: `cargo fmt --check`, `cargo check --locked --all-targets`,
`cargo check --locked --no-default-features`,
`cargo test --locked --all-targets --all-features` (35 tests),
`cargo test --locked --doc` (5 doctests),
`cargo clippy --locked --all-targets --all-features -- -D warnings`,
`RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features`, and
`git diff --check`. OpenSSL independently verified the server certificate
against the fixture CA. Python compatibility suites were not rerun because
production behavior and oracle files did not change.

The workflow has been validated locally with `actionlint` and `zizmor`. Creating
the matrix is not evidence that its jobs have passed: Linux, Windows and the
Linux-only trusted-CA tests still require an actual CI run. No push or workflow
dispatch is part of this milestone.

Independent Standards and Spec reviews of the staged change against starting
commit `db2eecd14641c785e4e85707d886f41b4d5294f2` found no actionable issues
on either axis. Neither review substitutes for the pending platform executions.
