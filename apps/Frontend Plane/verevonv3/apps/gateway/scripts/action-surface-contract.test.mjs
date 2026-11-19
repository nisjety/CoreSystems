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

const LEGACY_ALIAS_TABLE = /const LEGACY_ACTION_ALIASES\b/y;
const CFG_TEST_ATTRIBUTE = /#\[cfg\(test\)\]/y;
const RAW_STRING_START = /b?r(#*)"/y;
const CHAR_LITERAL = /'(?:\\u\{[0-9a-fA-F_]*\}|\\x[0-9a-fA-F]{2}|\\.|[^\\'\n])'/y;

function isIdentifierChar(ch) {
  return ch !== undefined && /[A-Za-z0-9_]/.test(ch);
}

// The index just past the comment, string or char literal that starts at `index`, or `index` itself.
// Braces inside these never move an item boundary.
function rustNoiseEnd(source, index) {
  const ch = source[index];
  if (ch === '/' && source[index + 1] === '/') {
    const newline = source.indexOf('\n', index);
    return newline === -1 ? source.length : newline;
  }
  if (ch === '/' && source[index + 1] === '*') {
    const close = source.indexOf('*/', index + 2);
    return close === -1 ? source.length : close + 2;
  }
  if (ch === '"') {
    for (let i = index + 1; i < source.length; i += 1) {
      if (source[i] === '\\') i += 1;
      else if (source[i] === '"') return i + 1;
    }
    return source.length;
  }
  if ((ch === 'r' || ch === 'b') && !isIdentifierChar(source[index - 1])) {
    RAW_STRING_START.lastIndex = index;
    const raw = RAW_STRING_START.exec(source);
    if (raw) {
      const close = source.indexOf(`"${raw[1]}`, index + raw[0].length);
      return close === -1 ? source.length : close + 1 + raw[1].length;
    }
  }
  if (ch === "'") {
    CHAR_LITERAL.lastIndex = index;
    if (CHAR_LITERAL.test(source)) return CHAR_LITERAL.lastIndex;
  }
  return index;
}

// The index just past the Rust item whose body or tail starts at `from`: the `}` closing its first
// brace block, or the `;` ending a braceless item. Only a `;` at depth 0 counts, so the `;` inside
// the `[&str; 4]` type of a const does not end it.
function rustItemEnd(source, from) {
  let depth = 0;
  for (let index = from; index < source.length; ) {
    const noiseEnd = rustNoiseEnd(source, index);
    if (noiseEnd > index) {
      index = noiseEnd;
      continue;
    }
    const ch = source[index];
    if (ch === '(' || ch === '[' || ch === '{') {
      depth += 1;
    } else if (ch === ')' || ch === ']') {
      depth -= 1;
    } else if (ch === '}') {
      depth -= 1;
      if (depth === 0) return index + 1;
    } else if (ch === ';' && depth === 0) {
      return index + 1;
    }
    index += 1;
  }
  return source.length;
}

// Where the skipped item that begins at `index` continues, or null when no skipped item starts there.
function skippedItemStart(source, index) {
  for (const pattern of [CFG_TEST_ATTRIBUTE, LEGACY_ALIAS_TABLE]) {
    pattern.lastIndex = index;
    if (pattern.test(source)) return pattern.lastIndex;
  }
  return null;
}

// The Rust source with the legacy web.* alias table and every #[cfg(test)] item removed. An alias key
// is not a registry contract ID, and a test literal names an ID only to assert on it, so neither is
// dispatch surface. Everything else is read exactly as before.
function dispatchSurfaceSource(source) {
  let kept = '';
  let copied = 0;
  for (let index = 0; index < source.length; ) {
    const noiseEnd = rustNoiseEnd(source, index);
    if (noiseEnd > index) {
      index = noiseEnd;
      continue;
    }
    const bodyStart = skippedItemStart(source, index);
    if (bodyStart !== null) {
      kept += source.slice(copied, index);
      index = rustItemEnd(source, bodyStart);
      copied = index;
      continue;
    }
    index += 1;
  }
  return kept + source.slice(copied);
}

