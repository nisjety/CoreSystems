#!/usr/bin/env node
// Free live proof for inference-core's typed refusals and request counters.
//
// What it proves, against a running inference-core (no provider is called while
// no deployment is ZDR-attested, which is the state of the stack today):
//
//   1. GET /metrics is the Prometheus exposition, not the old 37-byte stub.
//   2. One ListModels moves inference_requests_total{rpc="list_models",outcome="ok"}.
//   3. One Infer with request.zdr=true is refused FAILED_PRECONDITION with the
//      trailer x-mp-provider-error: zdr_unavailable, and moves
//      inference_requests_total{rpc="infer",outcome="zdr_unavailable"} by exactly
//      one while the "ok" series and the token counters do not move.
//
// Native Node HTTP/2 with hand-written protobuf, like smoke-wave10.js (there is
// no grpcurl on the dev machine).
//
// Environment (the bearer is never printed):
//   INFERENCE_CORE_BEARER   required. A service-principal token minted by Auth
//                           Core with audience inference-core and scope
//                           inference:invoke. The token need not carry a ZDR
//                           claim: the request's own zdr flag is enough.
//   INFERENCE_CORE_ORG_ID   required. The token's org_id claim.
//   INFERENCE_CORE_GRPC     default http://127.0.0.1:9092
//   INFERENCE_CORE_HTTP     default http://127.0.0.1:18082
//   PROBE_MODEL             default gpt-4o-mini
//
// Run:   node "apps/Model Plane/scripts/smoke-inference-core-metrics.js"
// Exit:  0 when every check passes, 1 otherwise. Every check prints OK or FAIL.

const http = require("http");
const http2 = require("http2");

const GRPC = process.env.INFERENCE_CORE_GRPC || "http://127.0.0.1:9092";
const HTTP = process.env.INFERENCE_CORE_HTTP || "http://127.0.0.1:18082";
const BEARER = process.env.INFERENCE_CORE_BEARER || "";
const ORG_ID = process.env.INFERENCE_CORE_ORG_ID || "";
const MODEL = process.env.PROBE_MODEL || "gpt-4o-mini";
const TIMEOUT_MS = 15000;

if (!BEARER || !ORG_ID) {
  console.error("INFERENCE_CORE_BEARER and INFERENCE_CORE_ORG_ID are required");
  process.exit(2);
}

// The calling service's own id, read from the bearer's `sub` claim (a decode, not
// a verification, and nothing of the token is ever printed). A live stack keeps
// serving other services while the probe runs, so every series the checks below
// compare is narrowed to this caller; a bearer with no service id falls back to
// all callers, which is only reliable on a quiet stack.
function callerService() {
  try {
    const sub = JSON.parse(Buffer.from(BEARER.split(".")[1], "base64url").toString("utf-8")).sub;
    return typeof sub === "string" && sub.startsWith("service:") ? sub : "";
  } catch {
    return "";
  }
}
const SERVICE = callerService();
const OWN = SERVICE ? { service: SERVICE } : {};

// ---- Protobuf primitives (wire format only) ------------------------------
const tag = (field, wire) => (field << 3) | wire;
function varint(v) {
  const out = [];
  while (v > 127) {
    out.push((v & 0x7f) | 0x80);
    v >>>= 7;
  }
  out.push(v);
  return Buffer.from(out);
}
const str = (field, s) => {
  const bytes = Buffer.from(s, "utf-8");
  return Buffer.concat([Buffer.from([tag(field, 2)]), varint(bytes.length), bytes]);
};
const bool = (field, b) => Buffer.concat([Buffer.from([tag(field, 0)]), varint(b ? 1 : 0)]);
const int = (field, n) => Buffer.concat([Buffer.from([tag(field, 0)]), varint(n)]);
const message = (field, payload) =>
  Buffer.concat([Buffer.from([tag(field, 2)]), varint(payload.length), payload]);

// InferRequest: request_id=1 org_id=2 model=3 messages=5 max_tokens=7 zdr=9.
// ChatMessage: role=1 content=2. (proto/model_plane/v1/inference.proto)
function inferRequest() {
  return Buffer.concat([
    str(1, `smoke-cr-t3-${Date.now()}`),
    str(2, ORG_ID),
    str(3, MODEL),
    message(5, Buffer.concat([str(1, "user"), str(2, "ping")])),
    int(7, 1),
    bool(9, true),
  ]);
}

