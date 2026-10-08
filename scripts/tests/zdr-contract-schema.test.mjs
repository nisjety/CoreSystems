// Loader tests for the two shared ZDR goldens (Z-10):
//
//   apps/Model Plane/api/contracts/zdr-posture-matrix.json
//   apps/Model Plane/api/contracts/connection-zdr-capabilities.json
//
// Every consuming plane (Auth Core TS, mp-contracts Rust, the BFF Rust, integration-corev2 Go)
// decodes these files strictly in its own language and evaluates the rows with ITS OWN real
// function. This file is the reference for what "strict" means, and it checks the golden itself:
// a golden with an unknown field, a row without a code, a code the Z-05a catalogue does not
// have, or a row that breaks a safety property is a broken contract however green the planes are.
//
// Run:  node --test scripts/tests/zdr-contract-schema.test.mjs
//
// It never skips: an absent golden is a failure, because a contract test that passes when its
// subject is missing is not a contract (the describeIfPolicy no-op trap).
//
// Mutation tests mutate an in-memory clone and assert the SPECIFIC finding, so a validator that
// rejects everything cannot pass them.

import test from 'node:test';
import assert from 'node:assert/strict';

import {
  CAPABILITIES_PATH,
  MANIFEST_REQUIRED_TAGS,
  MATRIX_PATH,
  Z05A_REFUSAL_CODES,
  canonicalJson,
  digestOf,
  loadGoldens,
  validateAll,
  validateCapabilities,
  validatePostureMatrix,
  validateRefusalCoverage,
} from './zdr-contract-schema.mjs';

const { matrix, capabilities } = loadGoldens();

const clone = (value) => structuredClone(value);
const findings = (m, c = capabilities) => validatePostureMatrix(m, c);
const capFindings = (c) => validateCapabilities(c);
const row = (doc, table, id) => {
  const found = doc[table].find((r) => r.id === id);
  assert.ok(found, `fixture row ${table}/${id} exists`);
  return found;
};
function assertFinding(list, pattern, message) {
  assert.ok(
    list.some((finding) => pattern.test(finding)),
    `${message ?? 'expected a finding'} matching ${pattern}; got:\n  ${list.join('\n  ') || '(none)'}`,
  );
}

// --- The goldens as shipped --------------------------------------------------------------

test('the shipped goldens load and validate with no findings', () => {
  assert.deepEqual(validateAll({ matrix, capabilities }), []);
});

test('the golden paths are where the build manifest says they are', () => {
  assert.equal(MATRIX_PATH, 'apps/Model Plane/api/contracts/zdr-posture-matrix.json');
  assert.equal(CAPABILITIES_PATH, 'apps/Model Plane/api/contracts/connection-zdr-capabilities.json');
});

test('every scenario the build manifest requires has a row', () => {
  const present = new Set();
  for (const table of ['org_posture_rows', 'decision_rows', 'enforcement_rows']) {
    for (const r of matrix[table]) for (const tag of r.tags) present.add(tag);
  }
  for (const tag of MANIFEST_REQUIRED_TAGS) assert.ok(present.has(tag), `a row is tagged ${tag}`);
  for (const tag of MANIFEST_REQUIRED_TAGS) {
    assert.ok(matrix.required_tags.includes(tag), `required_tags in the file lists ${tag}`);
  }
});

test('plan lapse follows the applied default (OD-LAPSE): stored intent stays ZDR', () => {
  assert.equal(matrix.policy.plan_lapse, 'intent_stays_zdr');
  assert.equal(matrix.policy.plan_lapse_status, 'applied_default_pending_owner_confirmation');
  const org = row(matrix, 'org_posture_rows', 'o_plan_lapsed_keeps_zdr');
  assert.equal(org.inputs.intent, true);
  assert.equal(org.inputs.entitled, false);
  assert.equal(org.expect.state, 'zdr');
  assert.equal(org.expect.lapsed, true);
  assert.equal(org.expect.persist_content, false);
  const decision = row(matrix, 'decision_rows', 'd_plan_lapsed_keeps_zdr');
  assert.equal(decision.expect.decision, 'zdr');
});

