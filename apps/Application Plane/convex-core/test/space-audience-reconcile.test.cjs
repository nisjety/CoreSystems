const assert = require('node:assert/strict');
const test = require('node:test');
const { getFunctionName } = require('convex/server');

const {
  MAX_AUDIENCE_RECIPIENTS,
  normalizeAudienceCandidates,
  planAudienceReconcile,
} = require('../convex/spaceAudiencePlan.ts');
const {
  audienceReconcileEnabled,
  reconcileAllRoomAudiences,
  reconcileRoomAudience,
} = require('../convex/spaceAudienceReconcile.ts');

// ---------------------------------------------------------------------------
// Pure plan
// ---------------------------------------------------------------------------

const active = (overrides = {}) => ({
  revision: 4,
  controlState: 'acknowledged',
  recipientExternalAuthIds: ['u1', 'u2'],
  ...overrides,
});

test('the audience limit is 512', () => {
  assert.equal(MAX_AUDIENCE_RECIPIENTS, 512);
});

test('normalizeAudienceCandidates trims, dedupes, drops non-strings and sorts by code point', () => {
  assert.deepEqual(
    normalizeAudienceCandidates([' a ', 'a', 'B', '_a', 'a-b', '', '   ', null, undefined, 7, {}]),
    ['B', '_a', 'a', 'a-b'],
  );
  assert.deepEqual(normalizeAudienceCandidates([]), []);
  // Astral code points sort after BMP ones, matching UTF-8 byte order.
  assert.deepEqual(normalizeAudienceCandidates(['\u{1F600}', 'Ａ', 'z']), ['z', 'Ａ', '\u{1F600}']);
});

test('equal sets that Control has not rejected are a noop, whatever the order or spacing', () => {
  assert.deepEqual(planAudienceReconcile({ candidates: ['u2', ' u1 '], active: active() }), { action: 'noop' });
  assert.deepEqual(
    planAudienceReconcile({ candidates: ['u1', 'u2'], active: active({ controlState: 'pending' }) }),
    { action: 'noop' },
  );
});

test('a rejected active audience is replaced even when the recipients are equal', () => {
  assert.deepEqual(
    planAudienceReconcile({ candidates: ['u1', 'u2'], active: active({ controlState: 'rejected', revision: 9 }) }),
    { action: 'replace', recipients: ['u1', 'u2'], expectedRevision: 9 },
  );
});

test('no active audience replaces with an undefined expectedRevision', () => {
  const plan = planAudienceReconcile({ candidates: ['u2', 'u1'], active: null });
  assert.deepEqual(plan, { action: 'replace', recipients: ['u1', 'u2'], expectedRevision: undefined });
  assert.equal(plan.expectedRevision, undefined);
});

test('a differing set replaces with expectedRevision equal to the active revision', () => {
  assert.deepEqual(
    planAudienceReconcile({ candidates: ['u1', 'u3'], active: active({ revision: 7 }) }),
    { action: 'replace', recipients: ['u1', 'u3'], expectedRevision: 7 },
  );
  assert.deepEqual(
    planAudienceReconcile({ candidates: ['u1', 'u3'], active: active({ revision: 7, controlState: 'rejected' }) }),
    { action: 'replace', recipients: ['u1', 'u3'], expectedRevision: 7 },
  );
});

test('a differing set waits while the active audience is still pending at Control', () => {
  assert.deepEqual(
    planAudienceReconcile({ candidates: ['u1', 'u3'], active: active({ controlState: 'pending' }) }),
    { action: 'wait' },
  );
});

test('no usable candidates is refused, never an empty audience', () => {
  for (const candidates of [[], ['', '  '], [null, undefined, 3]]) {
    assert.deepEqual(planAudienceReconcile({ candidates, active: active() }), {
      action: 'refuse',
      reason: 'empty_candidates',
    });
    assert.deepEqual(planAudienceReconcile({ candidates, active: null }), {
      action: 'refuse',
      reason: 'empty_candidates',
    });
  }
});

