const assert = require('node:assert/strict');
const test = require('node:test');

const notices = require('../convex/spaceRoomNotices.ts');
const agents = require('../convex/spaceAgents.ts');
const spaces = require('../convex/spaces.ts');
const { AUTH, ORG, SERVICE_KEY, makeCtx, memberTables, rejectsWith, withServiceKey } = require('./helpers/fake-convex.cjs');

const {
  MAX_NOTICES_PER_READ,
  NOTICE_RETENTION_MS,
  ROOM_NOTICE_KINDS,
  noticeKindForBindingStatus,
  pruneRoomNotices,
  recordRoomNotice,
  roomNoticesForGateway,
} = notices;

const DAY = 24 * 60 * 60 * 1000;
const base = { serviceKey: SERVICE_KEY, externalAuthId: AUTH, externalOrgId: ORG };

function noticeRows(tables) {
  return tables.spaceRoomNotices ?? [];
}

test('the notice vocabulary is the closed set the timeline renders', () => {
  assert.deepEqual([...ROOM_NOTICE_KINDS], ['agent_bound', 'agent_paused', 'agent_resumed', 'agent_revoked', 'room_renamed', 'topic_changed']);
  assert.equal(MAX_NOTICES_PER_READ, 100);
  assert.equal(NOTICE_RETENTION_MS, 90 * DAY);
});

test('noticeKindForBindingStatus maps paused, active and revoked; any other status has none', () => {
  assert.equal(noticeKindForBindingStatus('paused'), 'agent_paused');
  assert.equal(noticeKindForBindingStatus('active'), 'agent_resumed');
  assert.equal(noticeKindForBindingStatus('revoked'), 'agent_revoked');
  for (const status of ['pending', 'failed', '', 'PAUSED']) assert.equal(noticeKindForBindingStatus(status), null, status);
});

test('recordRoomNotice stores ids, a kind and a time only: no name, no text', async () => {
  const { ctx, tables } = makeCtx({});
  const before = Date.now();
  await recordRoomNotice(ctx, { spaceRef: 'space_1', externalOrgId: ORG, kind: 'room_renamed', actorExternalAuthId: 'alice', subjectId: 'agent-1' });
  const [row] = noticeRows(tables);
  assert.deepEqual(Object.keys(row).sort(), ['_creationTime', '_id', 'actorExternalAuthId', 'createdAt', 'externalOrgId', 'kind', 'noticeId', 'spaceRef', 'subjectId']);
  assert.match(row.noticeId, /^rn_[0-9a-f-]{36}$/);
  assert.ok(row.createdAt >= before && row.createdAt <= Date.now());
  await recordRoomNotice(ctx, { spaceRef: 'space_1', externalOrgId: ORG, kind: 'topic_changed' });
  assert.notEqual(noticeRows(tables)[0].noticeId, noticeRows(tables)[1].noticeId);
});

// --- the four writers -----------------------------------------------------------------------

test('createSpaceAgentForGateway writes an agent_bound notice with the new subject id, in the same pass', async (t) => {
  withServiceKey(t);
  const { ctx, tables } = makeCtx(memberTables({ spaces: [{ spaceRef: 'space_1', externalOrgId: ORG, kind: 'room', lifecycle: 'active' }] }));
  const result = await agents.createSpaceAgentForGateway._handler(ctx, { ...base, spaceRef: 'space_1', name: 'Helper' });
  const [notice] = noticeRows(tables);
  assert.equal(noticeRows(tables).length, 1);
  assert.equal(notice.kind, 'agent_bound');
  assert.equal(notice.spaceRef, 'space_1');
  assert.equal(notice.externalOrgId, ORG);
  assert.equal(notice.actorExternalAuthId, AUTH);
  assert.equal(notice.subjectId, result.subjectId);
  assert.equal(JSON.stringify(notice).includes('Helper'), false, 'the agent name is not copied into the notice');
});

