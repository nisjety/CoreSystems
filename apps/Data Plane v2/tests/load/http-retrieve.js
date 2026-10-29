// Isolated-only HTTP retrieval load test.
// Required:
//   BASE_URL=http://127.0.0.1:<explicit-local-port>
//   DPV2_USER_BEARER=<short-lived data-plane JWT>
//   DPV2_TEST_ORG_ID=<disposable organization>

import http from "k6/http";
import { check, sleep } from "k6";
import { Rate, Trend } from "k6/metrics";

function required(name) {
  const value = __ENV[name];
  if (!value) throw new Error(`${name} is required`);
  return value;
}

const BASE_URL = required("BASE_URL").replace(/\/$/, "");
const USER_BEARER = required("DPV2_USER_BEARER");
const TEST_ORG_ID = required("DPV2_TEST_ORG_ID");

const parsedBase = new URL(BASE_URL);
if (
  parsedBase.protocol !== "http:" ||
  !["localhost", "127.0.0.1", "[::1]", "::1"].includes(parsedBase.hostname)
) {
  throw new Error("BASE_URL must be an explicit loopback HTTP endpoint");
}
if (!/^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/.test(USER_BEARER)) {
  throw new Error("DPV2_USER_BEARER must be a compact JWT");
}
if (!/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/.test(TEST_ORG_ID)) {
  throw new Error("DPV2_TEST_ORG_ID has an invalid format");
}

const retrieveLatency = new Trend("retrieve_latency", true);
const retrieveErrors = new Rate("retrieve_errors");

export const options = {
  discardResponseBodies: true,
  stages: [
    { duration: "10s", target: 10 },
    { duration: "30s", target: 50 },
    { duration: "30s", target: 50 },
    { duration: "10s", target: 0 },
  ],
  thresholds: {
    retrieve_latency: ["p(95)<2000", "p(99)<5000"],
    retrieve_errors: ["rate<0.05"],
    http_req_duration: ["p(95)<3000"],
  },
};

const headers = {
  "Content-Type": "application/json",
  Authorization: `Bearer ${USER_BEARER}`,
  "X-Org-ID": TEST_ORG_ID,
};

const queries = [
  "authorization-safe retrieval load probe one",
  "authorization-safe retrieval load probe two",
  "authorization-safe retrieval load probe three",
];

export default function () {
  const query = queries[Math.floor(Math.random() * queries.length)];
  const response = http.post(
    `${BASE_URL}/v1/retrieve`,
    JSON.stringify({
      org_id: TEST_ORG_ID,
      query,
      top_k: 10,
      zdr_mode: "ephemeral",
      filters: {},
    }),
    { headers, timeout: "30s", responseType: "none" }
  );

  retrieveLatency.add(response.timings.duration);
  const ok = check(response, { "retrieve status 200": (result) => result.status === 200 });
  retrieveErrors.add(ok ? 0 : 1);
  sleep(0.5);
}

export function handleSummary(data) {
  const latency = data.metrics.retrieve_latency?.values || {};
  const errorRate = data.metrics.retrieve_errors?.values?.rate ?? "N/A";
  console.log("Data Plane v2 isolated retrieval load summary (response bodies suppressed)");
  console.log(`Retrieve p95: ${latency["p(95)"] ?? "N/A"}ms`);
  console.log(`Retrieve p99: ${latency["p(99)"] ?? "N/A"}ms`);
  console.log(`Error rate: ${errorRate}`);
  return {};
}