test('512 recipients are accepted and 513 are refused as audience_too_large', () => {
  const make = (count) => Array.from({ length: count }, (_, index) => `user_${String(index).padStart(4, '0')}`);
  const ok = planAudienceReconcile({ candidates: make(512), active: null });
  assert.equal(ok.action, 'replace');
  assert.equal(ok.recipients.length, 512);
  assert.deepEqual(planAudienceReconcile({ candidates: make(513), active: null }), {
    action: 'refuse',
    reason: 'audience_too_large',
  });
  // Duplicates collapse before the size check.
  assert.equal(planAudienceReconcile({ candidates: [...make(512), ...make(512)], active: null }).action, 'replace');
});

// ---------------------------------------------------------------------------
// reconcileRoomAudience, driven through a mocked ctx and fetch
// ---------------------------------------------------------------------------

const ENV_KEYS = ['SPACE_AUDIENCE_RECONCILE', 'CONTROL_SPACE_MEMBERSHIP_BASE_URL', 'APPLICATION_SPACE_LIFECYCLE_TOKEN'];
const ORG = 'org_a';

function jsonResponse(body, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
}

function candidatesBody(overrides = {}) {
  return {
    data: {
      org_id: ORG,
      state: 'active',
      members: [{ subject_id: 'u2' }, { subject_id: 'u1' }],
      ...overrides,
    },
  };
}

/**
 * @param {object} opts
 * @param {object[]} opts.reads  successive results of activeAudienceForReconcile (last one repeats)
 * @param {(args:any, call:number)=>any} [opts.replace]  replaceRecipientAudience behaviour
 */
function makeCtx({ reads, replace }) {
  const calls = { reads: 0, replaces: [] };
  const ctx = {
    async runQuery(ref, args) {
      assert.equal(getFunctionName(ref), 'spaces:activeAudienceForReconcile');
      const result = reads[Math.min(calls.reads, reads.length - 1)];
      calls.reads += 1;
      return result;
    },
    async runMutation(ref, args) {
      assert.equal(getFunctionName(ref), 'spaces:replaceRecipientAudience');
      calls.replaces.push(args);
      return replace ? replace(args, calls.replaces.length) : { revision: 1 };
    },
  };
  return { ctx, calls };
}

const sharedRoom = (audience = null, overrides = {}) => ({
  space: { kind: 'room', lifecycle: 'active', externalOrgId: ORG, ...overrides },
  audience,
});

async function withEnv(env, fetchImpl, fn) {
  const priorEnv = Object.fromEntries(ENV_KEYS.map((key) => [key, process.env[key]]));
  const priorFetch = global.fetch;
  for (const key of ENV_KEYS) delete process.env[key];
  Object.assign(process.env, env);
  const fetches = [];
  global.fetch = async (url, init) => {
    fetches.push({ url: String(url), init });
    if (!fetchImpl) throw new Error('unexpected fetch');
    return fetchImpl(String(url), init, fetches.length);
  };
  try {
    return await fn(fetches);
  } finally {
    global.fetch = priorFetch;
    for (const key of ENV_KEYS) {
      if (priorEnv[key] === undefined) delete process.env[key];
      else process.env[key] = priorEnv[key];
    }
  }
}

const ON = {
  SPACE_AUDIENCE_RECONCILE: '1',
  CONTROL_SPACE_MEMBERSHIP_BASE_URL: 'http://control.test/spaces/',
  APPLICATION_SPACE_LIFECYCLE_TOKEN: 'tttttttt',
};

const run = (ctx, spaceRef = 'space_1') => reconcileRoomAudience._handler(ctx, { spaceRef });

test('the audience reconcile flag is off unless 1 or true', () => {
  const prior = process.env.SPACE_AUDIENCE_RECONCILE;
  try {
    delete process.env.SPACE_AUDIENCE_RECONCILE;
    assert.equal(audienceReconcileEnabled(), false);
    process.env.SPACE_AUDIENCE_RECONCILE = '0';
    assert.equal(audienceReconcileEnabled(), false);
    process.env.SPACE_AUDIENCE_RECONCILE = 'true';
    assert.equal(audienceReconcileEnabled(), true);
    process.env.SPACE_AUDIENCE_RECONCILE = '1';
    assert.equal(audienceReconcileEnabled(), true);
  } finally {
    if (prior === undefined) delete process.env.SPACE_AUDIENCE_RECONCILE;
    else process.env.SPACE_AUDIENCE_RECONCILE = prior;
  }
});