test('createSpaceAgentForGateway writes no notice when it refuses (bad name, or a direct message)', async (t) => {
  withServiceKey(t);
  const rooms = memberTables({ spaces: [{ spaceRef: 'dm_1', externalOrgId: ORG, kind: 'dm', lifecycle: 'active' }] });
  const { ctx, tables } = makeCtx(rooms);
  await rejectsWith(agents.createSpaceAgentForGateway._handler(ctx, { ...base, spaceRef: 'dm_1', name: 'Helper' }), 'dm_agent_bind_refused');
  await rejectsWith(agents.createSpaceAgentForGateway._handler(ctx, { ...base, spaceRef: 'space_1', name: 'x' }), 'between 2 and 60');
  assert.equal(noticeRows(tables).length, 0);
});

test('bindExistingSpaceAgentForGateway writes an agent_bound notice for the bound subject', async (t) => {
  withServiceKey(t);
  const world = memberTables({ agents: [{ _id: 'agent_1', orgId: 'org_row', name: 'Existing' }] });
  const { ctx, tables } = makeCtx(world, { runMutation: async () => ({ bindingRef: 'sab_1', created: true }) });
  const result = await agents.bindExistingSpaceAgentForGateway._handler(ctx, { ...base, spaceRef: 'space_1', agentId: 'agent_1' });
  assert.deepEqual(result, { subjectId: 'agent-agent_1', bindingRef: 'sab_1' });
  const [notice] = noticeRows(tables);
  assert.equal(noticeRows(tables).length, 1);
  assert.equal(notice.kind, 'agent_bound');
  assert.equal(notice.subjectId, 'agent-agent_1');
  assert.equal(notice.actorExternalAuthId, AUTH);
});

test('bindExistingSpaceAgentForGateway writes no notice for another organization\'s agent', async (t) => {
  withServiceKey(t);
  const world = memberTables({ agents: [{ _id: 'agent_x', orgId: 'org_somewhere_else' }] });
  const { ctx, tables } = makeCtx(world, { runMutation: async () => ({ bindingRef: 'sab_1', created: true }) });
  await rejectsWith(agents.bindExistingSpaceAgentForGateway._handler(ctx, { ...base, spaceRef: 'space_1', agentId: 'agent_x' }), 'Agent definition not found');
  assert.equal(noticeRows(tables).length, 0);
});

function bindingWorld(status) {
  return memberTables({
    spaceAgentBindings: [{ bindingRef: 'sab_1', spaceRef: 'space_1', externalOrgId: ORG, agentId: 'agent_1', subjectId: 'agent-agent_1', status, projectionVersion: 3, updatedAt: 1 }],
  });
}

test('setSpaceAgentBindingStateForGateway writes paused, resumed and revoked notices', async (t) => {
  withServiceKey(t);
  const cases = [
    ['active', 'paused', 'agent_paused'],
    ['paused', 'active', 'agent_resumed'],
    ['active', 'revoked', 'agent_revoked'],
    ['paused', 'revoked', 'agent_revoked'],
  ];
  for (const [from, to, kind] of cases) {
    const { ctx, tables } = makeCtx(bindingWorld(from));
    const result = await agents.setSpaceAgentBindingStateForGateway._handler(ctx, { ...base, spaceRef: 'space_1', bindingRef: 'sab_1', status: to });
    assert.equal(result.changed, true, `${from}->${to}`);
    assert.equal(noticeRows(tables).length, 1, `${from}->${to}`);
    assert.equal(noticeRows(tables)[0].kind, kind);
    assert.equal(noticeRows(tables)[0].subjectId, 'agent-agent_1');
    assert.equal(noticeRows(tables)[0].actorExternalAuthId, AUTH);
    assert.equal(tables.spaceAgentBindings[0].status, to);
  }
});