function handlerActionIds(source) {
  // Action IDs only appear as dotted quoted literals in the execute_action
  // match. The legacy brreg underscore alias is deliberately ignored because
  // it is not a registry contract ID. The legacy web.* table and the
  // #[cfg(test)] items are not dispatch surface, so they are removed first.
  return new Set(
    [...dispatchSurfaceSource(source).matchAll(/"([a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+)"/g)].map(
      (match) => match[1],
    ),
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

// The WS11 attention, status and reminder actions (WS11-S3, S7, S10 and S11): the attention
// tables, the member status row and the reminder row all live in the Application plane.
const WS11_ACTION_IDS = [
  'spaces.mark_all_read',
  'spaces.mark_unread',
  'spaces.set_notify_level',
  'spaces.set_attention_item_state',
  'spaces.set_thread_follow',
  'spaces.set_status',
  'spaces.create_reminder',
  'spaces.cancel_reminder',
];

// WS01-S5 and WS03-S4. Independent of the whole-set equality test above (the registry/handlers
// drift it reports predates the Space work): each new id is declared in its own fragment, is part
// of the registry set, and has an execute_action arm that reaches a named dispatch_ function.
for (const [fragment, ids] of [
  ['ws01.ts', ['spaces.create_post', 'spaces.reply_in_thread']],
  ['ws03.ts', ['spaces.set_effect_entitlements', 'spaces.apply_effect_preset']],
  [
    'ws04.ts',
    [
      'spaces.update_room_details',
      'spaces.change_member_role',
      'spaces.leave_room',
      'spaces.join_room',
      'spaces.update_room_settings',
    ],
  ],
  ['ws08.ts', ['spaces.set_agent_wake_policy']],
  ['ws09.ts', ['spaces.stop_run']],
  ['ws10.ts', ['spaces.decide_approval', 'spaces.revoke_approval_grant']],
  ['ws11.ts', WS11_ACTION_IDS],
  [
    'ws12.ts',
    [
      'spaces.open_dm',
      'spaces.hide_space',
      'spaces.edit_message',
      'spaces.delete_message',
      'spaces.toggle_reaction',
    ],
  ],
  [
    'ws14.ts',
    ['space.files.attach', 'space.files.list', 'space.files.read_text', 'space.files.delete'],
  ],
  ['ws16.ts', ['spaces.update_sidebar']],
  [
    'ws17.ts',
    [
      'agents.create_personal',
      'agents.update_access',
      'agents.set_primary',
      'spaces.withdraw_agent',
      'spaces.open_private_panel',
      'memory.declassify_to_space',
      'memory.retract_from_space',
    ],
  ],
  ['ws18.ts', ['spaces.set_agent_self_api', 'spaces.set_agent_canvas_tools']],
  ['ws19.ts', ['spaces.save_canvas', 'spaces.restore_canvas_version']],
  ['ws20.ts', ['spaces.browser_run.start']],
]) {
  test(`${fragment} action ids are in the registry set and reach a gateway dispatcher`, () => {
    const fragmentSource = fs.readFileSync(path.join(spaceFragmentsDirectory, fragment), 'utf8');
    const fragmentIds = actionRegistryIds(fragmentSource);
    const registryIds = allRegistryIds('', spaceFragmentSources(spaceFragmentsDirectory));
    const handlerSource = fs.readFileSync(handlerPath, 'utf8');
    for (const actionId of ids) {
      assert.ok(fragmentIds.has(actionId), `${actionId} is declared in ${fragment}`);
      assert.ok(registryIds.has(actionId), `${actionId} is in the registry set`);
      const quotedId = actionId.replaceAll('.', '\\.');
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
test('the WS16-S8, WS12-S3, WS08-S5 and WS12-S8 actions are not model executable', () => {
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
    // WS12-S8: a reaction is a person's own mark; an agent cannot react as someone.
    'spaces.toggle_reaction',
  ]) {
    const row = modelAccessSource.match(new RegExp(`'${actionId.replace('.', '\\.')}':\\s*([^\\n]+)`));
    assert.ok(row, `${actionId} has a model-access row`);
    assert.match(row[1], /^exempt\(/, `${actionId} is a closed (access none) row`);
    assert.doesNotMatch(row[1], /eligible:\s*true/, `${actionId} must not be eligible`);
  }
});

// WS10-S4 and WS14-S7: deciding an approval, revoking a standing rule and the room file actions are
// human-only. An agent must not decide its own approval, revoke the rule that limits it, or reach a
// room's files through the action surface.
test('the WS10-S4 approval actions and the WS14-S7 room file actions are not model executable', () => {
  const modelAccessSource = fs.readFileSync(
    path.join(verevonDirectory, 'src/shared/actions/model-access.ts'),
    'utf8',
  );
  for (const actionId of [
    'spaces.decide_approval',
    'spaces.revoke_approval_grant',
    'space.files.attach',
    'space.files.list',
    'space.files.read_text',
    'space.files.delete',
  ]) {
    const row = modelAccessSource.match(new RegExp(`'${actionId.replaceAll('.', '\\.')}':\\s*([^\\n]+)`));
    assert.ok(row, `${actionId} has a model-access row`);
    assert.match(row[1], /^exempt\(/, `${actionId} is a closed (access none) row`);
    assert.doesNotMatch(row[1], /eligible:\s*true/, `${actionId} must not be eligible`);
  }
});

// W3-DEC-CANVAS (D2): the canvas-tools binding switch is a human decision. Its gateway arm reaches the
// Space canvas-tools dispatcher, and its model-access row is closed (`exempt`, not eligible), so an
// agent can neither offer canvas tools to itself nor see the switch in the model's action list.
test('spaces.set_agent_canvas_tools has a dispatcher arm and is not model executable', () => {
  const handlerSource = fs.readFileSync(handlerPath, 'utf8');
  assert.match(
    handlerSource,
    /"spaces\.set_agent_canvas_tools"[\s\S]{0,320}?=>[\s\S]{0,160}?dispatch_/,
    'spaces.set_agent_canvas_tools must reach a named owner dispatcher',
  );
  const modelAccessSource = fs.readFileSync(
    path.join(verevonDirectory, 'src/shared/actions/model-access.ts'),
    'utf8',
  );
  const row = modelAccessSource.match(/'spaces\.set_agent_canvas_tools':\s*([^\n]+)/);
  assert.ok(row, 'spaces.set_agent_canvas_tools has a model-access row');
  assert.match(row[1], /^exempt\(/, 'spaces.set_agent_canvas_tools is a closed (access none) row');
  assert.doesNotMatch(row[1], /eligible:\s*true/, 'spaces.set_agent_canvas_tools must not be eligible');
});

// The Wave 2 and 3 Space actions below change what a person is, owns or decides, or move their own
// UI state: none may become model executable. Each model-access row is a closed `exempt(...)` (access
// none) with no `eligible: true`, so neither the catalog manifest nor the agent tool list offers them.
test('the Wave 2 and Wave 3 Space actions are human-only: a closed model-access row each', () => {
  const modelAccessSource = fs.readFileSync(
    path.join(verevonDirectory, 'src/shared/actions/model-access.ts'),
    'utf8',
  );
  for (const actionId of [
    // WS04-S8: joining a room and changing its settings.
    'spaces.join_room',
    'spaces.update_room_settings',
    // WS09-S8: stopping a run is an approval authority.
    'spaces.stop_run',
    // WS11: read position, attention state, status text and reminders are the person's own.
    ...WS11_ACTION_IDS,
    // WS12-S7: an edit or a delete of a message.
    'spaces.edit_message',
    'spaces.delete_message',
    // WS17-S5 to S7b: personal agents, the private panel and personal memory leaving or returning.
    'agents.create_personal',
    'agents.update_access',
    'agents.set_primary',
    'spaces.withdraw_agent',
    'spaces.open_private_panel',
    'memory.declassify_to_space',
    'memory.retract_from_space',
    // WS18-S5: the self-API switch (the canvas-tools switch has its own test above).
    'spaces.set_agent_self_api',
    // WS19-S4: saving or restoring the room canvas.
    'spaces.save_canvas',
    'spaces.restore_canvas_version',
    // WS20-S5: starting a browser run from a room.
    'spaces.browser_run.start',
  ]) {
    const row = modelAccessSource.match(new RegExp(`'${actionId.replaceAll('.', '\\.')}':\\s*([^\\n]+)`));
    assert.ok(row, `${actionId} has a model-access row`);
    assert.match(row[1], /^exempt\(/, `${actionId} is a closed (access none) row`);
    assert.doesNotMatch(row[1], /eligible:\s*true/, `${actionId} must not be eligible`);
  }
});

// WS11-S3, S7, S10 and S11: the attention, status and reminder contracts name the Application plane
// as their owner, because the rows they change live there and nowhere else.
test('the WS11 attention, status and reminder actions are owned by the application plane', () => {
  const fragmentSource = fs.readFileSync(path.join(spaceFragmentsDirectory, 'ws11.ts'), 'utf8');
  for (const actionId of WS11_ACTION_IDS) {
    const owner = fragmentSource.match(
      new RegExp(`id:\\s*'${actionId.replaceAll('.', '\\.')}'[\\s\\S]*?ownerPlane:\\s*'([^']+)'`),
    );
    assert.ok(owner, `${actionId} names an owner plane`);
    assert.equal(owner[1], 'application', `${actionId} is owned by the application plane`);
  }
});

// WS09-S8: stopping a run has exactly one dispatcher arm in the live dispatch surface, so there is
// one place that decides who may stop one.
test('spaces.stop_run has exactly one dispatcher arm', () => {
  const surface = dispatchSurfaceSource(fs.readFileSync(handlerPath, 'utf8'));
  assert.equal([...surface.matchAll(/"spaces\.stop_run"/g)].length, 1);
  assert.match(surface, /"spaces\.stop_run"[\s\S]{0,320}?=>[\s\S]{0,160}?dispatch_/);
});

// The dispatchers the two WS10-S4 ids reach are the Space route functions, so the scope rules, the
// role gate and the typed errors exist once.
test('the WS10-S4 and WS14-S7 handler arms reach the Space dispatchers', () => {
  const handlerSource = fs.readFileSync(handlerPath, 'utf8');
  for (const [actionId, dispatcher] of [
    ['spaces.decide_approval', 'dispatch_spaces_decide_approval'],
    ['spaces.revoke_approval_grant', 'dispatch_spaces_revoke_approval_grant'],
    ['space.files.attach', 'dispatch_space_files_attach'],
    ['space.files.list', 'dispatch_space_files_list'],
    ['space.files.read_text', 'dispatch_space_files_read_text'],
    ['space.files.delete', 'dispatch_space_files_delete'],
  ]) {
    const quotedId = actionId.replaceAll('.', '\\.');
    assert.match(
      handlerSource,
      new RegExp(`"${quotedId}"[\\s\\S]{0,320}?=>[\\s\\S]{0,160}?${dispatcher}`),
      `${actionId} must reach ${dispatcher}`,
    );
  }
});

// The legacy web.* alias table and the #[cfg(test)] items are read past, not as dispatch surface. The
// synthetic source covers the boundaries the live handlers.rs relies on: a `;` inside a const type, a
// brace inside a string or char literal, and a raw string holding a quote and a brace.
test('the legacy alias table and cfg(test) items are skipped, and literals keep item boundaries', () => {
  const source = `
const LEGACY_ACTION_ALIASES: [(&str, &str); 1] = [("web.probe_alias", "tickets.create")];

fn execute(action_id: &str) {
    match action_id {
        "tickets.create" => dispatch_tickets_create(),
        _ => {}
    }
}

#[cfg(test)]
const TEST_ONLY: [&str; 2] = ["web.semi_probe", "web.semi_probe_two"];

#[cfg(test)]
mod tests {
    fn open() -> &'static str { "{" }
    const CHAR_OPEN: char = '{';
    const RAW: &str = r#"say "}" now"#;
    const AFTER_RAW: &str = "web.after_raw";
}

fn after_tests() {
    let _ = "plugins.install";
}
`;
  assert.deepEqual([...handlerActionIds(source)].sort(), ['plugins.install', 'tickets.create']);
});

test('the live handlers.rs declares the legacy alias table once and keeps its keys out of the dispatcher set', () => {
  const handlerSource = fs.readFileSync(handlerPath, 'utf8');
  assert.equal([...handlerSource.matchAll(/const LEGACY_ACTION_ALIASES\b/g)].length, 1);
  const dispatcherIds = handlerActionIds(handlerSource);
  for (const id of ['web.crawl_site', 'web.job_status', 'web.pause_job', 'web.cancel_job', 'web.resume_job']) {
    assert.equal(dispatcherIds.has(id), false, `${id} is a legacy alias or test id, not a dispatcher id`);
  }
});
