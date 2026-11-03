const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const {
  SIDEBAR_LIMITS,
  decideSidebarPut,
  sidebarDocsEqual,
  validateSidebarDoc,
} = require('../convex/spaceSidebarRules.ts');
const { sweepOrphans } = require('../convex/spaceSidebar.ts');
const { ORG, makeCtx } = require('./helpers/fake-convex.cjs');

function doc(overrides = {}) {
  return {
    v: 1,
    starred: [],
    hidden: [],
    sections: [],
    groups: [],
    computed: { needsYou: true, unread: true, working: false, muted: false },
    ...overrides,
  };
}

function section(overrides = {}) {
  return { id: 'sec_1', name: 'Work', collapsed: false, sort: 'manual', refs: [], ...overrides };
}

function invalid(input, reason) {
  assert.throws(() => validateSidebarDoc(input), (error) => {
    assert.equal(error.message, `invalid_sidebar_doc:${reason}`);
    return true;
  });
}

const refs = (n, prefix = 'r') => Array.from({ length: n }, (_, i) => `${prefix}${i}`);

test('a minimal and a full document validate and come back unchanged', () => {
  const minimal = doc();
  assert.equal(validateSidebarDoc(minimal), minimal);
  const full = doc({
    starred: ['space_a'],
    hidden: ['space_b'],
    sections: [section({ refs: ['space_c'], sort: 'activity' })],
    groups: [{ id: 'channels', collapsed: true, sort: 'name' }, { id: 'dms' }],
  });
  assert.equal(validateSidebarDoc(full), full);
});

test('the caps are 100 starred, 500 hidden, 30 sections and 200 refs per section', () => {
  assert.equal(SIDEBAR_LIMITS.starred, 100);
  assert.equal(SIDEBAR_LIMITS.hidden, 500);
  assert.equal(SIDEBAR_LIMITS.sections, 30);
  assert.equal(SIDEBAR_LIMITS.refsPerSection, 200);
  validateSidebarDoc(doc({ starred: refs(100) }));
  invalid(doc({ starred: refs(101) }), 'starred_too_many');
  validateSidebarDoc(doc({ hidden: refs(500) }));
  invalid(doc({ hidden: refs(501) }), 'hidden_too_many');
  const sections = (n) => Array.from({ length: n }, (_, i) => section({ id: `s${i}`, name: `Name ${i}` }));
  validateSidebarDoc(doc({ sections: sections(30) }));
  invalid(doc({ sections: sections(31) }), 'sections_too_many');
  validateSidebarDoc(doc({ sections: [section({ refs: refs(200) })] }));
  invalid(doc({ sections: [section({ refs: refs(201) })] }), 'section_refs_too_many');
});

test('refs must match the charset and length and appear once per list', () => {
  invalid(doc({ starred: ['has space'] }), 'starred_bad_ref');
  invalid(doc({ starred: ['slash/ref'] }), 'starred_bad_ref');
  invalid(doc({ starred: [''] }), 'starred_bad_ref');
  invalid(doc({ starred: ['x'.repeat(129)] }), 'starred_bad_ref');
  invalid(doc({ starred: [5] }), 'starred_bad_ref');
  validateSidebarDoc(doc({ starred: ['x'.repeat(128), 'A_b.c:d-e'] }));
  invalid(doc({ hidden: ['a', 'a'] }), 'hidden_duplicate_ref');
  invalid(doc({ starred: 'a' }), 'starred_not_array');
});

test('a ref is in at most one section', () => {
  invalid(
    doc({ sections: [section({ id: 'a', name: 'A', refs: ['r1'] }), section({ id: 'b', name: 'B', refs: ['r1'] })] }),
    'section_ref_in_two_sections',
  );
  invalid(doc({ sections: [section({ refs: ['r1', 'r1'] })] }), 'section_refs_duplicate_ref');
});

