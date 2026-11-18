/**
 * Data deletion adapter (WS19-S8, W3-FIX-10, W3-FIX-17): the receipt checks for both Data legs, the outcome combination, the
 * Space organization check, and deliverOne's mint, presentation, release and acknowledgement paths.
 *
 * convex-test is not a dependency of this package, so deliverOne runs through its `_handler` against a recording ctx and a
 * stubbed fetch. Nothing leaves the process: every fetch is answered in the test, and a call to any other URL fails it. The
 * module reads its URLs and credentials when it loads, so each delivery loads a fresh copy with the env it needs.
 * Run with:
 *   cd "apps/Application Plane/convex-core" && npx tsx --test convex/spaceDeletionData.test.ts
 */

import assert from "node:assert/strict";
import test from "node:test";

import type * as SpaceDeletionData from "./spaceDeletionData";

// getFunctionName reads a reference's name at runtime. The declarations this package resolves under NodeNext omit it, so it is
// loaded here and typed by hand.
const { getFunctionName } = require("convex/server") as { getFunctionName: (reference: unknown) => string };

const ENV_KEYS = [
  "DATA_SPACE_DELETION_URL",
  "DATA_SPACE_CANVAS_DELETION_URL",
  "AUTH_CORE_URL",
  "CONTROL_SPACE_DELETION_SERVICE_API_KEY",
] as const;
type EnvKey = (typeof ENV_KEYS)[number];

const AUTH_MINT_URL = "https://auth-core.test.invalid/api/data-plane/internal-token";
const LEG1_URL = "https://data-plane.test.invalid/v1/internal/space-binding/delete";
const LEG2_URL = "https://data-plane.test.invalid/v1/internal/space-canvas/purge";
const CONTROL_KEY = "fake-control-deletion-key-bbbbbbbbbbbbbbbb";
const TOKEN = "fake-minted-data-token-aaaaaaaaaaaaaaaa";
const BASE_ENV: Record<EnvKey, string> = {
  DATA_SPACE_DELETION_URL: LEG1_URL,
  DATA_SPACE_CANVAS_DELETION_URL: LEG2_URL,
  // A trailing slash, which the mint trims before it appends its path.
  AUTH_CORE_URL: "https://auth-core.test.invalid/",
  CONTROL_SPACE_DELETION_SERVICE_API_KEY: CONTROL_KEY,
};

const ORG = "org_ext";
const SPACE = "space_1";
const REQUEST = "req-1";
const EVENT = "evt-1";
const WORKER = "worker-1";
const CLAIM = { eventId: EVENT, workerId: WORKER, request: { requestId: REQUEST, spaceRef: SPACE, externalOrgId: ORG } };

type Module = typeof SpaceDeletionData;

function loadWith(env: Partial<Record<EnvKey, string>>): Module {
  const path = require.resolve("./spaceDeletionData.ts");
  const saved = ENV_KEYS.map((key) => [key, process.env[key]] as const);
  for (const key of ENV_KEYS) {
    const value = key in env ? env[key] : BASE_ENV[key];
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
  delete require.cache[path];
  try {
    return require(path) as Module;
  } finally {
    delete require.cache[path];
    for (const [key, value] of saved) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  }
}

const mod = loadWith({});

// ---------------------------------------------------------------------------------------------------------------------
// Receipt fixtures. The shapes match what the Data legs return: the retrieval revoke (retrieval-engine-rs) and the canvas
// purge (wiki-store canvas_purge.go) both name request_id, owner_plane, org_id, space_ref and owner_outcome.
// ---------------------------------------------------------------------------------------------------------------------

function retrievalReceipt(over: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    request_id: REQUEST,
    owner_plane: "data",
    org_id: ORG,
    space_ref: SPACE,
    owner_outcome: "partial",
    remaining_work: "retrieval bindings remain",
    ...over,
  };
}

function canvasReceipt(over: Record<string, unknown> = {}): Record<string, unknown> {
  return { request_id: REQUEST, owner_plane: "data", org_id: ORG, space_ref: SPACE, owner_outcome: "succeeded", ...over };
}

function outcome(status: string) {
  return { status: status as any, receiptRef: `data:${REQUEST}`, detail: `leg ${status}` };
}

// ---------------------------------------------------------------------------------------------------------------------
// Outcome combination and receipt checks (pure).
// ---------------------------------------------------------------------------------------------------------------------