test('setSpaceAgentBindingStateForGateway writes no notice when nothing changed or the change is refused', async (t) => {
  withServiceKey(t);
  const same = makeCtx(bindingWorld('paused'));
  const result = await agents.setSpaceAgentBindingStateForGateway._handler(same.ctx, { ...base, spaceRef: 'space_1', bindingRef: 'sab_1', status: 'paused' });
  assert.equal(result.changed, false);
  assert.equal(noticeRows(same.tables).length, 0);

  const revoked = makeCtx(bindingWorld('revoked'));
  await rejectsWith(agents.setSpaceAgentBindingStateForGateway._handler(revoked.ctx, { ...base, spaceRef: 'space_1', bindingRef: 'sab_1', status: 'active' }), 'revoked binding cannot be changed');
  const pending = makeCtx(bindingWorld('pending'));
  await rejectsWith(agents.setSpaceAgentBindingStateForGateway._handler(pending.ctx, { ...base, spaceRef: 'space_1', bindingRef: 'sab_1', status: 'paused' }), 'pending binding cannot be paused');
  const elsewhere = makeCtx(bindingWorld('active'));
  await rejectsWith(agents.setSpaceAgentBindingStateForGateway._handler(elsewhere.ctx, { ...base, spaceRef: 'space_2', bindingRef: 'sab_1', status: 'paused' }), 'Agent binding not found in this Space');
  for (const w of [revoked, pending, elsewhere]) assert.equal(noticeRows(w.tables).length, 0);
});

/**
 * Convex hands a handler a snapshot: a row it read does not change when it patches it. The fake
 * patches in place, which would make `space.name` already new at the comparison, so a room test
 * swaps `patch` for one that replaces the stored row with a copy.
 */
function snapshotting(made) {
  const { ctx, tables } = made;
  ctx.db.patch = async (id, value) => {
    for (const rows of Object.values(tables)) {
      const at = rows.findIndex((row) => row._id === id);
      if (at >= 0) {
        const next = { ...rows[at] };
        for (const [key, v] of Object.entries(value)) {
          if (v === undefined) delete next[key];
          else next[key] = v;
        }
        rows[at] = next;
        return;
      }
    }
    throw new Error(`patch: no row ${id}`);
  };
  return made;
}

function roomWorld(extra = {}) {
  return memberTables({
    spaces: [{ spaceRef: 'space_1', externalOrgId: ORG, kind: 'room', lifecycle: 'active', name: 'Old name', topic: 'Old topic', ...extra }],
  });
}

test('updateRoomDetailsForGateway writes room_renamed only when the name changed', async (t) => {
  withServiceKey(t);
  const changed = snapshotting(makeCtx(roomWorld()));
  await spaces.updateRoomDetailsForGateway._handler(changed.ctx, { ...base, spaceRef: 'space_1', name: 'New name' });
  assert.deepEqual(noticeRows(changed.tables).map((row) => row.kind), ['room_renamed']);
  assert.equal(noticeRows(changed.tables)[0].actorExternalAuthId, AUTH);
  assert.equal(JSON.stringify(noticeRows(changed.tables)[0]).includes('New name'), false, 'the name is not copied');

  const same = snapshotting(makeCtx(roomWorld()));
  await spaces.updateRoomDetailsForGateway._handler(same.ctx, { ...base, spaceRef: 'space_1', name: '  Old   name ' });
  assert.equal(noticeRows(same.tables).length, 0, 'a normalised-equal name is no change');
});

test('updateRoomDetailsForGateway writes topic_changed only when the topic changed, including a clear', async (t) => {
  withServiceKey(t);
  const changed = snapshotting(makeCtx(roomWorld()));
  await spaces.updateRoomDetailsForGateway._handler(changed.ctx, { ...base, spaceRef: 'space_1', topic: 'Fresh topic' });
  assert.deepEqual(noticeRows(changed.tables).map((row) => row.kind), ['topic_changed']);
  assert.equal(JSON.stringify(noticeRows(changed.tables)[0]).includes('Fresh topic'), false);

  const same = snapshotting(makeCtx(roomWorld()));
  await spaces.updateRoomDetailsForGateway._handler(same.ctx, { ...base, spaceRef: 'space_1', topic: 'Old topic' });
  assert.equal(noticeRows(same.tables).length, 0);

  const cleared = snapshotting(makeCtx(roomWorld()));
  await spaces.updateRoomDetailsForGateway._handler(cleared.ctx, { ...base, spaceRef: 'space_1', topic: '' });
  assert.deepEqual(noticeRows(cleared.tables).map((row) => row.kind), ['topic_changed']);

  const alreadyEmpty = snapshotting(makeCtx(roomWorld({ topic: undefined })));
  await spaces.updateRoomDetailsForGateway._handler(alreadyEmpty.ctx, { ...base, spaceRef: 'space_1', topic: '' });
  assert.equal(noticeRows(alreadyEmpty.tables).length, 0);
});