test('reconcileRoomAudience: disabled flag does nothing at all', async () => {
  await withEnv({ ...ON, SPACE_AUDIENCE_RECONCILE: '0' }, null, async (fetches) => {
    const { ctx, calls } = makeCtx({ reads: [sharedRoom()] });
    assert.deepEqual(await run(ctx), { status: 'disabled' });
    assert.equal(fetches.length, 0);
    assert.equal(calls.reads, 0);
  });
});

test('reconcileRoomAudience: missing Control URL or token is not_configured', async () => {
  for (const env of [
    { SPACE_AUDIENCE_RECONCILE: '1', APPLICATION_SPACE_LIFECYCLE_TOKEN: 'tttttttt' },
    { SPACE_AUDIENCE_RECONCILE: '1', CONTROL_SPACE_MEMBERSHIP_BASE_URL: 'http://control.test' },
  ]) {
    await withEnv(env, null, async (fetches) => {
      const { ctx, calls } = makeCtx({ reads: [sharedRoom()] });
      const result = await run(ctx);
      assert.equal(result.status, 'not_configured');
      assert.equal(fetches.length, 0);
      assert.equal(calls.replaces.length, 0);
    });
  }
});

test('reconcileRoomAudience: unknown, inactive and personal Spaces are skipped before Control is called', async () => {
  await withEnv(ON, null, async (fetches) => {
    const cases = [
      [{ space: null, audience: null }, 'space_not_found'],
      [sharedRoom(null, { kind: 'personal' }), 'personal_space'],
      [sharedRoom(null, { lifecycle: 'deleting' }), 'space_not_active'],
    ];
    for (const [read, reason] of cases) {
      const { ctx } = makeCtx({ reads: [read] });
      assert.deepEqual(await run(ctx), { status: 'skipped', reason });
    }
    assert.deepEqual(await run(makeCtx({ reads: [sharedRoom()] }).ctx, '   '), {
      status: 'skipped',
      reason: 'space_not_found',
    });
    assert.equal(fetches.length, 0);
  });
});

test('reconcileRoomAudience: a Control Space that is not active, or is another org, is skipped', async () => {
  for (const overrides of [{ state: 'pending_registration' }, { state: 'deleting' }, { org_id: 'org_other' }]) {
    await withEnv(ON, async () => jsonResponse(candidatesBody(overrides)), async () => {
      const { ctx, calls } = makeCtx({ reads: [sharedRoom()] });
      assert.deepEqual(await run(ctx), { status: 'skipped', reason: 'control_state' });
      assert.equal(calls.replaces.length, 0);
    });
  }
});

test('reconcileRoomAudience: a Control 5xx or network error is control_unavailable and writes nothing', async () => {
  await withEnv(ON, async () => jsonResponse({ error: 'down' }, 503), async () => {
    const { ctx, calls } = makeCtx({ reads: [sharedRoom()] });
    assert.deepEqual(await run(ctx), { status: 'control_unavailable', httpStatus: 503 });
    assert.equal(calls.replaces.length, 0);
  });
  await withEnv(ON, async () => { throw new Error('ECONNREFUSED'); }, async () => {
    const { ctx, calls } = makeCtx({ reads: [sharedRoom()] });
    assert.deepEqual(await run(ctx), { status: 'control_unavailable', httpStatus: undefined });
    assert.equal(calls.replaces.length, 0);
  });
});

