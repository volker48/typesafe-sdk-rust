import type { E2EConfig } from "e2e";

export default {
  targets: [{ name: "rust-sdk", platform: "custom" }],
  tests: ["tests/e2e/**/*.e2e.ts"],
  workers: 4,
  retries: 0,
  timeout: 45_000,
  trace: "off",
} satisfies E2EConfig;