test('section ids are unique and match the pattern', () => {
  invalid(
    doc({ sections: [section({ id: 'a', name: 'A' }), section({ id: 'a', name: 'B' })] }),
    'section_duplicate_id',
  );
  invalid(doc({ sections: [section({ id: 'bad id' })] }), 'section_bad_id');
  invalid(doc({ sections: [section({ id: '' })] }), 'section_bad_id');
  invalid(doc({ sections: [section({ id: 'x'.repeat(65) })] }), 'section_bad_id');
});

test('section names are NFC, trimmed, 1..40 code points and unique ignoring case', () => {
  invalid(doc({ sections: [section({ name: ' Work' })] }), 'section_name_not_normalized');
  invalid(doc({ sections: [section({ name: 'Work ' })] }), 'section_name_not_normalized');
  // "e" + combining acute is NFD; the document must already carry the composed form.
  invalid(doc({ sections: [section({ name: 'Café' })] }), 'section_name_not_normalized');
  validateSidebarDoc(doc({ sections: [section({ name: 'Café' })] }));
  invalid(doc({ sections: [section({ name: '' })] }), 'section_name_length');
  validateSidebarDoc(doc({ sections: [section({ name: 'x'.repeat(40) })] }));
  invalid(doc({ sections: [section({ name: 'x'.repeat(41) })] }), 'section_name_length');
  // 40 astral code points are 80 UTF-16 units but still within the cap.
  validateSidebarDoc(doc({ sections: [section({ name: '\u{1F600}'.repeat(40) })] }));
  invalid(doc({ sections: [section({ name: '\u{1F600}'.repeat(41) })] }), 'section_name_length');
  invalid(
    doc({ sections: [section({ id: 'a', name: 'Work' }), section({ id: 'b', name: 'WORK' })] }),
    'section_duplicate_name',
  );
  invalid(doc({ sections: [section({ name: 7 })] }), 'section_bad_name');
});

test('section collapsed and sort values are checked', () => {
  invalid(doc({ sections: [section({ collapsed: 'no' })] }), 'section_bad_collapsed');
  invalid(doc({ sections: [section({ sort: 'random' })] }), 'section_bad_sort');
  for (const sort of ['manual', 'name', 'activity']) {
    validateSidebarDoc(doc({ sections: [section({ sort })] }));
  }
});

test('group ids come from the closed set, appear once, and carry valid state', () => {
  for (const id of ['channels', 'dms', 'personal', 'archived']) {
    validateSidebarDoc(doc({ groups: [{ id }] }));
  }
  invalid(doc({ groups: [{ id: 'favourites' }] }), 'group_unknown_id');
  invalid(doc({ groups: [{ id: 'dms' }, { id: 'dms' }] }), 'group_duplicate_id');
  invalid(doc({ groups: [{ id: 'dms', collapsed: 'yes' }] }), 'group_bad_collapsed');
  invalid(doc({ groups: [{ id: 'dms', sort: 'manual' }] }), 'group_bad_sort');
  invalid(
    doc({ groups: [{ id: 'channels' }, { id: 'dms' }, { id: 'personal' }, { id: 'archived' }, { id: 'channels' }] }),
    'groups_too_many',
  );
});

test('unknown keys are refused at every level', () => {
  invalid({ ...doc(), extra: 1 }, 'doc_unknown_key');
  invalid(doc({ sections: [{ ...section(), extra: 1 }] }), 'section_unknown_key');
  invalid(doc({ groups: [{ id: 'dms', extra: 1 }] }), 'group_unknown_key');
  invalid(doc({ computed: { needsYou: true, unread: true, working: true, muted: true, extra: true } }), 'computed_unknown_key');
});

test('shape errors: not an object, wrong version, bad computed flags', () => {
  invalid(null, 'not_object');
  invalid([], 'not_object');
  invalid(doc({ v: 2 }), 'bad_version');
  invalid(doc({ computed: null }), 'computed_not_object');
  invalid(doc({ computed: { needsYou: 1, unread: true, working: true, muted: true } }), 'computed_bad_flag');
  invalid(doc({ computed: { needsYou: true } }), 'computed_bad_flag');
  invalid(doc({ sections: {} }), 'sections_not_array');
  invalid(doc({ groups: {} }), 'groups_not_array');
});