const OUTCOMES = ["succeeded", "partial", "unknown", "failed"] as const;
// The weaker status wins: failed, then unknown, then partial, then succeeded. Listed out in full, not computed from the rank table.
const WEAKER: Record<string, string> = {
  "succeeded|succeeded": "succeeded", "succeeded|partial": "partial", "succeeded|unknown": "unknown", "succeeded|failed": "failed",
  "partial|succeeded": "partial", "partial|partial": "partial", "partial|unknown": "unknown", "partial|failed": "failed",
  "unknown|succeeded": "unknown", "unknown|partial": "unknown", "unknown|unknown": "unknown", "unknown|failed": "failed",
  "failed|succeeded": "failed", "failed|partial": "failed", "failed|unknown": "failed", "failed|failed": "failed",
};

test("combineDataOutcomes keeps the weaker status for every pair, in either order", () => {
  for (const first of OUTCOMES) {
    for (const second of OUTCOMES) {
      const combined = mod.combineDataOutcomes(outcome(first), outcome(second));
      assert.equal(combined.status, WEAKER[`${first}|${second}`], `${first} then ${second}`);
    }
  }
});

test("combineDataOutcomes names both legs in the detail, keeps the first receipt ref and caps the detail at 256 characters", () => {
  assert.deepEqual(
    mod.combineDataOutcomes(
      { status: "partial", receiptRef: "data:req-1", detail: "first leg" },
      { status: "succeeded", receiptRef: "data:req-1", detail: "second leg" },
    ),
    { status: "partial", receiptRef: "data:req-1", detail: "first leg; second leg" },
  );
  const long = mod.combineDataOutcomes(
    { status: "succeeded", receiptRef: "data:req-1", detail: "a".repeat(200) },
    { status: "succeeded", receiptRef: "data:req-1", detail: "b".repeat(200) },
  );
  assert.equal(long.detail.length, 256);
});

test("dataCanvasPurgeReceipt refuses another request_id, another space_ref and a non-data owner_plane", () => {
  assert.deepEqual(mod.dataCanvasPurgeReceipt(canvasReceipt(), REQUEST, ORG, SPACE), {
    status: "succeeded",
    receiptRef: `data:${REQUEST}`,
    detail: "Data canvas purge receipt: succeeded",
  });
  assert.equal(mod.dataCanvasPurgeReceipt(canvasReceipt({ request_id: "req-2" }), REQUEST, ORG, SPACE), null);
  assert.equal(mod.dataCanvasPurgeReceipt(canvasReceipt({ space_ref: "space_2" }), REQUEST, ORG, SPACE), null);
  assert.equal(mod.dataCanvasPurgeReceipt(canvasReceipt({ owner_plane: "model" }), REQUEST, ORG, SPACE), null);
  assert.equal(mod.dataCanvasPurgeReceipt(null, REQUEST, ORG, SPACE), null);
});

test("dataCanvasPurgeReceipt returns the canvas outcome only when the org and the Space both match (W3-FIX-17)", () => {
  for (const status of OUTCOMES) {
    assert.equal(mod.dataCanvasPurgeReceipt(canvasReceipt({ owner_outcome: status }), REQUEST, ORG, SPACE)?.status, status);
  }
  assert.equal(mod.dataCanvasPurgeReceipt(canvasReceipt({ org_id: undefined }), REQUEST, ORG, SPACE), null);
  assert.equal(mod.dataCanvasPurgeReceipt(canvasReceipt({ org_id: "org_other" }), REQUEST, ORG, SPACE), null);
  assert.equal(mod.dataCanvasPurgeReceipt(canvasReceipt({ space_ref: undefined }), REQUEST, ORG, SPACE), null);
  assert.equal(mod.dataCanvasPurgeReceipt(canvasReceipt({ space_ref: "space_2" }), REQUEST, ORG, SPACE), null);
});

