// Guard for sdks/generate.sh: the pinned generator (openapi-generator 7.22.0) cannot handle a schema whose
// whole type is the bare OpenAPI 3.1 `type: 'null'`. Python aborts in pydantic codegen, and TypeScript imports a
// `./Null` model it never writes. A nullable value is written as a type array, `type: [string, 'null']`, which
// 7.22.0 handles. This test fails on the bare form before the SDK regeneration (spa-verify) does.
//
// Run from the Quarry-v2 directory:
//   node --test scripts/tests/openapi-no-bare-null.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const spec = readFileSync(resolve(here, "..", "..", "docs", "openapi.yaml"), "utf8").split(/\r?\n/);

test("no schema in openapi.yaml has the bare type null", () => {
  const bare = [];
  spec.forEach((line, i) => {
    if (/^\s*(-\s+)?type:\s*['"]?null['"]?\s*(#.*)?$/.test(line)) bare.push(`${i + 1}: ${line.trim()}`);
  });
  assert.deepEqual(bare, [], `bare null type (use type: [<type>, 'null']): ${bare.join("; ")}`);
});

test("ProfileCookieView.value stays required and always-null in its description", () => {
  const start = spec.findIndex((l) => /^\s{4}ProfileCookieView:\s*$/.test(l));
  assert.notEqual(start, -1, "ProfileCookieView schema not found");
  let end = spec.findIndex((l, i) => i > start && /^\s{4}\S/.test(l));
  if (end === -1) end = spec.length;
  const block = spec.slice(start, end).join("\n");
  assert.match(block, /required:\s*\[[^\]]*\bvalue\b[^\]]*\]/, "value must stay required");
  assert.match(block, /value:\s*\n\s+type:\s*\[string, 'null'\]\s*\n\s+description:\s*Always null\. A cookie value is never returned\./);
});
