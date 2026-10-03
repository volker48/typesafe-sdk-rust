import { createServer } from "node:http";
import { once } from "node:events";
import { rejects } from "node:assert/strict";
import { expect, test } from "e2e";
import { executable, execute, fixtureEnvironment } from "./process.js";

interface CapturedRequest {
  method: string | undefined;
  path: string | undefined;
  authorization: string | undefined;
  body: unknown;
}

async function runExample(name: string, responses: unknown[]) {
  const requests: CapturedRequest[] = [];
  const failures: unknown[] = [];
  const server = createServer(async (request, response) => {
    try {
      const chunks: Buffer[] = [];
      for await (const chunk of request) chunks.push(Buffer.from(chunk));
      requests.push({
        method: request.method,
        path: request.url,
        authorization: request.headers.authorization,
        body: JSON.parse(Buffer.concat(chunks).toString("utf8")),
      });
      const index = requests.length - 1;
      if (index >= responses.length) throw new Error("Unexpected extra request");
      response.writeHead(200, {
        "content-type": "application/json",
        "x-typesafe-request-id": `example-${index}`,
      });
      response.end(JSON.stringify(responses[index]));
    } catch (error) {
      failures.push(error);
      response.writeHead(500);
      response.end();
    }
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("Expected TCP listener");
  try {
    const result = await execute(executable(name), [], {
      env: fixtureEnvironment(`http://127.0.0.1:${address.port}`),
      timeout: 15_000,
    });
    expect(failures).toEqual([]);
    expect(requests).toHaveLength(responses.length);
    for (const request of requests) {
      expect(request.method).toBe("POST");
      expect(request.path).toBe("/v1/systemone");
      expect(request.authorization).toBe("Bearer example-fixture-key");
    }
    return { ...result, requests };
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) => {
      server.close((error) => (error ? reject(error) : resolve()));
    });
  }
}

test("question-kinds example sends all three question kinds and reports answers and metadata", async () => {
  const { stdout, requests } = await runExample("examples/question_kinds", [
    {
      model: "smoke-model",
      usage: { input_tokens: 12, output_tokens: 3 },
      answers: {
        is_urgent: { type: "noul", noul: 0.95 },
        team: {
          type: "choice",
          choice: "billing",
          confidence: 0.9,
          probabilities: { billing: 0.9 },
        },
        customer_sentiment: {
          type: "score",
          score: 1.5,
          confidence: 0.8,
          legend: { "0": "Calm", "1": "Frustrated" },
          probabilities: { "0": 0.1, "1": 0.9 },
        },
      },
    },
  ]);
  expect(requests[0].body).toMatchObject({
    model: "jev-latest",
    questions: {
      is_urgent: { type: "noul" },
      team: { type: "choice" },
      customer_sentiment: { type: "score" },
    },
  });
  for (const text of [
    "Model: smoke-model",
    "HTTP status: 200",
    "example-0",
    "is_urgent:",
    "team:",
    "customer_sentiment:",
    "input_tokens: Some(12)",
  ]) {
    expect(stdout).toContain(text);
  }
});

for (const [team, confidence, decision] of [
  ["billing", 0.9, "EscalateBilling"],
  ["technical", 0.9, "EscalateTechnical"],
  ["billing", 0.5, "Review"],
  ["unknown", 0.9, "Review"],
] as const) {
  test(`triage ${team} at confidence ${confidence} produces ${decision} then reviews a routine case`, async () => {
    const judgment = (urgent: number) => ({
      model: "triage-model",
      usage: { input_tokens: 5, output_tokens: 2 },
      answers: {
        urgent: { type: "noul", noul: urgent },
        team: { type: "choice", choice: team, confidence, probabilities: {} },
      },
    });
    const { stdout, requests } = await runExample("examples/support_triage", [
      judgment(0.95),
      judgment(0.1),
    ]);
    const lines = stdout.trim().split("\n");
    expect(lines).toHaveLength(2);
    expect(lines[0]).toContain(`decision=${decision}`);
    expect(lines[1]).toContain("decision=Review");
    for (const [index, caseId] of ["payroll", "invoice-help"].entries()) {
      expect(lines[index]).toContain(
        `case=${caseId} criteria=support-triage-v1 model=triage-model`,
      );
      expect(lines[index]).toContain(`example-${index}`);
      expect(requests[index].body).toMatchObject({
        state: { message: expect.any(String) },
        questions: { urgent: { type: "noul" }, team: { type: "choice" } },
      });
    }
  });
}

for (const name of ["examples/question_kinds", "examples/support_triage"]) {
  test(`${name} fails when credentials are absent`, async () => {
    await rejects(
      execute(executable(name), [], { env: fixtureEnvironment(), timeout: 5_000 }),
      /Input/,
    );
  });
}
