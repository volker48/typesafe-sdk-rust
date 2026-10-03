import { expect, test } from "e2e";
import { execute, fixtureEnvironment } from "./process.js";

// Keep cancellation, concurrent calls, precise input diagnostics and platform TLS
// checks at their native seam, while including their verdicts in the e2e report.
for (const suite of ["request", "response", "client", "tls"]) {
  test(`native ${suite} flows`, async () => {
    const { stdout } = await execute("cargo", ["test", "--locked", "--test", suite], {
      env: fixtureEnvironment(),
      timeout: 35_000,
    });
    expect(stdout).toContain("test result: ok.");
    expect(stdout).toContain("0 failed");
  });
}