test('Space ZDR is pinned as not applied in v1 and the row names the known limit', () => {
  assert.equal(matrix.policy.space_zdr, 'not_applied_in_v1');
  const space = row(matrix, 'decision_rows', 'd_space_zdr_not_applied_org_off');
  assert.equal(space.inputs.space_zdr, true);
  assert.equal(space.expect.decision, 'persistent');
  assert.match(space.known_limit, /L14/);
});

test('every refusal code a row can expect is in the Z-05a catalogue', () => {
  for (const code of matrix.vocabulary.refusal_codes) {
    assert.ok(Z05A_REFUSAL_CODES.includes(code), `${code} is a Z-05a catalogue code`);
  }
  for (const table of ['org_posture_rows', 'decision_rows', 'enforcement_rows']) {
    for (const r of matrix[table]) {
      const code = r.expect.code;
      assert.ok(
        matrix.vocabulary.refusal_codes.includes(code) || code in matrix.vocabulary.reason_codes,
        `${table}/${r.id} expects ${code}`,
      );
    }
  }
});

// --- Loader strictness: matrix -----------------------------------------------------------

test('an unknown top-level field is rejected', () => {
  const m = clone(matrix);
  m.extra = 1;
  assertFinding(findings(m), /\$: unknown field "extra"/);
});

for (const [where, mutate, pattern] of [
  ['policy', (m) => (m.policy.extra = 1), /policy: unknown field "extra"/],
  ['vocabulary', (m) => (m.vocabulary.extra = 1), /vocabulary: unknown field "extra"/],
  [
    'a decision row',
    (m) => (row(m, 'decision_rows', 'd_valid_ack_waives').extra = 1),
    /d_valid_ack_waives: unknown field "extra"/,
  ],
  [
    'decision inputs',
    (m) => (row(m, 'decision_rows', 'd_valid_ack_waives').inputs.extra = 1),
    /d_valid_ack_waives\.inputs: unknown field "extra"/,
  ],
  [
    'decision expect',
    (m) => (row(m, 'decision_rows', 'd_valid_ack_waives').expect.extra = 1),
    /d_valid_ack_waives\.expect: unknown field "extra"/,
  ],
  [
    'decision claims',
    (m) => (row(m, 'decision_rows', 'd_valid_ack_waives').expect.claims.extra = 1),
    /d_valid_ack_waives\.expect\.claims: unknown field "extra"/,
  ],
  [
    'an org posture row',
    (m) => (row(m, 'org_posture_rows', 'o_plan_lapsed_keeps_zdr').inputs.extra = 1),
    /o_plan_lapsed_keeps_zdr\.inputs: unknown field "extra"/,
  ],
  [
    'an enforcement route',
    (m) => (row(m, 'enforcement_rows', 'e_waiver_on_bound_route').inputs.route.extra = 1),
    /e_waiver_on_bound_route\.inputs\.route: unknown field "extra"/,
  ],
  [
    'an enforcement bearer',
    (m) => (row(m, 'enforcement_rows', 'e_waiver_on_bound_route').inputs.bearers[0].extra = 1),
    /e_waiver_on_bound_route\.inputs\.bearers\[0\]: unknown field "extra"/,
  ],
  [
    'a waiver claim',
    (m) => (row(m, 'enforcement_rows', 'e_waiver_on_bound_route').inputs.bearers[0].waiver.extra = 1),
    /e_waiver_on_bound_route\.inputs\.bearers\[0\]\.waiver: unknown field "extra"/,
  ],
]) {
  test(`an unknown field in ${where} is rejected`, () => {
    const m = clone(matrix);
    mutate(m);
    assertFinding(findings(m), pattern);
  });
}

test('a missing input field is rejected, not defaulted', () => {
  const m = clone(matrix);
  delete row(m, 'decision_rows', 'd_valid_ack_waives').inputs.waiver_enabled;
  assertFinding(findings(m), /d_valid_ack_waives\.inputs: missing field "waiver_enabled"/);
});

