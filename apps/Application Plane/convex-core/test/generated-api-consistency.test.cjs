const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const { API_DTS, API_JS, convexVersion, functionModulePaths, generateApiFiles } = require('./helpers/convex-api-codegen.cjs');

// The `convex codegen` CLI needs a configured deployment, but convex/_generated/
// api.d.ts and api.js are a pure function of the function-module list. The
// helper reproduces them offline from the installed convex package's own
// template and prettier, and the last tests below byte-compare the checked-in
// files against that output. Regenerate with:
//   node test/helpers/convex-api-codegen.cjs --write
// The structural tests (imports and fullApi aliases) keep a readable failure.

const convexDirectory = path.resolve(__dirname, '..', 'convex');
const generated = fs.readFileSync(path.join(convexDirectory, '_generated', 'api.d.ts'), 'utf8');

// Codegen registers each top-level .ts file as a module, except declarations,
// *.test.ts, schema.ts (the schema, not functions) and auth.config.ts. Each file
// under schema/ is registered as "schema/<name>".
const NOT_FUNCTION_MODULES = new Set(['schema', 'auth.config']);

function expectedModules() {
  const modules = [];
  for (const entry of fs.readdirSync(convexDirectory, { withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith('.ts')) continue;
    if (entry.name.endsWith('.d.ts') || entry.name.endsWith('.test.ts')) continue;
    const name = entry.name.slice(0, -'.ts'.length);
    if (!NOT_FUNCTION_MODULES.has(name)) modules.push(name);
  }
  for (const entry of fs.readdirSync(path.join(convexDirectory, 'schema'), { withFileTypes: true })) {
    if (entry.isFile() && entry.name.endsWith('.ts')) {
      modules.push(`schema/${entry.name.slice(0, -'.ts'.length)}`);
    }
  }
  return modules.sort();
}

// module path -> namespace alias, from `import type * as alias from "../module.js"`
const imports = new Map(
  [...generated.matchAll(/^import type \* as (\w+) from "\.\.\/(.+)\.js";$/gm)].map((match) => [match[2], match[1]]),
);

// module name -> namespace alias, from `name: typeof alias;` in fullApi
const mapped = new Map(
  [...generated.matchAll(/^\s+(?:"([^"]+)"|(\w+)): typeof (\w+);$/gm)].map((match) => [match[1] ?? match[2], match[3]]),
);

test('api.d.ts imports exactly the function modules codegen would register', () => {
  assert.deepEqual([...imports.keys()].sort(), expectedModules());
});

test('api.d.ts maps every imported module under the same alias', () => {
  assert.deepEqual([...mapped.keys()].sort(), expectedModules());
  for (const [moduleName, alias] of imports) {
    assert.equal(mapped.get(moduleName), alias, `fullApi alias for ${moduleName}`);
  }
});

test('api.d.ts still declares the fullApi object and the public and internal API exports', () => {
  assert.match(generated, /declare const fullApi: ApiFromModules<\{/);
  assert.match(generated, /export declare const api: FilterApi<\s*typeof fullApi,\s*FunctionReference<any, "public">\s*>;/);
  assert.match(generated, /export declare const internal: FilterApi<\s*typeof fullApi,\s*FunctionReference<any, "internal">\s*>;/);
});

test('the codegen helper module list matches the structural expectation', () => {
  assert.deepEqual(functionModulePaths().map((modulePath) => modulePath.replace(/\.ts$/, '')), expectedModules());
});

test('api.d.ts is byte-equal to the pinned convex codegen output', async () => {
  const { DTS } = await generateApiFiles();
  assert.equal(
    fs.readFileSync(API_DTS, 'utf8'),
    DTS,
    `convex/_generated/api.d.ts differs from convex ${convexVersion()} codegen; run node test/helpers/convex-api-codegen.cjs --write`,
  );
});

test('api.js is byte-equal to the pinned convex codegen output', async () => {
  const { JS } = await generateApiFiles();
  assert.equal(
    fs.readFileSync(API_JS, 'utf8'),
    JS,
    `convex/_generated/api.js differs from convex ${convexVersion()} codegen; run node test/helpers/convex-api-codegen.cjs --write`,
  );
});
