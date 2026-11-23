const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

// Every literal index a Convex function reads must be declared on the table it
// reads. A name that is not declared fails only when the deployed function runs,
// and the fake ctx used by the other tests does not check index names, so this
// reads the source. The purge helpers pass `take(table, index, ...)` pairs.

const convexDirectory = path.resolve(__dirname, '..', 'convex');
const schemaDirectory = path.join(convexDirectory, 'schema');

function sourceFiles(directory) {
  const files = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== '_generated') files.push(...sourceFiles(full));
    } else if (entry.name.endsWith('.ts') && !entry.name.endsWith('.d.ts') && !entry.name.endsWith('.test.ts')) {
      files.push(full);
    }
  }
  return files;
}

// table name -> declared index names, read from `name: defineTable({...}).index(...)`
// blocks in schema.ts and schema/*.ts. Each block runs to the next table declaration.
function declaredIndexes() {
  const schemaFiles = [
    path.join(convexDirectory, 'schema.ts'),
    ...fs.readdirSync(schemaDirectory).filter((name) => name.endsWith('.ts')).map((name) => path.join(schemaDirectory, name)),
  ];
  const indexes = new Map();
  let tableCount = 0;
  let defineCount = 0;
  for (const file of schemaFiles) {
    const source = fs.readFileSync(file, 'utf8');
    defineCount += (source.match(/defineTable\(/g) || []).length;
    const starts = [...source.matchAll(/^\s+(\w+): defineTable\(/gm)];
    tableCount += starts.length;
    starts.forEach((start, index) => {
      const end = index + 1 < starts.length ? starts[index + 1].index : source.length;
      const block = source.slice(start.index, end);
      indexes.set(start[1], new Set([...block.matchAll(/\.index\(\s*["'](\w+)["']/g)].map((match) => match[1])));
    });
  }
  return { indexes, tableCount, defineCount };
}

function undeclaredReferences(pattern, indexes) {
  const missing = [];
  let checked = 0;
  for (const file of sourceFiles(convexDirectory)) {
    const source = fs.readFileSync(file, 'utf8');
    for (const match of source.matchAll(pattern)) {
      const [, table, index] = match;
      checked += 1;
      if (!indexes.get(table)?.has(index)) {
        missing.push(`${path.relative(convexDirectory, file)}: ${table}.${index}`);
      }
    }
  }
  return { checked, missing };
}

test('the schema reader sees every table declaration', () => {
  const { tableCount, defineCount } = declaredIndexes();
  assert.ok(defineCount > 0);
  assert.equal(tableCount, defineCount);
});

test('every literal query(table).withIndex(name) names an index declared on that table', () => {
  const { indexes } = declaredIndexes();
  const { checked, missing } = undeclaredReferences(
    /\.query\(\s*["'](\w+)["']\s*\)\s*\.withIndex\(\s*["'](\w+)["']/g,
    indexes,
  );
  // 234 references on 2026-11-23; a floor, so a reader regression that drops them fails.
  assert.ok(checked >= 234, `expected the literal index references to be read, saw ${checked}`);
  assert.deepEqual(missing, []);
});

test('every literal take(table, index) pair under convex/ names an index declared on that table', () => {
  const { indexes } = declaredIndexes();
  const { checked, missing } = undeclaredReferences(/\btake\(\s*["'](\w+)["']\s*,\s*["'](\w+)["']/g, indexes);
  // 18 pairs on 2026-11-23; a floor, so a reader regression that drops them fails.
  assert.ok(checked >= 18, `expected the take(table, index) pairs to be read, saw ${checked}`);
  assert.deepEqual(missing, []);
});
