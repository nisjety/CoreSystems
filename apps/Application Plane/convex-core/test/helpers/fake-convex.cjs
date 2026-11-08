/**
 * A tiny in-memory stand-in for a Convex `ctx` (db + scheduler + runMutation),
 * enough to drive `_handler`s whose reads are equality-index lookups.
 *
 * `withIndex(name, build)` records every `eq(field, value)` the builder makes and
 * filters rows on them, so a lookup returns what the real index would. `order`
 * honours "asc"/"desc" on `_creationTime`. `paginate` pages by numeric cursor.
 * It is not a Convex emulator: no transactions, no schema validation.
 */
const ORG = 'org_ext';
const AUTH = 'auth_user_1';
const SERVICE_KEY = 'service-key-aaaa';

function makeCtx(initialTables = {}, options = {}) {
  const tables = {};
  let counter = 0;
  for (const [name, rows] of Object.entries(initialTables)) {
    tables[name] = rows.map((row) => ({ _id: `${name}_${counter + 1}`, _creationTime: ++counter, ...row }));
  }
  const log = { inserts: [], patches: [], deletes: [], scheduled: [], mutations: [] };

  function rowsOf(table) {
    if (!tables[table]) tables[table] = [];
    return tables[table];
  }

  function makeQuery(table) {
    let rows = rowsOf(table).slice();
    let direction = 'asc';
    const api = {
      withIndex(_name, build) {
        const conditions = [];
        const q = {
          eq(field, value) {
            conditions.push([field, value]);
            return q;
          },
          // Range bounds on the last index field (`q.lt("expiresAt", now)`).
          lt(field, value) {
            conditions.push([field, value, 'lt']);
            return q;
          },
          lte(field, value) {
            conditions.push([field, value, 'lte']);
            return q;
          },
          gt(field, value) {
            conditions.push([field, value, 'gt']);
            return q;
          },
          gte(field, value) {
            conditions.push([field, value, 'gte']);
            return q;
          },
        };
        if (build) build(q);
        rows = rows.filter((row) =>
          conditions.every(([field, value, op]) => {
            if (op === 'lt') return row[field] < value;
            if (op === 'lte') return row[field] <= value;
            if (op === 'gt') return row[field] > value;
            if (op === 'gte') return row[field] >= value;
            return row[field] === value;
          }),
        );
        return api;
      },
      filter() {
        return api;
      },
      order(dir) {
        direction = dir;
        return api;
      },
      _sorted() {
        const copy = rows.slice();
        copy.sort((a, b) => (direction === 'desc' ? b._creationTime - a._creationTime : a._creationTime - b._creationTime));
        return copy;
      },
      async collect() {
        return api._sorted();
      },
      async take(n) {
        return api._sorted().slice(0, n);
      },
      async first() {
        return api._sorted()[0] ?? null;
      },
      async unique() {
        const found = api._sorted();
        if (found.length > 1) throw new Error('unique() found more than one row');
        return found[0] ?? null;
      },
      async paginate({ numItems, cursor }) {
        // The cursor is the _creationTime of the last row returned, so deleting rows
        // between pages does not shift the walk (as with a real Convex cursor).
        const all = api._sorted().filter((row) => (cursor ? row._creationTime > Number(cursor) : true));
        const page = all.slice(0, numItems);
        const isDone = page.length === all.length;
        const last = page.length ? page[page.length - 1]._creationTime : cursor;
        return { page, isDone, continueCursor: isDone ? 'done' : String(last) };
      },
    };
    return api;
  }

  const ctx = {
    db: {
      query: makeQuery,
      async get(id) {
        for (const rows of Object.values(tables)) {
          const hit = rows.find((row) => row._id === id);
          if (hit) return hit;
        }
        return null;
      },
      // Convex returns the id when it belongs to the table, else null.
      normalizeId(table, id) {
        return rowsOf(table).some((row) => row._id === id) ? id : null;
      },
      async insert(table, value) {
        const _id = `${table}_${++counter}`;
        rowsOf(table).push({ _id, _creationTime: counter, ...value });
        log.inserts.push({ table, value });
        return _id;
      },
      async patch(id, value) {
        for (const rows of Object.values(tables)) {
          const hit = rows.find((row) => row._id === id);
          if (hit) {
            for (const [key, next] of Object.entries(value)) {
              if (next === undefined) delete hit[key];
              else hit[key] = next;
            }
            log.patches.push({ id, value });
            return;
          }
        }
        throw new Error(`patch: no row ${id}`);
      },
      async delete(id) {
        for (const rows of Object.values(tables)) {
          const at = rows.findIndex((row) => row._id === id);
          if (at >= 0) {
            rows.splice(at, 1);
            log.deletes.push(id);
            return;
          }
        }
        throw new Error(`delete: no row ${id}`);
      },
    },
    scheduler: {
      async runAfter(delay, fn, args) {
        log.scheduled.push({ delay, fn, args });
      },
    },
    async runMutation(fn, args) {
      log.mutations.push({ fn, args });
      if (options.runMutation) return options.runMutation(fn, args);
      return undefined;
    },
  };
  return { ctx, tables, log };
}

/** The org + member rows `requireGatewayMember` looks for. */
function memberTables(extra = {}, { orgId = ORG, authId = AUTH } = {}) {
  return {
    organizations: [{ _id: 'org_row', externalOrgId: orgId, syncStatus: 'active' }],
    users: [{ _id: 'user_row', externalAuthId: authId, orgId: 'org_row', syncStatus: 'active' }],
    ...extra,
  };
}

/** Set the service key for the duration of a test. */
function withServiceKey(t) {
  const prior = process.env.CONVEX_INTERNAL_SERVICE_KEY;
  process.env.CONVEX_INTERNAL_SERVICE_KEY = SERVICE_KEY;
  t.after(() => {
    if (prior === undefined) delete process.env.CONVEX_INTERNAL_SERVICE_KEY;
    else process.env.CONVEX_INTERNAL_SERVICE_KEY = prior;
  });
}

async function rejectsWith(promise, message) {
  const assert = require('node:assert/strict');
  await assert.rejects(promise, (error) => {
    assert.ok(
      String(error && error.message).includes(message),
      `expected error containing ${JSON.stringify(message)}, got ${JSON.stringify(error && error.message)}`,
    );
    return true;
  });
}

module.exports = { ORG, AUTH, SERVICE_KEY, makeCtx, memberTables, withServiceKey, rejectsWith };