test('updateRoomDetailsForGateway writes one notice per changed field, and none for a field not sent', async (t) => {
  withServiceKey(t);
  const both = snapshotting(makeCtx(roomWorld()));
  await spaces.updateRoomDetailsForGateway._handler(both.ctx, { ...base, spaceRef: 'space_1', name: 'Another', topic: 'Another topic' });
  assert.deepEqual(noticeRows(both.tables).map((row) => row.kind).sort(), ['room_renamed', 'topic_changed']);

  const nameOnly = snapshotting(makeCtx(roomWorld()));
  await spaces.updateRoomDetailsForGateway._handler(nameOnly.ctx, { ...base, spaceRef: 'space_1', name: 'Another' });
  assert.deepEqual(noticeRows(nameOnly.tables).map((row) => row.kind), ['room_renamed']);
});

test('updateRoomDetailsForGateway writes no notice when it is refused', async (t) => {
  withServiceKey(t);
  const empty = snapshotting(makeCtx(roomWorld()));
  await rejectsWith(spaces.updateRoomDetailsForGateway._handler(empty.ctx, { ...base, spaceRef: 'space_1' }), 'empty_update');
  const dm = snapshotting(makeCtx(roomWorld({ kind: 'dm' })));
  await rejectsWith(spaces.updateRoomDetailsForGateway._handler(dm.ctx, { ...base, spaceRef: 'space_1', name: 'x' }), 'space_kind_not_editable');
  const archived = snapshotting(makeCtx(roomWorld({ lifecycle: 'archived' })));
  await rejectsWith(spaces.updateRoomDetailsForGateway._handler(archived.ctx, { ...base, spaceRef: 'space_1', name: 'x' }), 'room_not_active');
  const foreign = snapshotting(makeCtx(roomWorld({ externalOrgId: 'org_other' })));
  await rejectsWith(spaces.updateRoomDetailsForGateway._handler(foreign.ctx, { ...base, spaceRef: 'space_1', name: 'x' }), 'space_not_found');
  for (const w of [empty, dm, archived, foreign]) assert.equal(noticeRows(w.tables).length, 0);
});

// --- roomNoticesForGateway ------------------------------------------------------------------

function notice(createdAt, extra = {}) {
  return { noticeId: `rn_${createdAt}`, spaceRef: 'space_1', externalOrgId: ORG, kind: 'agent_bound', actorExternalAuthId: 'alice', subjectId: 'agent-1', createdAt, ...extra };
}

function read(ctx, extra = {}) {
  return roomNoticesForGateway._handler(ctx, { ...base, spaceRef: 'space_1', ...extra });
}

test('roomNoticesForGateway returns the interval [after, before) newest first', async (t) => {
  withServiceKey(t);
  const rows = [100, 200, 300, 400, 500].map((at) => notice(at));
  const { ctx } = makeCtx(memberTables({ spaceRoomNotices: rows }));
  const all = await read(ctx);
  assert.deepEqual(all.notices.map((n) => n.createdAt), [500, 400, 300, 200, 100]);
  assert.equal(all.truncated, false);
  const window = await read(ctx, { after: 200, before: 400 });
  assert.deepEqual(window.notices.map((n) => n.createdAt), [300, 200], 'after is inclusive, before is exclusive');
  assert.deepEqual((await read(ctx, { after: 400 })).notices.map((n) => n.createdAt), [500, 400]);
  assert.deepEqual((await read(ctx, { before: 300 })).notices.map((n) => n.createdAt), [200, 100]);
  assert.deepEqual((await read(ctx, { after: 600 })).notices, []);
});

test('roomNoticesForGateway maps rows to ids, kind and time, with nulls for absent actor and subject', async (t) => {
  withServiceKey(t);
  const { ctx } = makeCtx(memberTables({ spaceRoomNotices: [notice(100, { actorExternalAuthId: undefined, subjectId: undefined, kind: 'topic_changed' })] }));
  const result = await read(ctx);
  assert.deepEqual(result.notices, [{ noticeId: 'rn_100', kind: 'topic_changed', actorExternalAuthId: null, subjectId: null, createdAt: 100 }]);
});