test('a document over 32 KiB serialised is refused', () => {
  // 30 sections x 200 refs of 128 chars is far past 32 KiB while each cap is respected.
  const big = doc({
    sections: Array.from({ length: 30 }, (_, s) =>
      section({
        id: `s${s}`,
        name: `Section ${s}`,
        refs: Array.from({ length: 200 }, (_, r) => `${String(s).padStart(2, '0')}${String(r).padStart(3, '0')}${'x'.repeat(123)}`),
      }),
    ),
  });
  invalid(big, 'too_large');
  assert.equal(SIDEBAR_LIMITS.maxBytes, 32768);
});

test('sidebarDocsEqual ignores key order and undefined', () => {
  assert.equal(sidebarDocsEqual({ a: 1, b: [1, { c: 2 }] }, { b: [1, { c: 2 }], a: 1 }), true);
  assert.equal(sidebarDocsEqual({ a: 1, b: undefined }, { a: 1 }), true);
  assert.equal(sidebarDocsEqual({ a: 1 }, { a: 2 }), false);
  assert.equal(sidebarDocsEqual([1, 2], [2, 1]), false);
});

test('decideSidebarPut compare-and-set', () => {
  const stored = { revision: 3, doc: doc({ starred: ['a'] }) };
  const next = doc({ starred: ['a', 'b'] });
  // base 0 with no row creates revision 1
  assert.deepEqual(decideSidebarPut(null, 0, next), { action: 'create', revision: 1 });
  // a never-seen base against an existing row conflicts
  assert.deepEqual(decideSidebarPut(stored, 0, next), { action: 'conflict', revision: 3 });
  // a stale base conflicts
  assert.deepEqual(decideSidebarPut(stored, 2, next), { action: 'conflict', revision: 3 });
  // a base ahead of the store conflicts too
  assert.deepEqual(decideSidebarPut(stored, 9, next), { action: 'conflict', revision: 3 });
  // a base with no row but non-zero conflicts at revision 0
  assert.deepEqual(decideSidebarPut(null, 4, next), { action: 'conflict', revision: 0 });
  // a current base updates and bumps the revision
  assert.deepEqual(decideSidebarPut(stored, 3, next), { action: 'update', revision: 4 });
  // an equal document is success at the stored revision, even from a stale base
  assert.deepEqual(decideSidebarPut(stored, 3, doc({ starred: ['a'] })), { action: 'noop', revision: 3 });
  assert.deepEqual(decideSidebarPut(stored, 1, doc({ starred: ['a'] })), { action: 'noop', revision: 3 });
});

test('SIDEBAR_LIMITS equals the golden fixture and the fixture matches its sha256 pin', () => {
  const dir = path.join(__dirname, 'fixtures');
  const bytes = fs.readFileSync(path.join(dir, 'space-sidebar-limits.json'));
  assert.deepEqual(JSON.parse(bytes.toString('utf8')), { ...SIDEBAR_LIMITS });
  const pin = fs.readFileSync(path.join(dir, 'space-sidebar-limits.json.sha256'), 'utf8').trim();
  const [hash, name] = pin.split(/\s+/);
  assert.equal(name, 'space-sidebar-limits.json');
  // The pin is over the file's bytes with line endings normalised, so a CRLF checkout still matches.
  const normalised = Buffer.from(bytes.toString('utf8').replace(/\r\n/g, '\n'), 'utf8');
  assert.equal(crypto.createHash('sha256').update(normalised).digest('hex'), hash);
});

// ---------------------------------------------------------------------------
// WS16-S11: sweepOrphans, driven through the in-memory ctx.
// ---------------------------------------------------------------------------

function sidebarRow(externalOrgId, externalAuthId) {
  return { externalOrgId, externalAuthId, revision: 1, doc: doc() };
}