for (const table of ['org_posture_rows', 'decision_rows', 'enforcement_rows']) {
  test(`a ${table} row without a code is rejected`, () => {
    const m = clone(matrix);
    delete m[table][0].expect.code;
    assertFinding(findings(m), new RegExp(`${m[table][0].id}\\.expect: missing field "code"`));
  });

  test(`a ${table} row with an empty code is rejected`, () => {
    const m = clone(matrix);
    m[table][0].expect.code = '';
    assertFinding(findings(m), new RegExp(`${m[table][0].id}\\.expect\\.code: .*not a known code`));
  });

  test(`a ${table} row with a code outside the catalogue is rejected`, () => {
    const m = clone(matrix);
    m[table][0].expect.code = 'zdr_invented_code';
    assertFinding(findings(m), new RegExp(`${m[table][0].id}\\.expect\\.code: .*not a known code`));
  });
}

test('a refusal carrying a reason code is rejected, and so is a non-refusal carrying a refusal', () => {
  const m = clone(matrix);
  row(m, 'decision_rows', 'd_no_ack_refused').expect.code = 'org_zdr';
  assertFinding(findings(m), /d_no_ack_refused\.expect\.code: a refuse row needs a refusal code/);

  const n = clone(matrix);
  row(n, 'decision_rows', 'd_valid_ack_waives').expect.code = 'zdr_connection_not_acknowledged';
  assertFinding(findings(n), /d_valid_ack_waives\.expect\.code: .*reason code of waived/);
});

test('duplicate row ids and duplicate inputs are rejected', () => {
  const m = clone(matrix);
  m.decision_rows[1].id = m.decision_rows[0].id;
  assertFinding(findings(m), /duplicate row id/);

  const n = clone(matrix);
  const copy = clone(n.decision_rows[0]);
  copy.id = 'd_copy_of_first_row';
  n.decision_rows.push(copy);
  assertFinding(findings(n), /d_copy_of_first_row: same inputs as/);
});

test('removing the last row that carries a required tag is rejected', () => {
  for (const tag of MANIFEST_REQUIRED_TAGS) {
    const m = clone(matrix);
    for (const table of ['org_posture_rows', 'decision_rows', 'enforcement_rows']) {
      for (const r of m[table]) r.tags = r.tags.filter((t) => t !== tag);
    }
    assertFinding(findings(m), new RegExp(`required tag "${tag}" is carried by no row`));
  }
});

test('a value outside its enum is rejected', () => {
  for (const [field, bad] of [
    ['connection', 'someone_else'],
    ['capability', 'maybe'],
    ['ack', 'sort_of'],
    ['outage', 'nats'],
  ]) {
    const m = clone(matrix);
    row(m, 'decision_rows', 'd_valid_ack_waives').inputs[field] = bad;
    assertFinding(findings(m), new RegExp(`d_valid_ack_waives\\.inputs\\.${field}: .*${bad}`));
  }
  const m = clone(matrix);
  row(m, 'decision_rows', 'd_valid_ack_waives').expect.decision = 'allow';
  assertFinding(findings(m), /d_valid_ack_waives\.expect\.decision: .*allow/);
});

// --- Safety properties: a row may never say something unsafe ------------------------------

test('a waiver is only ever the answer for an owner, a valid acknowledgement and an enabled route', () => {
  const cases = [
    ['ack', 'revoked'],
    ['ack', 'stale_digest'],
    ['ack', 'text_version_below_minimum'],
    ['ack', 'other_connection'],
    ['ack', 'other_member'],
    ['ack', 'none'],
    ['connection', 'other_member'],
    ['connection', 'other_org'],
    ['explicit_zdr', true],
    ['waiver_enabled', false],
    ['waiver_route', false],
    ['capability', 'supported'],
    ['org_zdr_intent', false],
    ['outage', 'integration_core'],
  ];
  for (const [field, value] of cases) {
    const m = clone(matrix);
    row(m, 'decision_rows', 'd_valid_ack_waives').inputs[field] = value;
    assertFinding(findings(m), /d_valid_ack_waives: waived requires/, `a waived row with ${field}=${value}`);
  }
});

