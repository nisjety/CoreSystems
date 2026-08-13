import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const scriptsDirectory = path.dirname(fileURLToPath(import.meta.url));
const gatewayDirectory = path.resolve(scriptsDirectory, '..');
const verevonDirectory = path.resolve(gatewayDirectory, '..', '..');
const registryPath = path.join(verevonDirectory, 'src/shared/actions/action-registry.ts');
const handlerPath = path.join(gatewayDirectory, 'src/domains/actions/handlers.rs');

function actionRegistryIds(source) {
  return new Set([...source.matchAll(/\bid:\s*'([^']+)'/g)].map((match) => match[1]));
}

function handlerActionIds(source) {
  // Action IDs only appear as dotted quoted literals in the execute_action
  // match. The legacy brreg underscore alias is deliberately ignored because
  // it is not a registry contract ID.
  return new Set(
    [...source.matchAll(/"([a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+)"/g)].map((match) => match[1]),
  );
}

test('every registered action has exactly one live gateway dispatcher contract', () => {
  const registrySource = fs.readFileSync(registryPath, 'utf8');
  const handlerSource = fs.readFileSync(handlerPath, 'utf8');
  const registryIds = actionRegistryIds(registrySource);
  const dispatcherIds = handlerActionIds(handlerSource);

  assert.ok(registryIds.size > 0, 'the registry must contain action contracts');
  assert.deepEqual([...dispatcherIds].sort(), [...registryIds].sort());

  for (const actionId of registryIds) {
    const quotedId = actionId.replace('.', '\\.')
    assert.match(
      handlerSource,
      new RegExp(`"${quotedId}"[\\s\\S]{0,320}?=>[\\s\\S]{0,160}?dispatch_`),
      `${actionId} must reach a named owner dispatcher`,
    );
  }
});
