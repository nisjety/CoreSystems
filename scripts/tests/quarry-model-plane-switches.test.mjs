// QB-T17: every Quarry Model Plane switch defaults OFF in the Ingestion compose.
//
// The quarry-edge block of apps/Ingestion Plane/docker-compose.yml is the deployed
// configuration. A Model Plane feature whose switch defaults to 1 is refused by Auth
// Core on every call while the registry grants quarry-edge no model-gateway request,
// so the default must be 0 and each switch must be written out, not left implicit.
// Reads the compose file only: no Docker, no network.
//
// Run: node --test scripts/tests/quarry-model-plane-switches.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const composePath = resolve(root, "apps", "Ingestion Plane", "docker-compose.yml");
const compose = readFileSync(composePath, "utf8");

const SWITCHES = [
  "LLM_CLASSIFY_INTENT",
  "AUTOPROMPT",
  "PAGE_TITLE_ENRICH",
  "ANSWER_SYNTHESIS",
  "AI_FORMATS",
  "SEARCH_SUGGEST",
  "MODEL_AUDIO",
];

function assignmentLines(name) {
  // An assignment line, not a comment that mentions the switch.
  return compose
    .split(/\r?\n/)
    .filter((line) => !line.trim().startsWith("#"))
    .filter((line) => line.trim().startsWith(`QUARRY_EDGE__${name}:`));
}

for (const name of SWITCHES) {
  test(`QUARRY_EDGE__${name} is written out in the quarry-edge block and defaults to 0`, () => {
    const lines = assignmentLines(name);
    assert.equal(lines.length, 1, `exactly one QUARRY_EDGE__${name} assignment, found ${lines.length}`);
    // Shape: QUARRY_EDGE__X: "${QUARRY_EDGE_X:-0}"  (the default is the text after :-)
    const match = lines[0].match(/:\s*"?\$\{QUARRY_EDGE_[A-Z_]+:-([^}]*)\}"?\s*$/);
    assert.ok(match, `QUARRY_EDGE__${name} takes its value from an env var with a default: ${lines[0].trim()}`);
    assert.equal(match[1], "0", `QUARRY_EDGE__${name} must default to 0, found ${match[1]}`);
  });
}

test("AUTOPROMPT no longer defaults to 1", () => {
  const [line] = assignmentLines("AUTOPROMPT");
  assert.ok(line, "AUTOPROMPT assignment present");
  assert.doesNotMatch(line, /:-1\}/);
});