test('a non-ZDR org is persistent, and a ZDR org is never persistent', () => {
  const m = clone(matrix);
  const r = row(m, 'decision_rows', 'd_org_off_no_connection');
  r.expect = { decision: 'zdr', code: 'org_zdr', decision_call: false, claims: { zdr: true, org_zdr: true, waiver: false } };
  assertFinding(findings(m), /d_org_off_no_connection: .*org is not ZDR.*persistent/);

  const n = clone(matrix);
  const on = row(n, 'decision_rows', 'd_org_on_no_connection');
  on.expect = { decision: 'persistent', code: 'org_zdr_off', decision_call: false, claims: { zdr: false, org_zdr: false, waiver: false } };
  assertFinding(findings(n), /d_org_on_no_connection: .*ZDR org.*never persistent/);
});

test('an unreachable org-core always refuses with zdr_decision_unavailable', () => {
  const m = clone(matrix);
  const r = row(m, 'decision_rows', 'd_org_core_outage_no_connection');
  r.expect = { decision: 'zdr', code: 'org_zdr', decision_call: false, claims: { zdr: true, org_zdr: true, waiver: false } };
  assertFinding(findings(m), /d_org_core_outage_no_connection: .*org-core.*zdr_decision_unavailable/);
});

test('Temporary chat on a connection that cannot do ZDR is refused, whatever the acknowledgement', () => {
  const m = clone(matrix);
  const r = row(m, 'decision_rows', 'd_temporary_chat_refuses_subscription');
  // Served as plain ZDR on the connection: only the explicit-request guard can object to this.
  r.expect = { decision: 'zdr', code: 'explicit_zdr', decision_call: true, claims: { zdr: true, org_zdr: true, waiver: false } };
  assertFinding(findings(m), /d_temporary_chat_refuses_subscription: .*explicit/);
  const waived = clone(matrix);
  row(waived, 'decision_rows', 'd_temporary_chat_refuses_subscription').expect = {
    decision: 'waived', code: 'connection_not_zdr_capable', decision_call: true, claims: { zdr: false, org_zdr: true, waiver: true },
  };
  assertFinding(findings(waived), /d_temporary_chat_refuses_subscription: waived requires/);
});

test('decision_call is true exactly when a connection is named and the org is known to be ZDR', () => {
  const m = clone(matrix);
  row(m, 'decision_rows', 'd_org_off_subscription_needs_nothing').expect.decision_call = true;
  assertFinding(findings(m), /d_org_off_subscription_needs_nothing\.expect\.decision_call/);

  const n = clone(matrix);
  row(n, 'decision_rows', 'd_valid_ack_waives').expect.decision_call = false;
  assertFinding(findings(n), /d_valid_ack_waives\.expect\.decision_call/);
});

test('minted claims agree with the decision', () => {
  const m = clone(matrix);
  row(m, 'decision_rows', 'd_valid_ack_waives').expect.claims.zdr = true;
  assertFinding(findings(m), /d_valid_ack_waives\.expect\.claims: .*waived/);

  const n = clone(matrix);
  row(n, 'decision_rows', 'd_no_ack_refused').expect.claims = { zdr: true, org_zdr: true, waiver: false };
  assertFinding(findings(n), /d_no_ack_refused\.expect\.claims: .*refuse.*null/);
});

test('enforcement: a waiver is honoured only on exactly the bound route, with no explicit ZDR', () => {
  for (const [mutate, label] of [
    [(r) => (r.inputs.route.connection = 'conn-2'), 'another connection'],
    [(r) => (r.inputs.route.provider = 'azure-openai'), 'another provider'],
    [(r) => (r.inputs.route.connection = null), 'no connection'],
    [(r) => (r.inputs.route.explicit_zdr = true), 'explicit ZDR'],
    [(r) => (r.inputs.bearers[0].waiver.version = 2), 'an unknown waiver version'],
    [(r) => (r.inputs.bearers[0].org_zdr = false), 'org_zdr false'],
    [(r) => (r.inputs.bearers[0].zdr = true), 'zdr true'],
  ]) {
    const m = clone(matrix);
    const r = row(m, 'enforcement_rows', 'e_waiver_on_bound_route');
    mutate(r);
    assertFinding(findings(m), /e_waiver_on_bound_route: waived requires/, `a waived row with ${label}`);
  }
});