test("dataSpaceDeletionReceipt returns null when org_id or space_ref is missing or differs (W3-FIX-17)", () => {
  assert.deepEqual(mod.dataSpaceDeletionReceipt(retrievalReceipt(), REQUEST, ORG, SPACE), {
    status: "partial",
    receiptRef: `data:${REQUEST}`,
    detail: "Data owner receipt: retrieval bindings remain",
  });
  for (const over of [
    { org_id: undefined },
    { org_id: "org_other" },
    { space_ref: undefined },
    { space_ref: "space_2" },
    { request_id: "req-2" },
    { owner_plane: "model" },
  ]) {
    assert.equal(mod.dataSpaceDeletionReceipt(retrievalReceipt(over), REQUEST, ORG, SPACE), null, JSON.stringify(over));
  }
});

// The Space lookup that gates every mint. It must return the org only when exactly one Space carries the ref.
function fakeSpacesDb(rows: Array<Record<string, unknown>>) {
  return {
    db: {
      query(table: string) {
        return {
          withIndex(_index: string, build: (q: any) => unknown) {
            const conditions: Array<[string, unknown]> = [];
            build({
              eq(field: string, value: unknown) {
                conditions.push([field, value]);
                return this;
              },
            });
            const matches = table === "spaces" ? rows.filter((row) => conditions.every(([field, value]) => row[field] === value)) : [];
            return { take: async (limit: number) => matches.slice(0, limit) };
          },
        };
      },
    },
  };
}

test("spaceOrgForDeletion answers the Space's organization only when exactly one Space carries the ref (W3-FIX-10)", async () => {
  const lookup = (rows: Array<Record<string, unknown>>, spaceRef: string) =>
    (mod.spaceOrgForDeletion as any)._handler(fakeSpacesDb(rows), { spaceRef });
  assert.equal(await lookup([{ spaceRef: SPACE, externalOrgId: ORG }], SPACE), ORG);
  assert.equal(await lookup([], SPACE), null);
  assert.equal(await lookup([{ spaceRef: "space_2", externalOrgId: ORG }], SPACE), null);
  assert.equal(await lookup([{ spaceRef: SPACE, externalOrgId: ORG }, { spaceRef: SPACE, externalOrgId: "org_other" }], SPACE), null);
});

// ---------------------------------------------------------------------------------------------------------------------
// deliverOne: a recording ctx and a stubbed fetch. Each trace is the ordered list of what the delivery did.
// ---------------------------------------------------------------------------------------------------------------------

type FetchCall = { url: string; method: string; headers: Record<string, string>; body: any };
type FetchEvent = { op: "fetch" } & FetchCall;
type CtxEvent = { op: "mutation" | "query" | "schedule"; name: string; args: any; delay?: number };
type Event = FetchEvent | CtxEvent;
type Responder = (call: FetchCall) => Response | Promise<Response>;
type Leg = "mint" | "leg1" | "leg2";

const LEG_BY_URL: Record<string, Leg> = { [AUTH_MINT_URL]: "mint", [LEG1_URL]: "leg1", [LEG2_URL]: "leg2" };
const SHORT: Record<string, string> = {
  "spaces:claimNextDataDeletionOwnerDelivery": "claim",
  "spaces:releaseDataDeletionOwnerDelivery": "release",
  "spaces:acknowledgeDataDeletionOwnerDelivery": "ack",
  "spaceDeletionData:spaceOrgForDeletion": "space-org",
  "spaceDeletionData:purgeSpacePrivatePanelPointers": "panel-purge",
  "spaceDeletionData:deliverOne": "retry",
};

function label(event: Event): string {
  if (event.op === "fetch") return `fetch:${LEG_BY_URL[event.url] ?? "unexpected"}`;
  return SHORT[event.name] ?? event.name;
}

function trace(events: Event[]): string[] {
  return events.map(label);
}

function argsOf(events: Event[], short: string): any {
  const found = events.filter((event) => label(event) === short);
  assert.equal(found.length, 1, `expected one ${short}, saw ${found.length}`);
  return (found[0] as CtxEvent).args;
}

