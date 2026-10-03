import { execFile } from "node:child_process";
import { promisify } from "node:util";

export const execute = promisify(execFile);

export function fixtureEnvironment(baseUrl?: string): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (["PATH", "HOME", "TMPDIR", "TEMP", "TMP", "SYSTEMROOT"].includes(key.toUpperCase())) {
      env[key] = value;
    }
  }
  env.NO_PROXY = "*";
  if (baseUrl !== undefined) {
    env.TYPESAFE_API_KEY = "example-fixture-key";
    env.TYPESAFE_BASE_URL = baseUrl;
  }
  return env;
}

export function executable(name: string): string {
  return `target/debug/${name}${process.platform === "win32" ? ".exe" : ""}`;
}