// ---- gRPC over HTTP/2 -----------------------------------------------------
// A refusal arrives "trailers-only": grpc-status rides the response headers.
// A success carries it in the trailers. Read both.
function grpc(method, payload) {
  return new Promise((resolve) => {
    const client = http2.connect(GRPC);
    const done = (value) => {
      client.close();
      resolve(value);
    };
    const timer = setTimeout(() => done({ error: `timeout ${TIMEOUT_MS}ms` }), TIMEOUT_MS);
    client.on("error", (e) => {
      clearTimeout(timer);
      done({ error: e.message });
    });
    const req = client.request({
      ":method": "POST",
      ":path": `/model_plane.v1.InferenceCore/${method}`,
      "content-type": "application/grpc",
      te: "trailers",
      authorization: `Bearer ${BEARER}`,
    });
    let headers = {};
    let trailers = {};
    const chunks = [];
    req.on("response", (h) => {
      headers = h;
    });
    req.on("trailers", (t) => {
      trailers = t;
    });
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      clearTimeout(timer);
      const all = { ...headers, ...trailers };
      done({
        status: Number(all["grpc-status"]),
        message: decodeURIComponent(all["grpc-message"] || ""),
        kind: all["x-mp-provider-error"] || "",
        bytes: Buffer.concat(chunks).length,
      });
    });
    req.on("error", (e) => {
      clearTimeout(timer);
      done({ error: e.message });
    });
    const frame = Buffer.alloc(5);
    frame.writeUInt32BE(payload.length, 1);
    req.end(Buffer.concat([frame, payload]));
  });
}

function scrape() {
  return new Promise((resolve) => {
    http
      .get(`${HTTP}/metrics`, (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () =>
          resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString("utf-8") }),
        );
      })
      .on("error", (e) => resolve({ error: e.message }));
  });
}

// Sum of every series of `name` that carries all of the key="value" pairs.
function sum(text, name, labels) {
  let total = 0;
  for (const line of text.split("\n")) {
    if (!line.startsWith(`${name}{`)) continue;
    if (!Object.entries(labels).every(([k, v]) => line.includes(`${k}="${v}"`))) continue;
    total += Number(line.slice(line.lastIndexOf(" ") + 1)) || 0;
  }
  return total;
}

let failures = 0;
function check(name, ok, detail) {
  console.log(`${ok ? "OK  " : "FAIL"} ${name}${ok || !detail ? "" : `  (${detail})`}`);
  if (!ok) failures += 1;
}

(async () => {
  const before = await scrape();
  check(
    "1. /metrics is the exposition, not the 37-byte stub",
    !before.error &&
      before.status === 200 &&
      before.body.length > 37 &&
      before.body.includes("inference_core_build_info"),
    before.error || `status ${before.status}, ${before.body ? before.body.length : 0} bytes`,
  );
  if (before.error) {
    console.log("cannot continue without a scrape");
    process.exit(1);
  }

  const catalogue = await grpc("ListModels", Buffer.alloc(0));
  check(
    "2a. ListModels answers OK",
    catalogue.status === 0,
    catalogue.error || `grpc-status ${catalogue.status}: ${catalogue.message}`,
  );

  const refused = await grpc("Infer", inferRequest());
  if (refused.status === 0) {
    console.log(
      "WARN  the zdr=true Infer was SERVED: a deployment is ZDR-attested, so this probe reached a provider (max_tokens=1)",
    );
  }
  check(
    "3a. zdr=true Infer is FAILED_PRECONDITION (9)",
    refused.status === 9,
    refused.error || `grpc-status ${refused.status}: ${refused.message}`,
  );
  check(
    "3b. the refusal carries x-mp-provider-error: zdr_unavailable",
    refused.kind === "zdr_unavailable",
    `trailer was "${refused.kind}"`,
  );

  const after = await scrape();
  if (after.error) {
    check("5. the second scrape answered", false, after.error);
    console.log("cannot compare the counters without a second scrape");
    process.exit(1);
  }
  console.log(
    SERVICE
      ? `counters compared for the caller ${SERVICE} only`
      : "the bearer names no service: counters compared across all callers (reliable only on a quiet stack)",
  );
  const requests = (text, labels) => sum(text, "inference_requests_total", { ...labels, ...OWN });
  const tokens = (text) => sum(text, "inference_tokens_total", OWN);

  check(
    "2b. list_models/ok moved by at least one",
    requests(after.body, { rpc: "list_models", outcome: "ok" }) >
      requests(before.body, { rpc: "list_models", outcome: "ok" }),
  );
  const refusals = (text) => requests(text, { rpc: "infer", outcome: "zdr_unavailable" });
  check(
    "3c. infer/zdr_unavailable moved by exactly one",
    refusals(after.body) - refusals(before.body) === 1,
    `delta ${refusals(after.body) - refusals(before.body)}`,
  );
  const answered = (text) => requests(text, { rpc: "infer", outcome: "ok" });
  check(
    "3d. infer/ok did not move: no provider answered",
    answered(after.body) === answered(before.body),
  );
  check("3e. no tokens were counted", tokens(after.body) === tokens(before.body));
  check(
    "4. no org id in any label",
    !after.body.includes(ORG_ID),
    "the org id appears in the exposition",
  );

  console.log(failures === 0 ? "ALL OK" : `${failures} check(s) FAILED`);
  process.exit(failures === 0 ? 0 : 1);
})();
