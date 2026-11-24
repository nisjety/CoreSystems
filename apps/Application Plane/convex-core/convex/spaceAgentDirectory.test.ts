/**
 * The agent directory projection, its key check and its request parser (A-REC-03). Run with:
 *   cd "apps/Application Plane/convex-core" && npx tsx --test convex/spaceAgentDirectory.test.ts
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

import {
  AGENT_DIRECTORY_LIMIT,
  authorizeAgentDirectoryRequest,
  parseDirectoryRequest,
  projectAgentDirectory,
} from "./spaceAgentDirectory";

const ORG = "org-1";
const ROOM = "room-1";

function agent(id: string, name: string, extra: Record<string, unknown> = {}) {
  return { _id: id, name, description: `${name} helps`, status: "active", ...extra };
}

function binding(agentId: string, spaceRef = ROOM, extra: Record<string, unknown> = {}) {
  return { agentId, spaceRef, externalOrgId: ORG, status: "active", ...extra };
}

test("lists active org-visible agents by name prefix and marks the caller's room", () => {
  const listed = projectAgentDirectory(
    [agent("a1", "Triage"), agent("a2", "Tri-helper"), agent("a3", "Sales")],
    [binding("a1")],
    ORG,
    ROOM,
    " tri ",
  );
  assert.deepEqual(
    listed.map((entry) => [entry.agent_ref, entry.in_room]),
    [
      ["a2", false],
      ["a1", true],
    ],
  );
  assert.equal(listed[1].display_name, "Triage");
  assert.equal(listed[1].description, "Triage helps");
});

test("an exact name sorts first", () => {
  const listed = projectAgentDirectory([agent("a1", "Ada Helper"), agent("a2", "Ada")], [], ORG, ROOM, "ada");
  assert.equal(listed[0].agent_ref, "a2");
});

test("personal, draft and inactive definitions are never listed", () => {
  const listed = projectAgentDirectory(
    [
      agent("a1", "Kim personal", { visibility: "owner" }),
      agent("a2", "Kim draft", { status: "draft" }),
      agent("a3", "Kim off", { status: "inactive" }),
      agent("a4", "Kim org", { visibility: "org" }),
    ],
    [],
    ORG,
    ROOM,
    "kim",
  );
  assert.deepEqual(
    listed.map((entry) => entry.agent_ref),
    ["a4"],
  );
});

test("only an active binding in the caller's room, in the same organization, counts as in the room", () => {
  const agents = [agent("a1", "Kim")];
  for (const other of [
    binding("a1", "room-2"),
    binding("a1", ROOM, { status: "paused" }),
    binding("a1", ROOM, { status: "revoked" }),
    binding("a1", ROOM, { externalOrgId: "org-2" }),
  ]) {
    assert.equal(projectAgentDirectory(agents, [other], ORG, ROOM, "kim")[0].in_room, false);
  }
});

test("no answer names a room, a binding or a secret", () => {
  const text = JSON.stringify(
    projectAgentDirectory(
      [agent("a1", "Kim", { systemPrompt: "secret prompt", publicSecret: "pub-secret", tools: ["mail"] })],
      [binding("a1", ROOM, { displayName: "Kim of the board room", bindingRef: "bind-9" }), binding("a1", "room-secret-9")],
      ORG,
      ROOM,
      "kim",
    ),
  );
  for (const leaked of [ROOM, "room-secret-9", "board room", "bind-9", "secret prompt", "pub-secret", "mail"]) {
    assert.ok(!text.includes(leaked), `${leaked} leaked: ${text}`);
  }
});

test("an empty query lists nothing and the answer is capped", () => {
  const many = Array.from({ length: AGENT_DIRECTORY_LIMIT + 10 }, (_, index) => agent(`a${index}`, `Kim ${index}`));
  assert.equal(projectAgentDirectory(many, [], ORG, ROOM, "   ").length, 0);
  assert.equal(projectAgentDirectory(many, [], ORG, ROOM, "kim").length, AGENT_DIRECTORY_LIMIT);
});

function request(authorization?: string) {
  return new Request("http://convex.test/api/internal/space-agent-directory", {
    method: "POST",
    headers: authorization === undefined ? {} : { Authorization: authorization },
  });
}

const KEY = "k".repeat(40);

test("the key check fails closed while the key is unset or short", async () => {
  await assert.rejects(authorizeAgentDirectoryRequest(request(`Bearer ${KEY}`), {}), /not configured/);
  await assert.rejects(
    authorizeAgentDirectoryRequest(request("Bearer short"), { CONVEX_AGENT_DIRECTORY_KEY: "short" }),
    /not configured/,
  );
});

test("only execution-core's directory key opens the route", async () => {
  const env = {
    CONVEX_AGENT_DIRECTORY_KEY: KEY,
    CONVEX_CONTROL_PROJECTION_KEY: "p".repeat(40),
  };
  assert.equal(await authorizeAgentDirectoryRequest(request(`Bearer ${KEY}`), env), true);
  for (const wrong of [undefined, `Basic ${KEY}`, `Bearer ${"w".repeat(40)}`, `Bearer ${"p".repeat(40)}`]) {
    assert.equal(await authorizeAgentDirectoryRequest(request(wrong), env), false, String(wrong));
  }
});

test("the request is closed and bounded", () => {
  assert.deepEqual(parseDirectoryRequest(JSON.stringify({ org_id: " org-1 ", space_ref: ROOM, query: " Kim " })), {
    orgId: "org-1",
    spaceRef: ROOM,
    query: "Kim",
  });
  for (const bad of [
    "not json",
    "[]",
    JSON.stringify({ org_id: ORG, space_ref: ROOM }),
    JSON.stringify({ org_id: ORG, space_ref: ROOM, query: "   " }),
    JSON.stringify({ org_id: ORG, space_ref: ROOM, query: "a".repeat(101) }),
    JSON.stringify({ org_id: ORG, space_ref: ROOM, query: "Kim", other_room: "room-2" }),
    JSON.stringify({ org_id: 7, space_ref: ROOM, query: "Kim" }),
  ]) {
    assert.equal(parseDirectoryRequest(bad), null, bad);
  }
});

test("the route is registered once on its own path", () => {
  const http = fs.readFileSync(path.join(__dirname, "http.ts"), "utf8");
  assert.equal(http.split('path: "/api/internal/space-agent-directory"').length - 1, 1);
  assert.match(http, /handler: agentDirectory,/);
});
