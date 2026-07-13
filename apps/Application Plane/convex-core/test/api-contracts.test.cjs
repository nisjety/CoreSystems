const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const convexDirectory = path.resolve(__dirname, '..', 'convex');

function exportedFunctions(moduleName) {
  const modulePath = path.join(convexDirectory, `${moduleName}.ts`);
  if (!fs.existsSync(modulePath)) return null;
  const source = fs.readFileSync(modulePath, 'utf8');
  return new Set(
    [...source.matchAll(/export\s+const\s+([A-Za-z_$][\w$]*)\s*=/g)]
      .map((match) => match[1]),
  );
}

test('every called Convex API function is defined by its target module', () => {
  const missing = [];
  for (const entry of fs.readdirSync(convexDirectory, { withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith('.ts')) continue;
    if (entry.name.endsWith('.d.ts')) continue;
    const source = fs.readFileSync(path.join(convexDirectory, entry.name), 'utf8');
    for (const match of source.matchAll(/\b(?:api|internal)\.([A-Za-z_$][\w$]*)\.([A-Za-z_$][\w$]*)/g)) {
      const [, moduleName, functionName] = match;
      const exports = exportedFunctions(moduleName);
      if (!exports?.has(functionName)) {
        missing.push(`${entry.name}: ${moduleName}.${functionName}`);
      }
    }
  }

  assert.deepEqual(missing, [], `called-but-undefined Convex APIs:\n${missing.join('\n')}`);
});

test('legacy jobs webhooks are removed instead of calling an orphan API', () => {
  const source = fs.readFileSync(path.join(convexDirectory, 'http.ts'), 'utf8');
  assert.doesNotMatch(source, /api\.jobs\./);
  assert.doesNotMatch(source, /\/webhooks\/(?:rag\/complete|job\/progress)/);
});
