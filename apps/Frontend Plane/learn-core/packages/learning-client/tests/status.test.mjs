import { test } from "node:test";
import assert from "node:assert/strict";
import {
  capabilityKeys,
  parseLearningStatus,
  loadLearningStatus,
} from "../src/index.ts";

const envelope = () => ({
  data: {
    service: "learning-core",
    phase: "school-access-and-courses",
    ready: false,
    processingProfile: "strict-self-hosted",
    capabilities: capabilityKeys.map((key, index) => ({
      key,
      state: index < 3 ? "connected" : "not-connected",
    })),
  },
});
test("accepts connected school and course foundations without implying teaching readiness", () => {
  const body = envelope();
  assert.equal(parseLearningStatus(body).ready, false);
});
test("rejects missing capabilities and duplicate entries", () => {
  const body = envelope();
  body.data.capabilities.pop();
  assert.throws(() => parseLearningStatus(body));
  body.data.capabilities.push(body.data.capabilities[0]);
  assert.throws(() => parseLearningStatus(body));
});
test("rejects live-state and processing-profile claims outside this contract", () => {
  for (const [key, value] of [
    ["ready", true],
    ["processingProfile", "cloud"],
    ["phase", "foundation"],
  ]) {
    const body = envelope();
    body.data[key] = value;
    assert.throws(() => parseLearningStatus(body));
  }
});
test("does not expose upstream error bodies", async () => {
  await assert.rejects(
    loadLearningStatus(
      undefined,
      async () => new Response("internal secret", { status: 503 }),
    ),
    (error) =>
      !error.message.includes("secret") &&
      error.message.includes("unavailable"),
  );
});
test("uses same-origin requests with cancellation and no-store", async () => {
  const controller = new AbortController();
  await loadLearningStatus(controller.signal, async (url, init) => {
    assert.equal(url, "/api/v1/learning/status");
    assert.equal(init.credentials, "same-origin");
    assert.equal(init.cache, "no-store");
    controller.abort();
    assert.equal(init.signal.aborted, true);
    return Response.json(envelope());
  });
});
