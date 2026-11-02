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
// Space action contracts live in per-workstream fragments (P-SPA1); their ids
// belong to the registry set exactly like the ones in action-registry.ts.
const spaceFragmentsDirectory = path.join(verevonDirectory, 'src/shared/actions/domains/spaces');

function actionRegistryIds(source) {
  return new Set([...source.matchAll(/\bid:\s*'([^']+)'/g)].map((match) => match[1]));
}

function spaceFragmentSources(directory) {
  if (!fs.existsSync(directory)) return [];
  return fs
    .readdirSync(directory)
    .filter((name) => name.endsWith('.ts') && !name.endsWith('.test.ts'))
    .sort()
    .map((name) => fs.readFileSync(path.join(directory, name), 'utf8'));
}

function allRegistryIds(registrySource, fragmentSources) {
  const ids = actionRegistryIds(registrySource);
  for (const source of fragmentSources) {
    for (const id of actionRegistryIds(source)) ids.add(id);
  }
  return ids;
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
  const registryIds = allRegistryIds(registrySource, spaceFragmentSources(spaceFragmentsDirectory));
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

test('an action id present only in a Space fragment is part of the registry set', () => {
  const registrySource = "export const actionRegistry = [{ id: 'tickets.create' }]";
  const fragmentSource = "export const fragment = [{ id: 'spaces.fragment_only_probe' }]";

  assert.equal(actionRegistryIds(registrySource).has('spaces.fragment_only_probe'), false);
  const ids = allRegistryIds(registrySource, [fragmentSource]);
  assert.equal(ids.has('spaces.fragment_only_probe'), true);
  assert.equal(ids.has('tickets.create'), true);
});

test('the live Space fragments are discovered and carry the moved base actions', () => {
  const ids = allRegistryIds('', spaceFragmentSources(spaceFragmentsDirectory));
  assert.ok(ids.has('spaces.create_room'), 'base fragment ids must be read from domains/spaces');
});

// WS01-S5 and WS03-S4. Independent of the whole-set equality test above (the registry/handlers
// drift it reports predates the Space work): each new id is declared in its own fragment, is part
// of the registry set, and has an execute_action arm that reaches a named dispatch_ function.
for (const [fragment, ids] of [
  ['ws01.ts', ['spaces.create_post', 'spaces.reply_in_thread']],
  ['ws03.ts', ['spaces.set_effect_entitlements', 'spaces.apply_effect_preset']],
  ['ws04.ts', ['spaces.update_room_details', 'spaces.change_member_role', 'spaces.leave_room']],
  ['ws08.ts', ['spaces.set_agent_wake_policy']],
  ['ws12.ts', ['spaces.open_dm', 'spaces.hide_space']],
  ['ws16.ts', ['spaces.update_sidebar']],
]) {
  test(`${fragment} action ids are in the registry set and reach a gateway dispatcher`, () => {
    const fragmentSource = fs.readFileSync(path.join(spaceFragmentsDirectory, fragment), 'utf8');
    const fragmentIds = actionRegistryIds(fragmentSource);
    const registryIds = allRegistryIds('', spaceFragmentSources(spaceFragmentsDirectory));
    const handlerSource = fs.readFileSync(handlerPath, 'utf8');
    for (const actionId of ids) {
      assert.ok(fragmentIds.has(actionId), `${actionId} is declared in ${fragment}`);
      assert.ok(registryIds.has(actionId), `${actionId} is in the registry set`);
      const quotedId = actionId.replace('.', '\\.');
      assert.match(
        handlerSource,
        new RegExp(`"${quotedId}"[\\s\\S]{0,320}?=>[\\s\\S]{0,160}?dispatch_`),
        `${actionId} must reach a named owner dispatcher`,
      );
    }
  });
}

// WS04-S2 and WS04-S6: the room admin actions are human-only. None of them may become model
// executable: their model-access row is a closed `none` with `eligible: false`, so the catalog
// manifest and the agent tool list never offer them.
test('the WS04 room admin actions are not model executable', () => {
  const modelAccessSource = fs.readFileSync(
    path.join(verevonDirectory, 'src/shared/actions/model-access.ts'),
    'utf8',
  );
  for (const actionId of [
    'spaces.update_room_details',
    'spaces.change_member_role',
    'spaces.leave_room',
  ]) {
    const row = modelAccessSource.match(new RegExp(`'${actionId.replace('.', '\\.')}':\\s*([^\\n]+)`));
    assert.ok(row, `${actionId} has a model-access row`);
    assert.match(row[1], /^exempt\(/, `${actionId} is a closed (access none) row`);
    assert.doesNotMatch(row[1], /eligible:\s*true/, `${actionId} must not be eligible`);
  }
});

// WS16-S8 and WS12-S3: the sidebar layout and the direct message actions are human-only as well.
test('the WS16-S8, WS12-S3 and WS08-S5 actions are not model executable', () => {
  const modelAccessSource = fs.readFileSync(
    path.join(verevonDirectory, 'src/shared/actions/model-access.ts'),
    'utf8',
  );
  for (const actionId of [
    'spaces.update_sidebar',
    'spaces.open_dm',
    'spaces.hide_space',
    // WS08-S5: who may wake an agent is a human decision; an agent cannot govern itself.
    'spaces.set_agent_wake_policy',
  ]) {
    const row = modelAccessSource.match(new RegExp(`'${actionId.replace('.', '\\.')}':\\s*([^\\n]+)`));
    assert.ok(row, `${actionId} has a model-access row`);
    assert.match(row[1], /^exempt\(/, `${actionId} is a closed (access none) row`);
    assert.doesNotMatch(row[1], /eligible:\s*true/, `${actionId} must not be eligible`);
  }
});