test('enforcement: bearers that disagree are always a posture mismatch', () => {
  const m = clone(matrix);
  const r = row(m, 'enforcement_rows', 'e_mixed_zdr_bearers');
  r.expect = { decision: 'zdr', code: 'org_zdr' };
  assertFinding(findings(m), /e_mixed_zdr_bearers: .*disagree.*zdr_posture_mismatch/);
});

test('enforcement: a persistent token for a ZDR org with no waiver is refused, never served', () => {
  const m = clone(matrix);
  const r = row(m, 'enforcement_rows', 'e_org_zdr_without_waiver_is_not_persistent');
  r.expect = { decision: 'persistent', code: 'org_zdr_off' };
  assertFinding(findings(m), /e_org_zdr_without_waiver_is_not_persistent: .*never persistent/);
});

test('enforcement: persistent requires a plain persistent request', () => {
  const m = clone(matrix);
  const r = row(m, 'enforcement_rows', 'e_persistent_default_route');
  r.inputs.route.explicit_zdr = true;
  assertFinding(findings(m), /e_persistent_default_route: persistent requires/);
});

test('enforcement: a waiver that contradicts its own token is a posture mismatch', () => {
  for (const id of ['e_waiver_with_zdr_true', 'e_waiver_with_org_zdr_false', 'e_waiver_without_org_zdr_claim', 'e_waiver_unknown_version']) {
    const served = clone(matrix);
    row(served, 'enforcement_rows', id).expect = { decision: 'waived', code: 'connection_not_zdr_capable' };
    assertFinding(findings(served), new RegExp(`${id}: waived requires`));

    // Refused, but under the wrong code: only the posture-mismatch guard can object.
    const wrongCode = clone(matrix);
    row(wrongCode, 'enforcement_rows', id).expect = { decision: 'refuse', code: 'zdr_waiver_route_mismatch' };
    assertFinding(findings(wrongCode), new RegExp(`${id}: a waiver on a token that contradicts it.*zdr_posture_mismatch`));
  }
});

// --- Added by the adversarial review: weakenings the first loader let through -------------

const refuseRow = (code) => ({ decision: 'refuse', code, decision_call: true, claims: null });
const zdrRow = (code) => ({ decision: 'zdr', code, decision_call: true, claims: { zdr: true, org_zdr: true, waiver: false } });

test('a zdr answer never names a connection that cannot do ZDR, is not the user own, or has an outage', () => {
  for (const [id, expect] of [
    ['d_no_ack_refused', zdrRow('org_zdr')],
    ['d_kill_switch_off_refuses_even_with_ack', zdrRow('org_zdr')],
    ['d_unknown_provider_without_ack', zdrRow('org_zdr')],
    ['d_non_owner_refused', zdrRow('org_zdr')],
    ['d_other_org_connection_refused', zdrRow('org_zdr')],
    ['d_non_owner_beats_supported_capability', zdrRow('connection_supports_zdr')],
    ['d_auth_core_outage_refuses_waiver_turn', zdrRow('org_zdr')],
    ['d_integration_core_outage_refuses_waiver_turn', zdrRow('org_zdr')],
  ]) {
    const m = clone(matrix);
    row(m, 'decision_rows', id).expect = expect;
    assertFinding(findings(m), new RegExp(`${id}: a zdr decision that names a connection needs`), `${id} answered zdr`);
  }
});

test('every dependency of the decision call fails closed, even for a connection that supports ZDR', () => {
  const m = clone(matrix);
  row(m, 'decision_rows', 'd_auth_core_outage_supported_connection').expect = zdrRow('connection_supports_zdr');
  assertFinding(findings(m), /d_auth_core_outage_supported_connection: with auth_core unavailable.*zdr_decision_unavailable/);
  const n = clone(matrix);
  row(n, 'decision_rows', 'd_integration_core_outage_refuses_waiver_turn').expect = refuseRow('zdr_connection_not_acknowledged');
  assertFinding(findings(n), /d_integration_core_outage_refuses_waiver_turn: with integration_core unavailable/);
});

