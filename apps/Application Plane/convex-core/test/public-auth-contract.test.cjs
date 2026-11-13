const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const convexDirectory = path.resolve(__dirname, '..', 'convex');
const authorizationCall = /require(?:Identity|Viewer|Editor|Org)/;

function publicFunctions(fileName) {
  return publicFunctionsInSource(fs.readFileSync(path.join(convexDirectory, fileName), 'utf8'));
}

function publicFunctionsInSource(source) {
  const starts = [...source.matchAll(/export\s+const\s+(\w+)\s*=\s*(query|mutation)\s*\(/g)];
  return starts.map((match, index) => {
    const block = source.slice(match.index, starts[index + 1]?.index ?? source.length);
    // `handler: getByRunIdHandler` names a function declared above its export, so the
    // export's own slice cannot contain the authorization call. Check the declaration too.
    const handlerName = /\bhandler:\s*(\w+)\s*[,}]/.exec(block)?.[1];
    return {
      name: match[1],
      kind: match[2],
      source: handlerName ? `${block}\n${namedHandlerSource(source, handlerName)}` : block,
    };
  });
}

// A top-level `function <name>(` declaration, up to the next top-level statement. A handler
// not declared in this file is not resolved, so it is judged on its export slice alone.
function namedHandlerSource(source, name) {
  const declaration = new RegExp(`^(?:export\\s+)?(?:async\\s+)?function\\s+${name}\\s*\\(`, 'm');
  const match = declaration.exec(source);
  if (!match) return '';
  const bodyStart = match.index + match[0].length;
  const next = source.slice(bodyStart).search(/^(?:export\s|async\s|function\s|const\s|let\s|type\s|\/\*\*)/m);
  return source.slice(match.index, next < 0 ? source.length : bodyStart + next);
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
      if (!authorizationCall.test(fn.source)) {
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

test('a named handler is checked through its own declaration', () => {
  const authorized = publicFunctionsInSource(`
export async function readHandler(ctx) {
  await requireViewerMembership(ctx, 'org');
  return null;
}
export const read = query({ args: {}, handler: readHandler });
`);
  const unauthorized = publicFunctionsInSource(`
export async function readHandler(ctx) {
  return ctx.db.query('notes').collect();
}
export const read = query({ args: {}, handler: readHandler });
`);
  assert.equal(authorized.length, 1);
  assert.match(authorized[0].source, authorizationCall);
  assert.doesNotMatch(unauthorized[0].source, authorizationCall);
});

test('Control session projection writes are internal-only', () => {
  const source = fs.readFileSync(path.join(convexDirectory, 'controlSessions.ts'), 'utf8');
  assert.match(source, /upsertControlSessionInternal\s*=\s*internalMutation/);
});

test('Control organization and membership projection writes are internal-only', () => {
  const natsSource = fs.readFileSync(path.join(convexDirectory, 'nats.ts'), 'utf8');
  const organizationsSource = fs.readFileSync(
    path.join(convexDirectory, 'organizations.ts'),
    'utf8',
  );
  const usersSource = fs.readFileSync(path.join(convexDirectory, 'users.ts'), 'utf8');
  assert.match(natsSource, /onOrganizationProjectionChanged\s*=\s*internalMutation/);
  assert.match(
    natsSource,
    /onOrganizationMembershipProjectionChanged\s*=\s*internalMutation/,
  );
  assert.match(organizationsSource, /createFromExternal\s*=\s*internalMutation/);
  assert.match(organizationsSource, /updateFromExternal\s*=\s*internalMutation/);
  assert.match(organizationsSource, /remove\s*=\s*internalMutation/);
  assert.match(usersSource, /createOrUpdateFromExternal\s*=\s*internalMutation/);
});

test('Control session reads verify active organization membership', () => {
  const source = fs.readFileSync(path.join(convexDirectory, 'controlSessions.ts'), 'utf8');
  assert.match(source, /const snapshot = await ctx\.db[\s\S]*requireViewerMembership\(ctx, snapshot\.externalOrgId\)/);
  assert.match(source, /externalOrgId: v\.string\(\)/);
});