function fetchOf(events: Event[], leg: Leg): FetchCall {
  const found = events.filter((event) => event.op === "fetch" && LEG_BY_URL[event.url] === leg);
  assert.equal(found.length, 1, `expected one ${leg} call, saw ${found.length}`);
  return found[0] as FetchCall;
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

function text(status: number, body: string): Response {
  return new Response(body, { status, headers: { "Content-Type": "text/html" } });
}

function inMs(ms: number): string {
  return new Date(Date.now() + ms).toISOString();
}

const DEFAULT_RESPONDERS: Record<Leg, Responder> = {
  mint: () => json(200, { token: TOKEN, expiresAt: inMs(60_000) }),
  leg1: () => json(200, retrievalReceipt()),
  leg2: () => json(200, canvasReceipt()),
};

type DeliverOptions = {
  env?: Partial<Record<EnvKey, string>>;
  claim?: unknown;
  spaceOrg?: string | null;
  respond?: Partial<Record<Leg, Responder>>;
};

type Delivery = { result: any; events: Event[] };

async function deliver(options: DeliverOptions = {}): Promise<Delivery> {
  const deliverOne = (loadWith(options.env ?? {}).deliverOne as any)._handler;
  const events: Event[] = [];
  const unexpected: string[] = [];
  const responders = { ...DEFAULT_RESPONDERS, ...options.respond };
  const claimed = options.claim === undefined ? CLAIM : options.claim;
  const retryAt = Date.now() + 60_000;
  const ctx = {
    async runMutation(fn: unknown, args: any) {
      const name = getFunctionName(fn as any);
      events.push({ op: "mutation", name, args });
      if (name === "spaces:claimNextDataDeletionOwnerDelivery") return claimed;
      if (name === "spaces:releaseDataDeletionOwnerDelivery") return retryAt;
      if (name === "spaces:acknowledgeDataDeletionOwnerDelivery") return { status: args.status, finalized: false };
      if (name === "spaceDeletionData:purgeSpacePrivatePanelPointers") return { deleted: 0, more: false };
      throw new Error(`test ctx: unexpected mutation ${name}`);
    },
    async runQuery(fn: unknown, args: any) {
      const name = getFunctionName(fn as any);
      events.push({ op: "query", name, args });
      if (name === "spaceDeletionData:spaceOrgForDeletion") return options.spaceOrg === undefined ? ORG : options.spaceOrg;
      throw new Error(`test ctx: unexpected query ${name}`);
    },
    scheduler: {
      async runAfter(delay: number, fn: unknown, args: any) {
        events.push({ op: "schedule", name: getFunctionName(fn as any), args, delay });
      },
    },
  };
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async (input: any, init: any = {}) => {
    const url = String(input);
    const call: FetchCall = {
      url,
      method: init.method ?? "GET",
      headers: { ...(init.headers ?? {}) },
      body: typeof init.body === "string" ? JSON.parse(init.body) : undefined,
    };
    events.push({ op: "fetch", ...call });
    const leg = LEG_BY_URL[url];
    if (!leg) {
      unexpected.push(url);
      throw new Error(`test fetch: unexpected URL ${url}`);
    }
    return responders[leg](call);
  }) as typeof fetch;
  try {
    const result = await deliverOne(ctx, { workerId: WORKER });
    assert.deepEqual(unexpected, [], "every fetch must go to one of the three expected URLs");
    return { result, events };
  } finally {
    globalThis.fetch = originalFetch;
  }
}

function assertReleased(delivery: Delivery, reason: string, expected: string[]): void {
  assert.deepEqual(trace(delivery.events), expected);
  assert.equal(delivery.result.status, "retry_scheduled");
  const release = argsOf(delivery.events, "release");
  assert.equal(release.eventId, EVENT);
  assert.equal(release.workerId, WORKER);
  assert.equal(release.error, reason);
  assert.equal(trace(delivery.events).includes("ack"), false, "a released delivery is never acknowledged");
}

test("deliverOne acknowledges once with the combined status, after both Data legs and the panel purge answered", async () => {
  const delivery = await deliver();
  assert.deepEqual(trace(delivery.events), ["claim", "space-org", "fetch:mint", "fetch:leg1", "fetch:leg2", "panel-purge", "ack"]);
  assert.deepEqual(delivery.result, { eventId: EVENT, status: "partial" });
  const ack = argsOf(delivery.events, "ack");
  assert.equal(ack.eventId, EVENT);
  assert.equal(ack.workerId, WORKER);
  assert.equal(ack.status, "partial");
  assert.equal(ack.receiptRef, `data:${REQUEST}`);
  assert.equal(ack.detail, "Data owner receipt: retrieval bindings remain; Data canvas purge receipt: succeeded");
  assert.equal(JSON.stringify(ack).includes(TOKEN), false, "the minted token is never written to a receipt");
});

