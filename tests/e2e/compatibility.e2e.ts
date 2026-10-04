import { execFile } from "node:child_process";
import { readFileSync } from "node:fs";
import { promisify } from "node:util";
import { describe, expect, test } from "e2e";

const execute = promisify(execFile);
const suites = {
  systemone: "cases.json",
  foundation: "foundation_cases.json",
  models: "models_cases.json",
  extensions: "extensions_cases.json",
};

for (const [suite, file] of Object.entries(suites)) {
  const cases: { id: string }[] = JSON.parse(readFileSync(`compat/${file}`, "utf8"));
  describe(suite, () => {
    for (const { id } of cases) {
      test(id, async () => {
        const { stdout } = await execute(
          "uv",
          [
            "run",
            "--no-cache",
            "--no-project",
            "python",
            "compat/e2e.py",
            "--suite",
            suite,
            "--case",
            id,
          ],
          { timeout: 35_000, maxBuffer: 2 * 1024 * 1024 },
        );
        expect(stdout.trim()).toBe(`PASS ${id}`);
      });
    }
  });
}