test('roomNoticesForGateway truncates at limit and reports it', async (t) => {
  withServiceKey(t);
  const rows = [100, 200, 300].map((at) => notice(at));
  const { ctx } = makeCtx(memberTables({ spaceRoomNotices: rows }));
  const two = await read(ctx, { limit: 2 });
  assert.deepEqual(two.notices.map((n) => n.createdAt), [300, 200]);
  assert.equal(two.truncated, true);
  const three = await read(ctx, { limit: 3 });
  assert.equal(three.truncated, false);
  assert.equal((await read(ctx, { limit: 0 })).notices.length, 1, 'a limit below one is raised to one');
  assert.equal((await read(ctx, { limit: -5 })).notices.length, 1);
});

test('roomNoticesForGateway never returns more than 100', async (t) => {
  withServiceKey(t);
  const rows = Array.from({ length: 105 }, (_, i) => notice(i + 1));
  const { ctx } = makeCtx(memberTables({ spaceRoomNotices: rows }));
  const result = await read(ctx, { limit: 1000 });
  assert.equal(result.notices.length, 100);
  assert.equal(result.truncated, true);
  assert.equal(result.notices[0].createdAt, 105);
});

test('roomNoticesForGateway scopes by Space and by organization', async (t) => {
  withServiceKey(t);
  const rows = [
    notice(100),
    notice(200, { spaceRef: 'space_2' }),
    notice(300, { externalOrgId: 'org_other' }),
    notice(400),
  ];
  const { ctx } = makeCtx(memberTables({ spaceRoomNotices: rows }));
  const result = await read(ctx);
  assert.deepEqual(result.notices.map((n) => n.createdAt), [400, 100]);
});

test('roomNoticesForGateway needs the service key, an organization member and a clean Space ref', async (t) => {
  withServiceKey(t);
  const { ctx } = makeCtx(memberTables({ spaceRoomNotices: [notice(100)] }));
  await assert.rejects(roomNoticesForGateway._handler(ctx, { ...base, serviceKey: 'wrong', spaceRef: 'space_1' }));
  await rejectsWith(roomNoticesForGateway._handler(ctx, { ...base, externalAuthId: 'stranger', spaceRef: 'space_1' }), 'Unauthorized');
  await rejectsWith(roomNoticesForGateway._handler(ctx, { ...base, spaceRef: '  ' }), 'room_notices_invalid:space_ref');
  await rejectsWith(roomNoticesForGateway._handler(ctx, { ...base, spaceRef: 'a\u0000b' }), 'room_notices_invalid:space_ref');
  await rejectsWith(roomNoticesForGateway._handler(ctx, { ...base, spaceRef: 'x'.repeat(201) }), 'room_notices_invalid:space_ref');
});

// --- pruneRoomNotices -----------------------------------------------------------------------

test('pruneRoomNotices deletes notices older than 90 days and keeps newer ones', async () => {
  const now = Date.now();
  const { ctx, tables, log } = makeCtx({
    spaceRoomNotices: [
      notice(now - 91 * DAY),
      notice(now - 90 * DAY - 5000),
      notice(now - 89 * DAY),
      notice(now - 1000),
    ],
  });
  const result = await pruneRoomNotices._handler(ctx, {});
  assert.deepEqual(result, { deleted: 2 });
  assert.deepEqual(tables.spaceRoomNotices.map((row) => row.noticeId), [`rn_${now - 89 * DAY}`, `rn_${now - 1000}`]);
  assert.deepEqual(log.scheduled, []);
});

test('pruneRoomNotices reschedules itself when a batch of 200 was deleted', async () => {
  const now = Date.now();
  const rows = Array.from({ length: 200 }, (_, i) => notice(now - 100 * DAY + i));
  const { ctx, tables, log } = makeCtx({ spaceRoomNotices: rows });
  assert.deepEqual(await pruneRoomNotices._handler(ctx, {}), { deleted: 200 });
  assert.equal(tables.spaceRoomNotices.length, 0);
  assert.equal(log.scheduled.length, 1);
  assert.deepEqual(log.scheduled[0].args, {});
});
