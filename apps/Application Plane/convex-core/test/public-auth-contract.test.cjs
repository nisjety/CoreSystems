const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const convexDirectory = path.resolve(__dirname, '..', 'convex');

function publicFunctions(fileName) {
  const source = fs.readFileSync(path.join(convexDirectory, fileName), 'utf8');
  const starts = [...source.matchAll(/export\s+const\s+(\w+)\s*=\s*(query|mutation)\s*\(/g)];
  return starts.map((match, index) => ({
    name: match[1],
    kind: match[2],
    source: source.slice(match.index, starts[index + 1]?.index ?? source.length),
  }));
}

test('sensitive public Convex functions authorize inside their own handler', () => {
  const failures = [];
  for (const fileName of [
    'agentRuns.ts',
    'controlSessions.ts',
    'knowledgeQnA.ts',
    'plannerDocuments.ts',
    'projects.ts',
  ]) {
    for (const fn of publicFunctions(fileName)) {
      if (!/require(?:Identity|Viewer|Editor|Org)/.test(fn.source)) {
        failures.push(`${fileName}:${fn.name}`);
      }
    }
  }
  assert.deepEqual(
    failures,
    [],
    `public functions without in-handler authorization:\n${failures.join('\n')}`,
  );
});

test('Control session projection writes are internal-only', () => {
  const source = fs.readFileSync(path.join(convexDirectory, 'controlSessions.ts'), 'utf8');
  assert.match(source, /upsertControlSessionInternal\s*=\s*internalMutation/);
});

test('Control session reads verify active organization membership', () => {
  const source = fs.readFileSync(path.join(convexDirectory, 'controlSessions.ts'), 'utf8');
  assert.match(source, /const snapshot = await ctx\.db[\s\S]*requireViewerMembership\(ctx, snapshot\.externalOrgId\)/);
  assert.match(source, /externalOrgId: v\.string\(\)/);
});