test('reconcileRoomAudience: a malformed candidate body is candidates_invalid, never an empty audience', async () => {
  const bodies = [
    { data: { org_id: ORG, state: 'active' } },
    { data: { org_id: ORG, state: 'active', members: 'nope' } },
    { data: { org_id: 5, state: 'active', members: [] } },
    { data: null },
    {},
  ];
  for (const body of bodies) {
    await withEnv(ON, async () => jsonResponse(body), async () => {
      const { ctx, calls } = makeCtx({ reads: [sharedRoom(null)] });
      assert.deepEqual(await run(ctx), { status: 'candidates_invalid' });
      assert.equal(calls.replaces.length, 0);
    });
  }
  await withEnv(ON, async () => new Response('<html>not json</html>', { status: 200 }), async () => {
    const { ctx, calls } = makeCtx({ reads: [sharedRoom(null)] });
    assert.deepEqual(await run(ctx), { status: 'candidates_invalid' });
    assert.equal(calls.replaces.length, 0);
  });
});

test('reconcileRoomAudience: a valid but empty member list is refused, the last audience stays', async () => {
  await withEnv(ON, async () => jsonResponse(candidatesBody({ members: [] })), async () => {
    const { ctx, calls } = makeCtx({ reads: [sharedRoom(active())] });
    assert.deepEqual(await run(ctx), { status: 'refused', reason: 'empty_candidates' });
    assert.equal(calls.replaces.length, 0);
  });
});

test('reconcileRoomAudience: calls Control with the service headers on the encoded Space ref', async () => {
  await withEnv(ON, async () => jsonResponse(candidatesBody()), async (fetches) => {
    const { ctx } = makeCtx({ reads: [sharedRoom(active())] });
    await run(ctx, 'space/1 x');
    assert.equal(fetches[0].url, 'http://control.test/spaces/space%2F1%20x/audience-candidates');
    assert.equal(fetches[0].init.headers['X-Service-Id'], 'application-space-lifecycle');
    assert.equal(fetches[0].init.headers['X-Service-Token'], 'tttttttt');
  });
});

test('reconcileRoomAudience: noop and waiting do not register a revision', async () => {
  await withEnv(ON, async () => jsonResponse(candidatesBody()), async () => {
    const same = makeCtx({ reads: [sharedRoom(active())] });
    assert.deepEqual(await run(same.ctx), { status: 'noop' });
    assert.equal(same.calls.replaces.length, 0);

    const pending = makeCtx({
      reads: [sharedRoom(active({ controlState: 'pending', recipientExternalAuthIds: ['u1'] }))],
    });
    assert.deepEqual(await run(pending.ctx), { status: 'waiting' });
    assert.equal(pending.calls.replaces.length, 0);
  });
});

test('reconcileRoomAudience: no audience registers revision one without an expectedRevision, sorted', async () => {
  await withEnv(ON, async () => jsonResponse(candidatesBody()), async () => {
    const { ctx, calls } = makeCtx({ reads: [sharedRoom(null)], replace: () => ({ revision: 1 }) });
    assert.deepEqual(await run(ctx), { status: 'replaced', revision: 1, recipients: 2 });
    assert.deepEqual(calls.replaces, [{ spaceRef: 'space_1', recipientExternalAuthIds: ['u1', 'u2'] }]);
    assert.equal('expectedRevision' in calls.replaces[0], false);
  });
});

test('reconcileRoomAudience: a changed roster replaces with the active revision as expectedRevision', async () => {
  await withEnv(ON, async () => jsonResponse(candidatesBody()), async () => {
    const { ctx, calls } = makeCtx({
      reads: [sharedRoom(active({ revision: 4, recipientExternalAuthIds: ['u1'] }))],
      replace: () => ({ revision: 5 }),
    });
    assert.deepEqual(await run(ctx), { status: 'replaced', revision: 5, recipients: 2 });
    assert.equal(calls.replaces[0].expectedRevision, 4);
  });
});

test('reconcileRoomAudience: a compare-and-set conflict is re-read and retried once', async () => {
  await withEnv(ON, async () => jsonResponse(candidatesBody()), async () => {
    const { ctx, calls } = makeCtx({
      reads: [
        sharedRoom(active({ revision: 4, recipientExternalAuthIds: ['u1'] })),
        sharedRoom(active({ revision: 5, recipientExternalAuthIds: ['u1'] })),
      ],
      replace: (args, call) => {
        if (call === 1) throw new Error('recipient audience revision conflict');
        return { revision: 6 };
      },
    });
    assert.deepEqual(await run(ctx), { status: 'replaced', revision: 6, recipients: 2 });
    assert.equal(calls.reads, 2);
    assert.deepEqual(
      calls.replaces.map((args) => args.expectedRevision),
      [4, 5],
    );
  });
});