test("one org-bound token is minted for the request's organization and presented to both Data legs (W3-FIX-10)", async () => {
  const delivery = await deliver();
  const mint = fetchOf(delivery.events, "mint");
  assert.equal(mint.url, AUTH_MINT_URL, "the trailing slash of AUTH_CORE_URL is trimmed");
  assert.equal(mint.method, "POST");
  assert.equal(mint.headers["x-service-id"], "control-space-deletion");
  assert.equal(mint.headers["x-service-api-key"], CONTROL_KEY);
  assert.equal(mint.body.orgId, ORG);
  assert.deepEqual(mint.body.scopes, ["data:space-binding:delete", "data:space-canvas:delete"]);
  assert.equal(fetchOf(delivery.events, "leg1").url, LEG1_URL);
  assert.equal(fetchOf(delivery.events, "leg2").url, LEG2_URL);
  for (const leg of ["leg1", "leg2"] as const) {
    const call = fetchOf(delivery.events, leg);
    assert.equal(call.headers.Authorization, `Bearer ${TOKEN}`, `${leg} presents the minted token`);
    assert.equal(call.body.deletion_request_id, REQUEST);
    assert.equal(call.body.space_ref, SPACE);
    assert.equal(call.body.idempotency_key, `space-delete:${REQUEST}`);
  }
});

test("a Space whose externalOrgId differs from the request, or that is missing or ambiguous, releases before any mint (W3-FIX-10)", async () => {
  for (const spaceOrg of ["org_other", null]) {
    const delivery = await deliver({ spaceOrg });
    assertReleased(delivery, "Space organization does not match the deletion request", ["claim", "space-org", "release", "retry"]);
  }
});

test("a deletion request with no organization releases before the Space lookup or any mint", async () => {
  const delivery = await deliver({ claim: { ...CLAIM, request: { ...CLAIM.request, externalOrgId: "   " } } });
  assertReleased(delivery, "Data deletion request has no organization", ["claim", "release", "retry"]);
});

const EXPIRES_IN_THE_SKEW_WINDOW = inMs(5_000);
const MINT_REFUSALS: Array<[string, Responder, string]> = [
  ["401", () => json(401, { error: "unauthorized" }), "auth_token_rejected: token authority refused with HTTP 401"],
  ["403", () => json(403, { error: "forbidden" }), "auth_token_rejected: token authority refused with HTTP 403"],
  ["503", () => json(503, { error: "organization_directory_unavailable" }), "auth_token_unavailable: token authority returned HTTP 503"],
  ["a network error", () => { throw new TypeError("fetch failed"); }, "auth_token_unavailable: token authority request failed"],
  ["a non-JSON body", () => text(200, "<html>gateway</html>"), "auth_token_invalid: token authority returned an invalid body"],
  ["an expired token", () => json(200, { token: TOKEN, expiresAt: inMs(-1_000) }), "auth_token_invalid: token authority returned an unusable token"],
  ["a token inside the skew window", () => json(200, { token: TOKEN, expiresAt: EXPIRES_IN_THE_SKEW_WINDOW }), "auth_token_invalid: token authority returned an unusable token"],
  ["a missing token", () => json(200, { expiresAt: inMs(60_000) }), "auth_token_invalid: token authority returned an unusable token"],
  ["a blank token", () => json(200, { token: "  ", expiresAt: inMs(60_000) }), "auth_token_invalid: token authority returned an unusable token"],
];

for (const [name, mint, reason] of MINT_REFUSALS) {
  test(`a token authority answer of ${name} releases without calling a Data leg or acknowledging (W3-FIX-10)`, async () => {
    const delivery = await deliver({ respond: { mint } });
    assertReleased(delivery, reason, ["claim", "space-org", "fetch:mint", "release", "retry"]);
  });
}

const LEG1_MISMATCHES: Array<[string, Record<string, unknown>]> = [
  ["another org_id", { org_id: "org_other" }],
  ["no org_id", { org_id: undefined }],
  ["another space_ref", { space_ref: "space_2" }],
  ["no space_ref", { space_ref: undefined }],
  ["another request_id", { request_id: "req-2" }],
  ["a non-data owner_plane", { owner_plane: "model" }],
];