test('each refusal code of the decision function has its precondition, and the receiving planes codes are not its codes', () => {
  const cases = [
    ['d_supported_connection_stays_zdr', 'zdr_connection_not_acknowledged', /zdr_connection_not_acknowledged needs/],
    ['d_non_owner_refused', 'zdr_explicit_request_not_supported_by_connection', /zdr_explicit_request_not_supported_by_connection needs/],
    ['d_no_ack_refused', 'zdr_connection_not_supported', /zdr_connection_not_supported needs/],
    ['d_no_ack_refused', 'zdr_decision_unavailable', /zdr_decision_unavailable needs an outage/],
    ['d_no_ack_refused', 'zdr_posture_mismatch', /refuses with one of .*never zdr_posture_mismatch/],
    ['d_no_ack_refused', 'zdr_waiver_route_mismatch', /refuses with one of .*never zdr_waiver_route_mismatch/],
  ];
  for (const [id, code, pattern] of cases) {
    const m = clone(matrix);
    row(m, 'decision_rows', id).expect = refuseRow(code);
    assertFinding(findings(m), pattern, `${id} refused with ${code}`);
  }
});

test('minted claims say what the org is; a request flag alone never raises org_zdr', () => {
  const m = clone(matrix);
  row(m, 'decision_rows', 'd_temporary_chat_org_off').expect.claims = { zdr: true, org_zdr: true, waiver: false };
  assertFinding(findings(m), /d_temporary_chat_org_off\.expect\.claims\.org_zdr: must be false/);
  const n = clone(matrix);
  row(n, 'decision_rows', 'd_org_on_no_connection').expect.claims = { zdr: true, org_zdr: false, waiver: false };
  assertFinding(findings(n), /d_org_on_no_connection\.expect\.claims\.org_zdr: must be true/);
});

test('enforcement: an explicit ZDR request on a connection route is refused even when a valid waiver rides along', () => {
  const m = clone(matrix);
  row(m, 'enforcement_rows', 'e_waiver_with_explicit_zdr').expect = { decision: 'zdr', code: 'explicit_zdr' };
  assertFinding(findings(m), /e_waiver_with_explicit_zdr: a ZDR request on a connection route.*with or without a waiver/);
});

test('the scenarios this task added to the manifest list cannot be dropped from required_tags', () => {
  for (const tag of ['silent_reroute', 'kill_switch', 'non_zdr_unchanged', 'no_unauthorized_persistent']) {
    const m = clone(matrix);
    m.required_tags = m.required_tags.filter((t) => t !== tag);
    assertFinding(findings(m), new RegExp(`required_tags: missing "${tag}"`), `required_tags without ${tag}`);
  }
});

// --- Cross-file consistency --------------------------------------------------------------

test('the waiver allowlist in the matrix is exactly the providers the capability contract gives a waiver route', () => {
  const m = clone(matrix);
  m.policy.waiver_providers = ['openai-codex-subscription', 'azure-openai'];
  assertFinding(findings(m), /policy\.waiver_providers: .*waiver_route/);

  const c = clone(capabilities);
  c.providers[0].waiver_route = false;
  assertFinding(findings(matrix, c), /policy\.waiver_providers: .*waiver_route/);
});

test('the matrix capability vocabulary is exactly the capability contract states', () => {
  const m = clone(matrix);
  m.vocabulary.capabilities = ['supported', 'not_supported'];
  assertFinding(findings(m), /vocabulary\.capabilities: .*states/);
});

test('rows tagged unknown_provider carry what the fallback assessment gives', () => {
  const m = clone(matrix);
  row(m, 'decision_rows', 'd_unknown_provider_has_no_waiver_route').inputs.waiver_route = true;
  assertFinding(findings(m), /d_unknown_provider_has_no_waiver_route: .*fallback/);
});