test('reconcileRoomAudience: a retry that finds the roster already applied is a noop', async () => {
  await withEnv(ON, async () => jsonResponse(candidatesBody()), async () => {
    const { ctx, calls } = makeCtx({
      reads: [sharedRoom(active({ revision: 4, recipientExternalAuthIds: ['u1'] })), sharedRoom(active({ revision: 5 }))],
      replace: () => {
        throw new Error('recipient audience replacement requires expected revision');
      },
    });
    assert.deepEqual(await run(ctx), { status: 'noop' });
    assert.equal(calls.replaces.length, 1);
  });
});

test('reconcileRoomAudience: a second consecutive conflict gives up as conflict (one retry only)', async () => {
  await withEnv(ON, async () => jsonResponse(candidatesBody()), async () => {
    const { ctx, calls } = makeCtx({
      reads: [sharedRoom(active({ revision: 4, recipientExternalAuthIds: ['u1'] }))],
      replace: () => {
        throw new Error('recipient audience revision conflict');
      },
    });
    assert.deepEqual(await run(ctx), { status: 'conflict' });
    assert.equal(calls.replaces.length, 2);
  });
});

test('reconcileRoomAudience: a person not yet projected into Application is projection_lag', async () => {
  await withEnv(ON, async () => jsonResponse(candidatesBody()), async () => {
    const { ctx, calls } = makeCtx({
      reads: [sharedRoom(null)],
      replace: () => {
        throw new Error('recipient user is not an active Application organization member');
      },
    });
    assert.deepEqual(await run(ctx), { status: 'projection_lag' });
    assert.equal(calls.replaces.length, 1);
  });
});

test('reconcileRoomAudience: active Space required is skipped, anything else is failed', async () => {
  await withEnv(ON, async () => jsonResponse(candidatesBody()), async () => {
    const skipped = makeCtx({
      reads: [sharedRoom(null)],
      replace: () => {
        throw new Error('active Space required');
      },
    });
    assert.deepEqual(await run(skipped.ctx), { status: 'skipped', reason: 'space_not_active' });
    const failed = makeCtx({
      reads: [sharedRoom(null)],
      replace: () => {
        throw new Error('boom');
      },
    });
    assert.deepEqual(await run(failed.ctx), { status: 'failed' });
  });
});

test('reconcileAllRoomAudiences: disabled does nothing; one failing Space never stops the rest', async () => {
  await withEnv({ ...ON, SPACE_AUDIENCE_RECONCILE: '0' }, null, async () => {
    const ctx = {
      runQuery: async () => assert.fail('must not query'),
      runAction: async () => assert.fail('must not run'),
    };
    assert.deepEqual(await reconcileAllRoomAudiences._handler(ctx, {}), { status: 'disabled', rooms: 0 });
  });
  await withEnv(ON, null, async () => {
    const seen = [];
    const ctx = {
      runQuery: async (ref) => {
        assert.equal(getFunctionName(ref), 'spaces:listActiveSharedSpaces');
        return [{ spaceRef: 's1' }, { spaceRef: 's2' }, { spaceRef: 's3' }];
      },
      runAction: async (ref, args) => {
        assert.equal(getFunctionName(ref), 'spaceAudienceReconcile:reconcileRoomAudience');
        seen.push(args.spaceRef);
        if (args.spaceRef === 's1') throw new Error('boom');
        return args.spaceRef === 's2' ? { status: 'replaced', revision: 2, recipients: 1 } : { status: 'noop' };
      },
    };
    assert.deepEqual(await reconcileAllRoomAudiences._handler(ctx, {}), { status: 'ok', rooms: 3, replaced: 1 });
    assert.deepEqual(seen, ['s1', 's2', 's3']);
  });
});