for (const [name, over] of LEG1_MISMATCHES) {
  test(`a retrieval receipt with ${name} releases before the canvas leg and never acknowledges (W3-FIX-10, W3-FIX-17)`, async () => {
    const delivery = await deliver({ respond: { leg1: () => json(200, retrievalReceipt(over)) } });
    assertReleased(
      delivery,
      "Data Space deletion receipt did not match the claimed request, organization or Space",
      ["claim", "space-org", "fetch:mint", "fetch:leg1", "release", "retry"],
    );
  });
}

const LEG2_MISMATCHES: Array<[string, Record<string, unknown>]> = [
  ["another org_id", { org_id: "org_other" }],
  ["no org_id", { org_id: undefined }],
  ["another space_ref", { space_ref: "space_2" }],
  ["no space_ref", { space_ref: undefined }],
  ["another request_id", { request_id: "req-2" }],
  ["a non-data owner_plane", { owner_plane: "model" }],
];

for (const [name, over] of LEG2_MISMATCHES) {
  test(`a canvas purge receipt with ${name} releases without acknowledging (WS19-S8, W3-FIX-17)`, async () => {
    const delivery = await deliver({ respond: { leg2: () => json(200, canvasReceipt(over)) } });
    assertReleased(
      delivery,
      "Data canvas purge receipt did not match the claimed request, organization or Space",
      ["claim", "space-org", "fetch:mint", "fetch:leg1", "fetch:leg2", "release", "retry"],
    );
  });
}

const UNSET_CONFIGURATION: Array<[EnvKey, string]> = [
  ["DATA_SPACE_DELETION_URL", "DATA_SPACE_DELETION_URL is not configured"],
  ["DATA_SPACE_CANVAS_DELETION_URL", "DATA_SPACE_CANVAS_DELETION_URL is not configured"],
  ["AUTH_CORE_URL", "AUTH_CORE_URL is not configured"],
  ["CONTROL_SPACE_DELETION_SERVICE_API_KEY", "CONTROL_SPACE_DELETION_SERVICE_API_KEY is not configured"],
];

for (const [key, reason] of UNSET_CONFIGURATION) {
  test(`${key} unset releases before any call and never acknowledges (WS19-S8)`, async () => {
    const delivery = await deliver({ env: { [key]: undefined } });
    assertReleased(delivery, reason, ["claim", "release", "retry"]);
  });
}

test("a blank DATA_SPACE_CANVAS_DELETION_URL counts as unset and releases before any call (WS19-S8)", async () => {
  const delivery = await deliver({ env: { DATA_SPACE_CANVAS_DELETION_URL: "   " } });
  assertReleased(delivery, "DATA_SPACE_CANVAS_DELETION_URL is not configured", ["claim", "release", "retry"]);
});

const LEG1_FAILURES: Array<[string, Responder, string]> = [
  ["HTTP 500", () => json(500, { error: "internal" }), "Data Space deletion returned HTTP 500"],
  ["a network error", () => { throw new TypeError("fetch failed"); }, "Data Space deletion request failed"],
  ["a non-JSON receipt", () => text(200, "<html>proxy</html>"), "Data Space deletion returned an invalid receipt"],
];

for (const [name, respond, reason] of LEG1_FAILURES) {
  test(`the retrieval leg answering ${name} releases without calling the canvas leg (W3-FIX-10)`, async () => {
    const delivery = await deliver({ respond: { leg1: respond } });
    assertReleased(delivery, reason, ["claim", "space-org", "fetch:mint", "fetch:leg1", "release", "retry"]);
  });
}

const LEG2_FAILURES: Array<[string, Responder, string]> = [
  ["HTTP 404", () => json(404, { error: "not_found" }), "Data canvas purge returned HTTP 404"],
  ["HTTP 500", () => json(500, { error: "internal" }), "Data canvas purge returned HTTP 500"],
  ["a network error", () => { throw new TypeError("fetch failed"); }, "Data canvas purge request failed"],
  ["a non-JSON receipt", () => text(200, "<html>proxy</html>"), "Data canvas purge returned an invalid receipt"],
];

for (const [name, respond, reason] of LEG2_FAILURES) {
  test(`the canvas leg answering ${name} releases without acknowledging (WS19-S8)`, async () => {
    const delivery = await deliver({ respond: { leg2: respond } });
    assertReleased(delivery, reason, ["claim", "space-org", "fetch:mint", "fetch:leg1", "fetch:leg2", "release", "retry"]);
  });
}
