// k6 load test for HTTP retrieval endpoints
// Install: brew install k6  OR  https://k6.io/docs/get-started/installation/
//
// Usage:
//   k6 run tests/load/http-retrieve.js
//   k6 run --vus 50 --duration 60s tests/load/http-retrieve.js
//   BASE_URL=http://host:8014 API_KEY=xxx k6 run tests/load/http-retrieve.js

import http from "k6/http";
import { check, sleep } from "k6";
import { Rate, Trend } from "k6/metrics";

const BASE_URL = __ENV.BASE_URL || "http://localhost:8014";
const API_KEY = __ENV.API_KEY || "";

const retrieveLatency = new Trend("retrieve_latency", true);
const retrieveErrors = new Rate("retrieve_errors");

export const options = {
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
};
if (API_KEY) {
  headers["x-api-key"] = API_KEY;
}

const queries = [
  "What is the company vacation policy?",
  "How do I submit an expense report?",
  "What are the security requirements for production deployments?",
  "Explain the data retention policy for customer information",
  "What is the onboarding process for new employees?",
];

export default function () {
  const query = queries[Math.floor(Math.random() * queries.length)];

  // Core retrieval
  const retrieveRes = http.post(
    `${BASE_URL}/v1/retrieve`,
    JSON.stringify({
      org_id: "org-loadtest",
      query: query,
      filters: {},
    }),
    { headers, timeout: "30s" }
  );

  retrieveLatency.add(retrieveRes.timings.duration);
  const retrieveOk = check(retrieveRes, {
    "retrieve status 200": (r) => r.status === 200,
    "retrieve has candidates": (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.candidates !== undefined;
      } catch {
        return false;
      }
    },
  });
  if (!retrieveOk) retrieveErrors.add(1);
  else retrieveErrors.add(0);

  sleep(0.1);

  // Health check (lightweight baseline)
  const healthRes = http.get(`${BASE_URL}/health`);
  check(healthRes, {
    "health 200": (r) => r.status === 200,
  });

  sleep(0.5);
}

export function handleSummary(data) {
  const p95 = data.metrics.retrieve_latency
    ? data.metrics.retrieve_latency.values["p(95)"]
    : "N/A";
  const p99 = data.metrics.retrieve_latency
    ? data.metrics.retrieve_latency.values["p(99)"]
    : "N/A";
  const errRate = data.metrics.retrieve_errors
    ? data.metrics.retrieve_errors.values.rate
    : "N/A";

  console.log("\n═══════════════════════════════════════════════");
  console.log(" Data Plane v2 — Load Test Summary");
  console.log("═══════════════════════════════════════════════");
  console.log(` Retrieve p95: ${p95}ms`);
  console.log(` Retrieve p99: ${p99}ms`);
  console.log(` Error rate:   ${errRate}`);
  console.log("═══════════════════════════════════════════════\n");

  return {};
}