test('every refusal code of the matrix must exist in the Z-05a refusal catalogue file once it exists', () => {
  assert.deepEqual(validateRefusalCoverage(matrix, { codes: [...Z05A_REFUSAL_CODES] }), []);
  const missing = Z05A_REFUSAL_CODES.filter((c) => c !== 'zdr_posture_mismatch');
  assertFinding(
    validateRefusalCoverage(matrix, { codes: missing }),
    /zdr_posture_mismatch.*not in the refusal catalogue/,
  );
  assertFinding(validateRefusalCoverage(matrix, {}), /refusal catalogue.*codes/);
});

// --- Capability contract -----------------------------------------------------------------

test('capability contract: unknown fields are rejected at every level', () => {
  for (const [mutate, pattern] of [
    [(c) => (c.extra = 1), /\$: unknown field "extra"/],
    [(c) => (c.providers[0].extra = 1), /openai-codex-subscription@1: unknown field "extra"/],
    [(c) => (c.fallback.extra = 1), /fallback: unknown field "extra"/],
    [(c) => (c.cases[0].extra = 1), /cases\[known_codex_subscription\]: unknown field "extra"/],
    [(c) => (c.digest.extra = 1), /digest: unknown field "extra"/],
    [(c) => (c.digest.vectors[0].extra = 1), /digest\.vectors\[.*\]: unknown field "extra"/],
    [(c) => (c.wording_keys[0].extra = 1), /wording_keys\[.*\]: unknown field "extra"/],
  ]) {
    const c = clone(capabilities);
    mutate(c);
    assertFinding(capFindings(c), pattern);
  }
});

test('capability contract: a digest vector that does not hash to its canonical input is rejected', () => {
  const c = clone(capabilities);
  c.digest.vectors[0].sha256 = '0'.repeat(64);
  assertFinding(capFindings(c), /digest\.vectors\[.*\]\.sha256: .* does not match/);

  const d = clone(capabilities);
  d.digest.vectors[0].canonical = d.digest.vectors[0].canonical.replace(' ', '');
  d.digest.vectors[0].canonical += ' ';
  assertFinding(capFindings(d), /digest\.vectors\[.*\]\.canonical/);
});

test('capability contract: the digest changes with every field it covers, and not with key order', () => {
  const base = clone(capabilities.digest.vectors[0].input);
  const reference = digestOf(base);
  for (const [field, value] of [
    ['catalog_version', base.catalog_version + 1],
    ['capability', 'supported'],
    ['basis', 'provider_by_contract'],
    ['auth_mode', 'api_key'],
    ['provider_key', 'other-provider'],
  ]) {
    assert.notEqual(digestOf({ ...base, [field]: value }), reference, `${field} is covered by the digest`);
  }
  const shuffled = Object.fromEntries(Object.entries(base).reverse());
  assert.equal(digestOf(shuffled), reference);
  assert.equal(canonicalJson({ b: 1, a: [true, null, 'x'] }), '{"a":[true,null,"x"],"b":1}');
});

test('capability contract: a provider whose capability disagrees with its inference-core provider is rejected', () => {
  const c = clone(capabilities);
  c.providers[0].inference_supports_zdr = true;
  assertFinding(capFindings(c), /openai-codex-subscription@1: .*supports_zdr/);

  const d = clone(capabilities);
  d.providers[0].capability = 'supported';
  assertFinding(capFindings(d), /openai-codex-subscription@1: .*supports_zdr/);
});

test('capability contract: the ChatGPT subscription is not_supported, matching inference-core codex supports_zdr=false', () => {
  const codex = capabilities.providers.find((p) => p.provider_key === 'openai-codex-subscription');
  assert.ok(codex, 'the only connection provider that exists today is in the catalogue');
  assert.equal(codex.capability, 'not_supported');
  assert.equal(codex.basis, 'provider_terms_known');
  assert.equal(codex.inference_provider_id, 'openai-codex-subscription');
  assert.equal(codex.inference_supports_zdr, false);
  assert.equal(codex.waiver_route, true);
});

test('capability contract: the fallback is cannot_verify and has no waiver route', () => {
  for (const [field, value] of [
    ['capability', 'supported'],
    ['basis', 'provider_terms_known'],
    ['waiver_route', true],
  ]) {
    const c = clone(capabilities);
    c.fallback[field] = value;
    assertFinding(capFindings(c), new RegExp(`fallback\\.${field}`));
  }
});