test('sweepOrphans deletes rows whose org, user or membership is gone and keeps a live member', async () => {
  const { ctx, tables, log } = makeCtx({
    organizations: [
      { _id: 'org_live', externalOrgId: ORG, syncStatus: 'active' },
      { _id: 'org_dead', externalOrgId: 'org_deleted', syncStatus: 'deleted' },
    ],
    users: [
      { _id: 'u_live', externalAuthId: 'alice', orgId: 'org_live', syncStatus: 'active' },
      { _id: 'u_gone', externalAuthId: 'bob', orgId: 'org_live', syncStatus: 'deleted' },
      { _id: 'u_in_dead_org', externalAuthId: 'carol', orgId: 'org_dead', syncStatus: 'active' },
    ],
    spaceSidebarPreferences: [
      sidebarRow(ORG, 'alice'), // live member: kept
      sidebarRow(ORG, 'bob'), // users row syncStatus deleted: deleted
      sidebarRow(ORG, 'dave'), // no users row: deleted
      sidebarRow('org_missing', 'alice'), // org missing: deleted
      sidebarRow('org_deleted', 'carol'), // org deleted: deleted
    ],
  });

  const result = await sweepOrphans._handler(ctx, {});
  assert.deepEqual(result, { scanned: 5, deleted: 4, done: true });
  assert.equal(tables.spaceSidebarPreferences.length, 1);
  assert.equal(tables.spaceSidebarPreferences[0].externalAuthId, 'alice');
  assert.equal(tables.spaceSidebarPreferences[0].externalOrgId, ORG);
  assert.equal(log.scheduled.length, 0, 'a finished sweep schedules nothing');

  const second = await sweepOrphans._handler(ctx, {});
  assert.deepEqual(second, { scanned: 1, deleted: 0, done: true });
  assert.equal(tables.spaceSidebarPreferences.length, 1);
});

test('sweepOrphans pages 100 rows at a time and schedules the next page with the cursor', async () => {
  const rows = [];
  for (let i = 0; i < 250; i += 1) rows.push(sidebarRow(ORG, `member_${i}`));
  const users = rows.map((row, i) => ({
    _id: `u_${i}`,
    externalAuthId: row.externalAuthId,
    orgId: 'org_live',
    // every 10th member has left
    syncStatus: i % 10 === 0 ? 'deleted' : 'active',
  }));
  const { ctx, tables, log } = makeCtx({
    organizations: [{ _id: 'org_live', externalOrgId: ORG, syncStatus: 'active' }],
    users,
    spaceSidebarPreferences: rows,
  });

  const first = await sweepOrphans._handler(ctx, {});
  assert.equal(first.scanned, 100);
  assert.equal(first.done, false);
  assert.equal(first.deleted, 10);
  assert.equal(log.scheduled.length, 1);
  assert.equal(log.scheduled[0].delay, 0);
  const cursor1 = log.scheduled[0].args.cursor;
  assert.ok(cursor1, 'the next page carries a cursor');

  const page2 = await sweepOrphans._handler(ctx, { cursor: cursor1 });
  assert.equal(page2.scanned, 100);
  assert.equal(page2.done, false);
  assert.equal(log.scheduled.length, 2);
  const page3 = await sweepOrphans._handler(ctx, { cursor: log.scheduled[1].args.cursor });
  assert.equal(page3.scanned, 50);
  assert.equal(page3.done, true);
  assert.equal(log.scheduled.length, 2, 'the last page schedules nothing');
  // members 0,10,...,240 left: 25 rows deleted, 225 kept
  assert.equal(first.deleted + page2.deleted + page3.deleted, 25);
  assert.equal(tables.spaceSidebarPreferences.length, 225);

  const again = await sweepOrphans._handler(ctx, {});
  assert.equal(again.deleted, 0, 'a second run deletes nothing');
});

test('crons.ts registers the sidebar orphan sweep every 6 hours against spaceSidebar.sweepOrphans', () => {
  const source = fs.readFileSync(path.join(__dirname, '..', 'convex', 'crons.ts'), 'utf8');
  const match = source.match(/crons\.interval\(\s*"space sidebar orphan sweep",\s*\{\s*hours:\s*6\s*\},\s*internal\.spaceSidebar\.sweepOrphans/);
  assert.ok(match, 'crons.ts must register the sweep');
});