test('capability contract: assessment cases must follow the exact-match-else-fallback rule', () => {
  const c = clone(capabilities);
  c.cases.find((x) => x.id === 'unknown_provider').expect.capability = 'supported';
  assertFinding(capFindings(c), /cases\[unknown_provider\]\.expect: .*fallback/);

  const d = clone(capabilities);
  d.cases.find((x) => x.id === 'known_provider_other_auth_mode').expect.capability = 'not_supported';
  assertFinding(capFindings(d), /cases\[known_provider_other_auth_mode\]\.expect: .*fallback/);

  const e = clone(capabilities);
  e.cases = e.cases.filter((x) => x.id !== 'provider_alias_is_not_a_key');
  assertFinding(capFindings(e), /cases: .*provider_alias_is_not_a_key/);
});

test('capability contract: every state that needs an acknowledgement has a first-use text, and supported never does', () => {
  const c = clone(capabilities);
  const needs = c.wording_keys.find((w) => w.state === 'not_supported');
  needs.variants = needs.variants.filter((v) => v !== 'first_use');
  assertFinding(capFindings(c), /wording_keys\[.*\]\.variants: .*first_use/);

  const d = clone(capabilities);
  const supported = d.wording_keys.find((w) => w.state === 'supported');
  supported.variants = [...supported.variants, 'first_use'];
  assertFinding(capFindings(d), /wording_keys\[.*\]\.variants: .*first_use/);
});

test('capability contract: a provider may only use a wording key of its own state', () => {
  const c = clone(capabilities);
  c.providers[0].wording_key = 'connection_cannot_verify';
  assertFinding(capFindings(c), /openai-codex-subscription@1\.wording_key: .*state/);
});

test('capability contract: basis_ref values are unique and versioned', () => {
  const c = clone(capabilities);
  c.providers[0].basis_ref = 'openai-codex-subscription';
  assertFinding(capFindings(c), /.basis_ref: must be "<provider_key>@<row revision>"/);

  const d = clone(capabilities);
  d.providers.push({ ...clone(d.providers[0]), auth_mode: 'another_mode' });
  assertFinding(capFindings(d), /.basis_ref: duplicate basis_ref/);
});

test('capability contract: the run rule covers every state exactly once', () => {
  const c = clone(capabilities);
  c.run_rule.zdr_stays_on = [];
  assertFinding(capFindings(c), /run_rule: .*every state/);
});

// --- OD-LAPSE: flipping the owner decision is one mechanical change, and a half flip is caught ---

test('flipping the plan-lapse policy without flipping the rows is rejected', () => {
  const m = clone(matrix);
  m.policy.plan_lapse = 'intent_and_entitlement';
  assertFinding(findings(m), /o_plan_lapsed_keeps_zdr.expect.state: policy.plan_lapse intent_and_entitlement makes this org not_zdr/);
  assertFinding(findings(m), /d_plan_lapsed_keeps_zdr: .*ZDR org is never persistent|d_plan_lapsed_keeps_zdr: the org is not ZDR/);
});

test('if the owner chooses intent AND entitlement, the flip is the policy plus the lapse rows and nothing else', () => {
  const m = clone(matrix);
  m.policy.plan_lapse = 'intent_and_entitlement';
  m.policy.plan_lapse_status = 'owner_confirmed';
  Object.assign(row(m, 'org_posture_rows', 'o_plan_lapsed_keeps_zdr').expect, {
    state: 'not_zdr', code: 'org_zdr_off', persist_content: true, lapsed: true,
  });
  const persistent = { decision: 'persistent', code: 'org_zdr_off', decision_call: false, claims: { zdr: false, org_zdr: false, waiver: false } };
  row(m, 'decision_rows', 'd_plan_lapsed_keeps_zdr').expect = persistent;
  row(m, 'decision_rows', 'd_plan_lapsed_org_still_waives').expect = persistent;
  assert.deepEqual(findings(m), []);
});
