// Reference loader and self-consistency checks for the two shared ZDR goldens (Z-10):
//
//   apps/Model Plane/api/contracts/zdr-posture-matrix.json
//   apps/Model Plane/api/contracts/connection-zdr-capabilities.json
//
// What this is, and is not. Every consuming plane decodes the goldens strictly in its own
// language and evaluates the rows with ITS OWN real function (Auth Core TS, mp-contracts Rust,
// the BFF Rust, integration-corev2 Go). Nothing here evaluates a row: this module checks that
// the golden itself is well formed (no unknown field, no missing field, no row without a code,
// codes only from the catalogue), that no row says something unsafe (a waiver for a revoked
// acknowledgement, a persistent answer for a ZDR org, a mismatched token set that is served),
// and that the two files agree with each other. The safety checks are necessary conditions, not
// a second resolver: they would let a wrong row through that is merely wrong, never one that
// breaks a stated safety property.
//
// Used by scripts/tests/zdr-contract-sync.test.sh (repo-level drift check) and by
// scripts/tests/zdr-contract-schema.test.mjs.
//
//   node scripts/tests/zdr-contract-schema.mjs [repoRoot] [--refusals <repo-relative path>]
//
// Exit status is non-zero on any finding. A missing golden is a finding, never a skip.

import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const MATRIX_PATH = 'apps/Model Plane/api/contracts/zdr-posture-matrix.json';
export const CAPABILITIES_PATH = 'apps/Model Plane/api/contracts/connection-zdr-capabilities.json';

/**
 * The Z-05a refusal catalogue as the build manifest fixes it (task Z-05a, corrections), plus the
 * three kind strings CR-T3 froze. The matrix may only expect codes from here. Z-05a replaces this
 * constant with a read of its own zdr-refusals.json by listing that file in the sync script
 * (validateRefusalCoverage); until then this is the authority.
 */
export const Z05A_REFUSAL_CODES = [
  'zdr_unavailable',
  'zdr_feature_not_supported',
  'zdr_connection_not_supported',
  'zdr_connection_not_acknowledged',
  'zdr_explicit_request_not_supported_by_connection',
  'zdr_decision_unavailable',
  'zdr_waiver_route_mismatch',
  'zdr_posture_mismatch',
  'residency_unavailable',
  'tier_unavailable',
];

/** Scenarios the build manifest (Z-10 corrections) requires the matrix to cover. */
export const MANIFEST_REQUIRED_TAGS = [
  'plan_lapsed',
  'temporary_chat',
  'space_scoped',
  'other_connection',
  'other_member',
  'revoked_ack',
  'stale_digest',
  'text_version_below_minimum',
  'unknown_provider',
  'outage_auth_core',
  'outage_org_core',
  'outage_integration_core',
  'non_owner',
  'mixed_zdr_token_set',
];

/** Scenarios this task added to the manifest's list; removing one from required_tags is a finding too. */
export const ADDED_REQUIRED_TAGS = ['silent_reroute', 'kill_switch', 'non_zdr_unchanged', 'no_unauthorized_persistent'];

const DECISIONS = ['zdr', 'persistent', 'waived', 'refuse'];
const ORG_STATES = ['zdr', 'not_zdr', 'unknown'];
const CONNECTIONS = ['none', 'own', 'other_member', 'other_org'];
const CAPABILITY_STATES = ['supported', 'not_supported', 'cannot_verify'];
const BASES = [
  'verevon_attested_deployment',
  'provider_terms_known',
  'provider_by_contract',
  'customer_asserted',
  'not_assessed',
];
const ACKS = [
  'none',
  'valid',
  'revoked',
  'stale_digest',
  'text_version_below_minimum',
  'other_connection',
  'other_member',
];
const OUTAGES = ['org_core', 'auth_core', 'integration_core'];
const LOOKUPS = ['ok', 'unavailable'];
const WORDING_VARIANTS = ['org_zdr_on', 'org_zdr_off', 'first_use'];
const LOCALES = ['no', 'en'];
const REASON_DECISION = {
  org_zdr_off: 'persistent',
  org_zdr: 'zdr',
  explicit_zdr: 'zdr',
  connection_supports_zdr: 'zdr',
  connection_not_zdr_capable: 'waived',
};
const DIGEST_FIELDS = ['auth_mode', 'basis', 'capability', 'catalog_version', 'provider_key'];
const REQUIRED_CASE_IDS = [
  'unknown_provider',
  'known_provider_other_auth_mode',
  'empty_provider',
  'provider_alias_is_not_a_key',
];

const SNAKE = /^[a-z][a-z0-9_]*$/;
const BASIS_REF = /^[a-z0-9][a-z0-9-]*@[0-9]+$/;
const SHA256_HEX = /^[0-9a-f]{64}$/;

// ---- small helpers -----------------------------------------------------------------------

const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
const isString = (v) => typeof v === 'string';
const isNonEmptyString = (v) => typeof v === 'string' && v.trim() !== '';
const sameSet = (a, b) => a.length === b.length && a.every((x) => b.includes(x)) && new Set(a).size === a.length;
const sameList = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);

/** Canonical JSON: keys in ascending order, no whitespace outside strings, UTF-8. */
export function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  if (isObject(value)) {
    return `{${Object.keys(value)
      .sort()
      .map((k) => `${JSON.stringify(k)}:${canonicalJson(value[k])}`)
      .join(',')}}`;
  }
  return JSON.stringify(value);
}

const sha256Hex = (text) => createHash('sha256').update(text, 'utf8').digest('hex');

/** The capability digest as the contract defines it: sha256 of the canonical JSON of its fields. */
export function digestOf(input) {
  return sha256Hex(canonicalJson(input));
}

/**
 * Check an object has exactly the given fields. Returns true when it is safe to read the
 * required fields afterwards. A missing field is a finding, never a default; an unknown field is
 * a finding, never ignored.
 */
function exactKeys(value, required, path, out, optional = []) {
  if (!isObject(value)) {
    out.push(`${path}: must be an object`);
    return false;
  }
  for (const key of Object.keys(value)) {
    if (!required.includes(key) && !optional.includes(key)) out.push(`${path}: unknown field "${key}"`);
  }
  let ok = true;
  for (const key of required) {
    if (!(key in value)) {
      out.push(`${path}: missing field "${key}"`);
      ok = false;
    }
  }
  return ok;
}

function oneOf(value, allowed, path, out) {
  if (!allowed.includes(value)) {
    out.push(`${path}: ${JSON.stringify(value)} is not one of ${allowed.join(', ')}`);
    return false;
  }
  return true;
}

function isBool(value, path, out, { nullable = false } = {}) {
  if (typeof value === 'boolean' || (nullable && value === null)) return true;
  out.push(`${path}: must be ${nullable ? 'a boolean or null' : 'a boolean'}`);
  return false;
}

function stringList(value, path, out) {
  if (!Array.isArray(value) || value.some((v) => !isNonEmptyString(v))) {
    out.push(`${path}: must be a list of non-empty strings`);
    return false;
  }
  if (new Set(value).size !== value.length) {
    out.push(`${path}: must not repeat a value`);
    return false;
  }
  return true;
}

// ---- the posture matrix ------------------------------------------------------------------

const DECISION_INPUTS = [
  'org_zdr_intent',
  'org_entitled',
  'explicit_zdr',
  'space_zdr',
  'waiver_enabled',
  'connection',
  'capability',
  'waiver_route',
  'ack',
  'outage',
];
const ORG_INPUTS = ['intent', 'entitled', 'lookup'];
const ENFORCEMENT_INPUT_DOC_KEYS = ['bearers', 'bearers[].zdr', 'bearers[].org_zdr', 'bearers[].waiver', 'route'];
const TABLES = ['org_posture_rows', 'decision_rows', 'enforcement_rows'];
const TABLE_PREFIX = { org_posture_rows: 'o_', decision_rows: 'd_', enforcement_rows: 'e_' };

function orgIsZdr(policy, intent, entitled) {
  return policy.plan_lapse === 'intent_and_entitlement' ? intent === true && entitled === true : intent === true;
}

/**
 * Validate the posture matrix. `capabilities` (the other golden) enables the cross-file checks;
 * pass it whenever it is available.
 */
export function validatePostureMatrix(doc, capabilities) {
  const out = [];
  const top = [
    'contract',
    'version',
    'waiver_contract_version',
    'description',
    'policy',
    'vocabulary',
    'rule',
    'required_tags',
    ...TABLES,
  ];
  if (!exactKeys(doc, top, '$', out)) return out;
  if (doc.contract !== 'zdr.posture-matrix') out.push('contract: must be "zdr.posture-matrix"');
  if (doc.version !== 1) out.push('version: must be 1 (a format change bumps it)');
  if (!Number.isInteger(doc.waiver_contract_version) || doc.waiver_contract_version < 1) {
    out.push('waiver_contract_version: must be a positive integer');
  }
  if (!isNonEmptyString(doc.description)) out.push('description: must be a non-empty string');

  const policy = checkPolicy(doc.policy, out);
  const vocab = checkVocabulary(doc.vocabulary, out);
  if (!Array.isArray(doc.rule) || doc.rule.length === 0 || doc.rule.some((r) => !isNonEmptyString(r))) {
    out.push('rule: must be a non-empty list of strings');
  }
  if (!stringList(doc.required_tags, 'required_tags', out)) return out;
  for (const tag of MANIFEST_REQUIRED_TAGS) {
    if (!doc.required_tags.includes(tag)) out.push(`required_tags: missing "${tag}" (the build manifest requires it)`);
  }
  for (const tag of ADDED_REQUIRED_TAGS) {
    if (!doc.required_tags.includes(tag)) out.push(`required_tags: missing "${tag}" (a scenario no edit may remove)`);
  }
  if (!policy || !vocab) return out;

  const ids = new Set();
  const carried = new Set();
  for (const table of TABLES) {
    if (!Array.isArray(doc[table]) || doc[table].length === 0) {
      out.push(`${table}: must be a non-empty list of rows`);
      continue;
    }
    const seenInputs = new Map();
    for (const row of doc[table]) {
      const id = isObject(row) && isString(row.id) ? row.id : '(row without id)';
      const path = `${table}.${id}`;
      if (!exactKeys(row, ['id', 'title', 'tags', 'inputs', 'expect'], path, out, ['note', 'known_limit'])) continue;
      if (!SNAKE.test(row.id) || !row.id.startsWith(TABLE_PREFIX[table])) {
        out.push(`${path}.id: must be snake_case and start with "${TABLE_PREFIX[table]}"`);
      }
      if (ids.has(row.id)) out.push(`${path}: duplicate row id`);
      ids.add(row.id);
      if (!isNonEmptyString(row.title)) out.push(`${path}.title: must be a non-empty string`);
      for (const optionalText of ['note', 'known_limit']) {
        if (optionalText in row && !isNonEmptyString(row[optionalText])) {
          out.push(`${path}.${optionalText}: must be a non-empty string when present`);
        }
      }
      if (Array.isArray(row.tags) && row.tags.every((t) => isString(t) && SNAKE.test(t))) {
        for (const tag of row.tags) carried.add(tag);
      } else {
        out.push(`${path}.tags: must be a list of snake_case strings`);
      }
      const key = canonicalJson(row.inputs);
      if (seenInputs.has(key)) out.push(`${path}: same inputs as ${seenInputs.get(key)}; rows must differ`);
      else seenInputs.set(key, row.id);

      if (table === 'org_posture_rows') checkOrgRow(row, path, policy, vocab, out);
      else if (table === 'decision_rows') checkDecisionRow(row, path, policy, vocab, out, capabilities);
      else checkEnforcementRow(row, path, doc, policy, vocab, out);
    }
  }
  for (const tag of doc.required_tags) {
    if (!carried.has(tag)) out.push(`required tag "${tag}" is carried by no row`);
  }

  if (capabilities && isObject(capabilities) && Array.isArray(capabilities.states)) {
    if (!sameList(vocab.capabilities ?? [], capabilities.states)) {
      out.push('vocabulary.capabilities: must equal the capability contract states');
    }
    if (Array.isArray(capabilities.providers)) {
      const waivable = capabilities.providers.filter((p) => p.waiver_route === true).map((p) => p.provider_key);
      if (!sameSet(policy.waiver_providers, waivable)) {
        out.push(
          `policy.waiver_providers: must equal the provider_key of every capability provider with waiver_route true (${waivable.join(', ') || 'none'})`,
        );
      }
      const capable = capabilities.providers.filter((p) => p.capability === 'supported').map((p) => p.provider_key);
      if (!sameSet(policy.zdr_capable_connection_providers, capable)) {
        out.push(
          `policy.zdr_capable_connection_providers: must equal the provider_key of every capability provider whose capability is supported (${capable.join(', ') || 'none'})`,
        );
      }
    }
  }
  return out;
}

function checkPolicy(policy, out) {
  const keys = ['plan_lapse', 'plan_lapse_status', 'space_zdr', 'waiver_providers', 'zdr_capable_connection_providers'];
  if (!exactKeys(policy, keys, 'policy', out)) return null;
  oneOf(policy.plan_lapse, ['intent_stays_zdr', 'intent_and_entitlement'], 'policy.plan_lapse', out);
  oneOf(
    policy.plan_lapse_status,
    ['applied_default_pending_owner_confirmation', 'owner_confirmed'],
    'policy.plan_lapse_status',
    out,
  );
  oneOf(policy.space_zdr, ['not_applied_in_v1'], 'policy.space_zdr', out);
  const a = stringList(policy.waiver_providers, 'policy.waiver_providers', out);
  const b =
    Array.isArray(policy.zdr_capable_connection_providers) &&
    (policy.zdr_capable_connection_providers.length === 0 ||
      stringList(policy.zdr_capable_connection_providers, 'policy.zdr_capable_connection_providers', out));
  if (!Array.isArray(policy.zdr_capable_connection_providers)) {
    out.push('policy.zdr_capable_connection_providers: must be a list');
  }
  return a && b ? policy : null;
}

function checkVocabulary(v, out) {
  const keys = [
    'decisions',
    'retention_classes',
    'refusal_codes',
    'reason_codes',
    'org_states',
    'connections',
    'capabilities',
    'acks',
    'outages',
    'fields',
  ];
  if (!exactKeys(v, keys, 'vocabulary', out)) return null;
  if (!sameList(v.decisions ?? [], DECISIONS)) out.push(`vocabulary.decisions: must be exactly ${DECISIONS.join(', ')}`);
  if (
    !isObject(v.retention_classes) ||
    !sameSet(Object.keys(v.retention_classes), ['zdr', 'persistent', 'waived']) ||
    v.retention_classes.zdr !== 'zdr' ||
    v.retention_classes.persistent !== 'persistent' ||
    v.retention_classes.waived !== 'zdr_waived'
  ) {
    out.push('vocabulary.retention_classes: must map zdr to zdr, persistent to persistent, waived to zdr_waived');
  }
  if (!stringList(v.refusal_codes, 'vocabulary.refusal_codes', out)) return null;
  for (const code of v.refusal_codes) {
    if (!Z05A_REFUSAL_CODES.includes(code)) {
      out.push(`vocabulary.refusal_codes: "${code}" is not a code of the Z-05a refusal catalogue`);
    }
  }
  if (!isObject(v.reason_codes) || JSON.stringify(Object.entries(v.reason_codes).sort()) !== JSON.stringify(Object.entries(REASON_DECISION).sort())) {
    out.push('vocabulary.reason_codes: must map each reason code to the one decision it explains');
  }
  if (!sameList(v.org_states ?? [], ORG_STATES)) out.push(`vocabulary.org_states: must be exactly ${ORG_STATES.join(', ')}`);
  if (!isObject(v.connections) || !sameSet(Object.keys(v.connections), CONNECTIONS)) {
    out.push(`vocabulary.connections: must describe exactly ${CONNECTIONS.join(', ')}`);
  }
  if (!Array.isArray(v.capabilities)) out.push('vocabulary.capabilities: must be a list');
  if (!isObject(v.acks) || !sameSet(Object.keys(v.acks), ACKS)) {
    out.push(`vocabulary.acks: must describe exactly ${ACKS.join(', ')}`);
  }
  if (!isObject(v.outages) || !sameSet(Object.keys(v.outages), OUTAGES)) {
    out.push(`vocabulary.outages: must describe exactly ${OUTAGES.join(', ')}`);
  }
  for (const map of ['connections', 'acks', 'outages']) {
    if (isObject(v[map]) && Object.values(v[map]).some((d) => !isNonEmptyString(d))) {
      out.push(`vocabulary.${map}: every value needs a description`);
    }
  }
  const fieldsOk = exactKeys(v.fields, ['org', 'decision', 'enforcement'], 'vocabulary.fields', out);
  if (fieldsOk) {
    if (!isObject(v.fields.org) || !sameSet(Object.keys(v.fields.org), ORG_INPUTS)) {
      out.push(`vocabulary.fields.org: must document exactly ${ORG_INPUTS.join(', ')}`);
    }
    if (!isObject(v.fields.decision) || !sameSet(Object.keys(v.fields.decision), DECISION_INPUTS)) {
      out.push(`vocabulary.fields.decision: must document exactly ${DECISION_INPUTS.join(', ')}`);
    }
    if (!isObject(v.fields.enforcement) || !sameSet(Object.keys(v.fields.enforcement), ENFORCEMENT_INPUT_DOC_KEYS)) {
      out.push(`vocabulary.fields.enforcement: must document exactly ${ENFORCEMENT_INPUT_DOC_KEYS.join(', ')}`);
    }
  }
  return v;
}

/** The code of a row must be a catalogue code (refusal) or a known reason code of its decision. */
function checkCode(code, decision, path, vocab, out) {
  const known = vocab.refusal_codes.includes(code) || Object.hasOwn(REASON_DECISION, code);
  if (!isString(code) || !known) {
    out.push(`${path}.code: ${JSON.stringify(code)} is not a known code (a Z-05a refusal code or a reason code)`);
    return false;
  }
  if (decision === 'refuse') {
    if (!vocab.refusal_codes.includes(code)) {
      out.push(`${path}.code: a refuse row needs a refusal code, got reason code "${code}"`);
      return false;
    }
  } else if (REASON_DECISION[code] !== decision) {
    out.push(`${path}.code: "${code}" is not a reason code of ${decision}`);
    return false;
  }
  return true;
}

function checkOrgRow(row, path, policy, vocab, out) {
  const inputsOk = exactKeys(row.inputs, ORG_INPUTS, `${path}.inputs`, out);
  const expectOk = exactKeys(row.expect, ['state', 'code', 'persist_content', 'lapsed'], `${path}.expect`, out);
  if (!inputsOk || !expectOk) return;
  const { intent, entitled, lookup } = row.inputs;
  const { state, code, persist_content: persist, lapsed } = row.expect;
  oneOf(lookup, LOOKUPS, `${path}.inputs.lookup`, out);
  if (lookup === 'ok') {
    isBool(intent, `${path}.inputs.intent`, out);
    isBool(entitled, `${path}.inputs.entitled`, out);
  } else if (intent !== null || entitled !== null) {
    out.push(`${path}.inputs: intent and entitled must be null when the lookup is unavailable`);
  }
  if (!oneOf(state, ORG_STATES, `${path}.expect.state`, out)) return;
  isBool(persist, `${path}.expect.persist_content`, out);
  isBool(lapsed, `${path}.expect.lapsed`, out);
  const wantCode = { zdr: 'org_zdr', not_zdr: 'org_zdr_off', unknown: 'zdr_decision_unavailable' }[state];
  if (!isString(code) || !(vocab.refusal_codes.includes(code) || Object.hasOwn(REASON_DECISION, code))) {
    out.push(`${path}.expect.code: ${JSON.stringify(code)} is not a known code (a Z-05a refusal code or a reason code)`);
  } else if (code !== wantCode) {
    out.push(`${path}.expect.code: state ${state} carries ${wantCode}, got ${code}`);
  }
  if (persist !== (state === 'not_zdr')) {
    out.push(`${path}.expect.persist_content: only a known non-ZDR org may persist content; unknown is never read as not ZDR`);
  }
  if (lookup === 'unavailable') {
    if (state !== 'unknown') out.push(`${path}.expect.state: an unavailable lookup is unknown`);
    return;
  }
  const want = orgIsZdr(policy, intent, entitled) ? 'zdr' : 'not_zdr';
  if (state !== want) out.push(`${path}.expect.state: policy.plan_lapse ${policy.plan_lapse} makes this org ${want}`);
  if (lapsed !== (intent === true && entitled === false)) {
    out.push(`${path}.expect.lapsed: true exactly when intent is on and the entitlement is gone`);
  }
}

function checkDecisionRow(row, path, policy, vocab, out, capabilities) {
  const inputsOk = exactKeys(row.inputs, DECISION_INPUTS, `${path}.inputs`, out);
  const expectOk = exactKeys(row.expect, ['decision', 'code', 'decision_call', 'claims'], `${path}.expect`, out);
  if (!inputsOk || !expectOk) return;
  const i = row.inputs;
  const e = row.expect;

  let typed = true;
  typed = isBool(i.org_zdr_intent, `${path}.inputs.org_zdr_intent`, out, { nullable: true }) && typed;
  typed = isBool(i.org_entitled, `${path}.inputs.org_entitled`, out, { nullable: true }) && typed;
  typed = isBool(i.explicit_zdr, `${path}.inputs.explicit_zdr`, out) && typed;
  typed = isBool(i.space_zdr, `${path}.inputs.space_zdr`, out) && typed;
  typed = isBool(i.waiver_enabled, `${path}.inputs.waiver_enabled`, out) && typed;
  typed = oneOf(i.connection, CONNECTIONS, `${path}.inputs.connection`, out) && typed;
  typed = oneOf(i.capability, [...CAPABILITY_STATES, null], `${path}.inputs.capability`, out) && typed;
  typed = isBool(i.waiver_route, `${path}.inputs.waiver_route`, out, { nullable: true }) && typed;
  typed = oneOf(i.ack, ACKS, `${path}.inputs.ack`, out) && typed;
  typed = oneOf(i.outage, [...OUTAGES, null], `${path}.inputs.outage`, out) && typed;
  typed = oneOf(e.decision, DECISIONS, `${path}.expect.decision`, out) && typed;
  typed = isBool(e.decision_call, `${path}.expect.decision_call`, out) && typed;
  if (!typed) return;
  checkCode(e.code, e.decision, `${path}.expect`, vocab, out);

  if ((i.org_zdr_intent === null) !== (i.outage === 'org_core') || (i.org_entitled === null) !== (i.outage === 'org_core')) {
    out.push(`${path}.inputs: org_zdr_intent and org_entitled are null exactly when the outage is org_core`);
  }
  if (i.connection === 'none') {
    if (i.capability !== null || i.waiver_route !== null || i.ack !== 'none') {
      out.push(`${path}.inputs: with no connection named, capability and waiver_route are null and ack is none`);
    }
  } else if (i.capability === null || i.waiver_route === null) {
    out.push(`${path}.inputs: a named connection needs a capability and a waiver_route`);
  }

  const orgKnown = i.outage !== 'org_core';
  const orgZdr = orgKnown && orgIsZdr(policy, i.org_zdr_intent, i.org_entitled);
  const named = i.connection !== 'none';
  const cannotZdr = i.capability === 'not_supported' || i.capability === 'cannot_verify';

  // Safety properties: necessary conditions, not an oracle.
  if (orgKnown && !orgZdr && !i.explicit_zdr && e.decision !== 'persistent') {
    out.push(`${path}: the org is not ZDR and the turn asks for nothing, so the decision must be persistent`);
  }
  if (orgKnown && orgZdr && e.decision === 'persistent') {
    out.push(`${path}: a ZDR org is never persistent; a turn that cannot be ZDR is waived or refused`);
  }
  if (e.decision === 'waived') {
    const ok =
      orgZdr &&
      !i.explicit_zdr &&
      i.connection === 'own' &&
      cannotZdr &&
      i.ack === 'valid' &&
      i.waiver_enabled &&
      i.waiver_route === true &&
      i.outage === null;
    if (!ok) {
      out.push(
        `${path}: waived requires an org with ZDR on, no explicit ZDR, the session user's own connection that cannot do ZDR, a valid acknowledgement, the kill switch on, a provider with a waiver route and no outage`,
      );
    }
  }
  if (i.outage === 'org_core' && !(e.decision === 'refuse' && e.code === 'zdr_decision_unavailable')) {
    out.push(`${path}: with org-core unavailable the org posture is unknown and the turn must refuse with zdr_decision_unavailable`);
  }
  if (i.explicit_zdr && i.connection === 'own' && cannotZdr && i.outage === null) {
    if (!(e.decision === 'refuse' && e.code === 'zdr_explicit_request_not_supported_by_connection')) {
      out.push(
        `${path}: explicit ZDR on a connection that cannot do ZDR must refuse with zdr_explicit_request_not_supported_by_connection; a waiver never covers it`,
      );
    }
  }
  if (e.code === 'connection_supports_zdr' && !(i.connection === 'own' && i.capability === 'supported')) {
    out.push(`${path}.expect.code: connection_supports_zdr needs the session user's own supported connection`);
  }
  if (e.code === 'org_zdr' && (i.explicit_zdr || (named && i.capability === 'supported'))) {
    out.push(`${path}.expect.code: org_zdr is the reason only when nothing more specific applies (explicit ZDR and a supported connection come first)`);
  }
  if (e.code === 'explicit_zdr' && !i.explicit_zdr) {
    out.push(`${path}.expect.code: explicit_zdr needs explicit_zdr in the inputs`);
  }
  // A zdr answer for a turn that names a connection exists only for the session user's own
  // connection that supports ZDR, with every dependency up. A connection that cannot do ZDR is
  // waived (valid acknowledgement) or refused; it is never answered "zdr" as if it were the org default.
  if (e.decision === 'zdr' && named && !(i.connection === 'own' && i.capability === 'supported' && i.outage === null)) {
    out.push(
      `${path}: a zdr decision that names a connection needs the session user's own connection that supports ZDR and no outage; any other connection is waived or refused`,
    );
  }
  // Every dependency of the decision call fails closed.
  if (named && orgZdr && (i.outage === 'auth_core' || i.outage === 'integration_core')) {
    if (!(e.decision === 'refuse' && e.code === 'zdr_decision_unavailable')) {
      out.push(`${path}: with ${i.outage} unavailable and a connection named in a ZDR org the turn must refuse with zdr_decision_unavailable`);
    }
  }
  // The decision function refuses with its own four codes, and each one has a precondition.
  if (e.decision === 'refuse') {
    const decisionCodes = [
      'zdr_connection_not_acknowledged',
      'zdr_connection_not_supported',
      'zdr_explicit_request_not_supported_by_connection',
      'zdr_decision_unavailable',
    ];
    if (!decisionCodes.includes(e.code)) {
      out.push(`${path}.expect.code: the decision function refuses with one of ${decisionCodes.join(', ')}, never ${e.code} (that code belongs to a plane that receives tokens)`);
    } else if (e.code === 'zdr_decision_unavailable' && i.outage === null) {
      out.push(`${path}.expect.code: zdr_decision_unavailable needs an outage in the inputs`);
    } else if (e.code === 'zdr_explicit_request_not_supported_by_connection' && !(i.explicit_zdr && named)) {
      out.push(`${path}.expect.code: zdr_explicit_request_not_supported_by_connection needs an explicit ZDR request that names a connection`);
    } else if (
      e.code === 'zdr_connection_not_supported' &&
      !(
        orgZdr &&
        i.connection === 'own' &&
        cannotZdr &&
        !i.explicit_zdr &&
        i.outage === null &&
        (!i.waiver_enabled || i.waiver_route === false)
      )
    ) {
      out.push(
        `${path}.expect.code: zdr_connection_not_supported needs a ZDR org, the session user's own connection that cannot do ZDR, no explicit ZDR, no outage, and a kill switch that is off or a provider with no waiver route (otherwise acknowledging could help)`,
      );
    } else if (
      e.code === 'zdr_connection_not_acknowledged' &&
      !(
        named &&
        (orgZdr || i.explicit_zdr) &&
        (i.connection !== 'own' || cannotZdr) &&
        i.outage === null &&
        !(i.connection === 'own' && (!i.waiver_enabled || i.waiver_route === false))
      )
    ) {
      out.push(
        `${path}.expect.code: zdr_connection_not_acknowledged needs a named connection that is not the session user's own, or one that cannot do ZDR with the kill switch on and a waiver route, and no outage (when acknowledging cannot help the answer is zdr_connection_not_supported)`,
      );
    }
  }

  const wantCall = named && orgKnown && orgZdr;
  if (e.decision_call !== wantCall) {
    out.push(
      `${path}.expect.decision_call: must be ${wantCall}: the BFF calls the decision exactly when a connection is named and the org is known to be ZDR`,
    );
  }

  checkDecisionClaims(e, i, path, out);
  // What a minted token says about the org is the org's posture; the request flag alone never changes it.
  if (e.decision !== 'refuse' && isObject(e.claims) && typeof e.claims.org_zdr === 'boolean' && e.claims.org_zdr !== orgZdr) {
    out.push(`${path}.expect.claims.org_zdr: must be ${orgZdr}, the org's posture; the request flag alone never changes what a token says about the org`);
  }

  if (capabilities && isObject(capabilities) && Array.isArray(row.tags) && row.tags.includes('unknown_provider')) {
    const fb = capabilities.fallback;
    if (isObject(fb) && named && (i.capability !== fb.capability || i.waiver_route !== fb.waiver_route)) {
      out.push(
        `${path}: a row tagged unknown_provider must carry what the capability contract fallback gives (capability ${fb.capability}, waiver_route ${fb.waiver_route})`,
      );
    }
  }
}

function checkDecisionClaims(e, i, path, out) {
  if (e.decision === 'refuse') {
    if (e.claims !== null) out.push(`${path}.expect.claims: a refuse decision mints no tokens, so claims must be null`);
    return;
  }
  if (!exactKeys(e.claims, ['zdr', 'org_zdr', 'waiver'], `${path}.expect.claims`, out)) return;
  const c = e.claims;
  let ok = true;
  for (const k of ['zdr', 'org_zdr', 'waiver']) ok = isBool(c[k], `${path}.expect.claims.${k}`, out) && ok;
  if (!ok) return;
  if (e.decision === 'waived') {
    if (!(c.zdr === false && c.org_zdr === true && c.waiver === true)) {
      out.push(`${path}.expect.claims: a waived decision mints {zdr: false, org_zdr: true, waiver: true}`);
    }
  } else if (e.decision === 'persistent') {
    if (!(c.zdr === false && c.org_zdr === false && c.waiver === false)) {
      out.push(`${path}.expect.claims: a persistent decision mints {zdr: false, org_zdr: false, waiver: false}`);
    }
  } else {
    if (c.waiver !== false) out.push(`${path}.expect.claims.waiver: a zdr decision never carries a waiver`);
    if (c.zdr !== c.org_zdr) out.push(`${path}.expect.claims: a zdr decision mints zdr and org_zdr alike`);
    if (c.zdr === false && !i.explicit_zdr) {
      out.push(`${path}.expect.claims: tokens are minted persistent only when the request flag alone raises the turn to ZDR`);
    }
  }
}

function checkClaimsObject(c, path, out) {
  if (!exactKeys(c, ['zdr', 'org_zdr', 'waiver'], path, out)) return false;
  let ok = isBool(c.zdr, `${path}.zdr`, out);
  ok = isBool(c.org_zdr, `${path}.org_zdr`, out, { nullable: true }) && ok;
  if (c.waiver !== null) {
    if (exactKeys(c.waiver, ['version', 'provider', 'connection'], `${path}.waiver`, out)) {
      if (!Number.isInteger(c.waiver.version) || c.waiver.version < 1) out.push(`${path}.waiver.version: must be a positive integer`);
      if (!isNonEmptyString(c.waiver.provider)) out.push(`${path}.waiver.provider: must be a non-empty string`);
      if (!isNonEmptyString(c.waiver.connection)) out.push(`${path}.waiver.connection: must be a non-empty string`);
    } else {
      ok = false;
    }
  }
  return ok;
}

function checkEnforcementRow(row, path, doc, policy, vocab, out) {
  const inputsOk = exactKeys(row.inputs, ['bearers', 'route'], `${path}.inputs`, out);
  const expectOk = exactKeys(row.expect, ['decision', 'code'], `${path}.expect`, out);
  if (!inputsOk || !expectOk) return;
  const { bearers, route } = row.inputs;
  const e = row.expect;
  let ok = true;
  if (!Array.isArray(bearers) || bearers.length === 0) {
    out.push(`${path}.inputs.bearers: must be a non-empty list`);
    ok = false;
  } else {
    bearers.forEach((b, n) => {
      ok = checkClaimsObject(b, `${path}.inputs.bearers[${n}]`, out) && ok;
    });
  }
  if (exactKeys(route, ['provider', 'connection', 'explicit_zdr'], `${path}.inputs.route`, out)) {
    if (!isString(route.provider)) out.push(`${path}.inputs.route.provider: must be a string ("" is the default chain)`);
    if (route.connection !== null && !isNonEmptyString(route.connection)) {
      out.push(`${path}.inputs.route.connection: must be a non-empty string or null`);
    }
    ok = isBool(route.explicit_zdr, `${path}.inputs.route.explicit_zdr`, out) && ok;
  } else {
    ok = false;
  }
  ok = oneOf(e.decision, DECISIONS, `${path}.expect.decision`, out) && ok;
  if (!ok) return;
  if (!checkCode(e.code, e.decision, `${path}.expect`, vocab, out)) return;

  const waiverKey = (b) => canonicalJson({ zdr: b.zdr, waiver: b.waiver });
  const agree = bearers.every((b) => waiverKey(b) === waiverKey(bearers[0]));
  const b = bearers[0];
  const connectionRoute = policy.waiver_providers.includes(route.provider) && !policy.zdr_capable_connection_providers.includes(route.provider);

  if (!agree) {
    if (!(e.decision === 'refuse' && e.code === 'zdr_posture_mismatch')) {
      out.push(`${path}: bearers that disagree on zdr or waiver must be refused with zdr_posture_mismatch`);
    }
    return;
  }
  const contradictory = b.waiver !== null && (b.zdr || b.org_zdr !== true);
  const unknownVersion = b.waiver !== null && b.waiver.version !== doc.waiver_contract_version;
  if ((contradictory || unknownVersion) && !(e.decision === 'refuse' && e.code === 'zdr_posture_mismatch')) {
    out.push(`${path}: a waiver on a token that contradicts it, or at a version this plane does not know, must be refused with zdr_posture_mismatch`);
  }
  if (b.waiver === null && b.zdr === false && b.org_zdr === true && e.decision !== 'refuse') {
    out.push(`${path}: a token that says the org is ZDR but carries no zdr and no waiver is never persistent; it must be refused`);
  }
  // With or without a waiver: a waiver covers the org default only, so a token that says zdr, or a
  // request that asks for ZDR, is refused on a connection route that cannot do ZDR.
  if ((b.zdr || route.explicit_zdr) && connectionRoute && e.decision !== 'refuse') {
    out.push(`${path}: a ZDR request on a connection route that cannot do ZDR is never served, with or without a waiver; it must be refused`);
  }
  if (e.decision === 'persistent') {
    if (b.zdr || b.org_zdr === true || b.waiver !== null || route.explicit_zdr) {
      out.push(`${path}: persistent requires a token that is not ZDR, no waiver and no explicit ZDR`);
    }
  }
  if (e.decision === 'waived') {
    const w = b.waiver;
    const good =
      w !== null &&
      w.version === doc.waiver_contract_version &&
      policy.waiver_providers.includes(w.provider) &&
      route.provider === w.provider &&
      route.connection === w.connection &&
      !route.explicit_zdr &&
      b.org_zdr === true &&
      b.zdr === false;
    if (!good) {
      out.push(
        `${path}: waived requires a waiver at the current version for an allowed provider, exactly the bound provider and connection on the route, no explicit ZDR, org_zdr true and zdr false`,
      );
    }
  }
  if (e.decision === 'zdr') {
    if (!(b.zdr || route.explicit_zdr)) out.push(`${path}: a zdr decision needs a ZDR token or an explicit ZDR request`);
    const want = route.explicit_zdr ? 'explicit_zdr' : 'org_zdr';
    if (e.code !== want) out.push(`${path}.expect.code: this request is zdr because of ${want}, got ${e.code}`);
  }
  if (e.code === 'connection_supports_zdr' && !policy.zdr_capable_connection_providers.includes(route.provider)) {
    out.push(`${path}.expect.code: connection_supports_zdr needs a provider in policy.zdr_capable_connection_providers`);
  }
}

// ---- the capability contract -------------------------------------------------------------

export function validateCapabilities(doc) {
  const out = [];
  const top = [
    'contract',
    'version',
    'catalog_version',
    'description',
    'states',
    'bases',
    'run_rule',
    'locales',
    'wording_variants',
    'wording_keys',
    'digest',
    'providers',
    'fallback',
    'cases',
  ];
  if (!exactKeys(doc, top, '$', out)) return out;
  if (doc.contract !== 'connection.zdr-capabilities') out.push('contract: must be "connection.zdr-capabilities"');
  if (doc.version !== 1) out.push('version: must be 1 (a format change bumps it)');
  if (!Number.isInteger(doc.catalog_version) || doc.catalog_version < 1) out.push('catalog_version: must be a positive integer');
  if (!isNonEmptyString(doc.description)) out.push('description: must be a non-empty string');
  if (!sameList(doc.states ?? [], CAPABILITY_STATES)) out.push(`states: must be exactly ${CAPABILITY_STATES.join(', ')}`);
  if (!sameList(doc.bases ?? [], BASES)) out.push(`bases: must be exactly ${BASES.join(', ')}`);
  if (!sameList(doc.locales ?? [], LOCALES)) out.push(`locales: must be exactly ${LOCALES.join(', ')}`);
  if (!sameList(doc.wording_variants ?? [], WORDING_VARIANTS)) {
    out.push(`wording_variants: must be exactly ${WORDING_VARIANTS.join(', ')}`);
  }

  if (exactKeys(doc.run_rule, ['zdr_stays_on', 'acknowledgement_required'], 'run_rule', out)) {
    const a = doc.run_rule.zdr_stays_on;
    const b = doc.run_rule.acknowledgement_required;
    if (!Array.isArray(a) || !Array.isArray(b) || !sameSet([...a, ...b], CAPABILITY_STATES)) {
      out.push('run_rule: zdr_stays_on and acknowledgement_required must cover every state exactly once');
    } else if (!sameList(a, ['supported'])) {
      out.push('run_rule.zdr_stays_on: only supported keeps ZDR on; cannot_verify is never rounded up');
    }
  }

  const wording = new Map();
  if (Array.isArray(doc.wording_keys)) {
    for (const w of doc.wording_keys) {
      const path = `wording_keys[${isObject(w) && isString(w.key) ? w.key : '?'}]`;
      if (!exactKeys(w, ['key', 'state', 'variants'], path, out)) continue;
      if (!SNAKE.test(w.key)) out.push(`${path}.key: must be snake_case`);
      if (wording.has(w.key)) out.push(`${path}: duplicate wording key`);
      wording.set(w.key, w);
      if (!oneOf(w.state, CAPABILITY_STATES, `${path}.state`, out)) continue;
      if (!Array.isArray(w.variants) || w.variants.some((v) => !WORDING_VARIANTS.includes(v)) || new Set(w.variants).size !== w.variants.length) {
        out.push(`${path}.variants: must be a list of ${WORDING_VARIANTS.join(', ')} without repeats`);
        continue;
      }
      const needsAck = (doc.run_rule?.acknowledgement_required ?? []).includes(w.state);
      if (needsAck && !['org_zdr_on', 'org_zdr_off', 'first_use'].every((v) => w.variants.includes(v))) {
        out.push(`${path}.variants: a state that needs an acknowledgement needs org_zdr_on, org_zdr_off and first_use texts`);
      }
      if (!needsAck && (w.variants.includes('first_use') || !['org_zdr_on', 'org_zdr_off'].every((v) => w.variants.includes(v)))) {
        out.push(`${path}.variants: a state that needs no acknowledgement needs org_zdr_on and org_zdr_off texts and never first_use`);
      }
    }
    for (const state of CAPABILITY_STATES) {
      if (![...wording.values()].some((w) => w.state === state)) out.push(`wording_keys: no key for state ${state}`);
    }
  } else {
    out.push('wording_keys: must be a list');
  }

  const providers = checkProviders(doc, wording, out);
  checkFallback(doc, wording, out);
  checkDigest(doc, providers, out);
  checkCases(doc, providers, out);
  return out;
}

function checkProviders(doc, wording, out) {
  const rows = [];
  if (!Array.isArray(doc.providers) || doc.providers.length === 0) {
    out.push('providers: must be a non-empty list');
    return rows;
  }
  const refs = new Set();
  const pairs = new Set();
  const keys = [
    'basis_ref',
    'provider_key',
    'auth_mode',
    'capability',
    'basis',
    'waiver_route',
    'wording_key',
    'inference_provider_id',
    'inference_supports_zdr',
    'evidence',
  ];
  for (const p of doc.providers) {
    const path = `providers.${isObject(p) && isString(p.basis_ref) ? p.basis_ref : '?'}`;
    if (!exactKeys(p, keys, path, out)) continue;
    rows.push(p);
    if (!BASIS_REF.test(p.basis_ref) || p.basis_ref.split('@')[0] !== p.provider_key) {
      out.push(`${path}.basis_ref: must be "<provider_key>@<row revision>", a versioned row id`);
    }
    if (refs.has(p.basis_ref)) out.push(`${path}.basis_ref: duplicate basis_ref`);
    refs.add(p.basis_ref);
    if (!isNonEmptyString(p.provider_key) || !isNonEmptyString(p.auth_mode)) {
      out.push(`${path}: provider_key and auth_mode must be non-empty strings`);
    }
    const pair = `${p.provider_key}\u0000${p.auth_mode}`;
    if (pairs.has(pair)) out.push(`${path}: provider_key and auth_mode pair repeats`);
    pairs.add(pair);
    oneOf(p.capability, CAPABILITY_STATES, `${path}.capability`, out);
    oneOf(p.basis, BASES, `${path}.basis`, out);
    isBool(p.waiver_route, `${path}.waiver_route`, out);
    if (p.capability === 'supported' && !['verevon_attested_deployment', 'customer_asserted'].includes(p.basis)) {
      out.push(`${path}.basis: supported needs an attested deployment or a recorded customer assertion, never provider_terms_known or less`);
    }
    if (p.basis === 'not_assessed' && p.capability !== 'cannot_verify') {
      out.push(`${path}.basis: not_assessed can only mean cannot_verify`);
    }
    const w = wording.get(p.wording_key);
    if (!w) out.push(`${path}.wording_key: "${p.wording_key}" is not a wording key`);
    else if (w.state !== p.capability) {
      out.push(`${path}.wording_key: "${p.wording_key}" is a ${w.state} text but the provider is ${p.capability} (state mismatch)`);
    }
    if ((p.inference_provider_id === null) !== (p.inference_supports_zdr === null)) {
      out.push(`${path}: inference_provider_id and inference_supports_zdr are both set or both null`);
    } else if (p.inference_provider_id !== null) {
      if (!isNonEmptyString(p.inference_provider_id) || typeof p.inference_supports_zdr !== 'boolean') {
        out.push(`${path}: inference_provider_id must be a string and inference_supports_zdr a boolean`);
      } else if ((p.capability === 'supported') !== p.inference_supports_zdr) {
        out.push(
          `${path}: capability ${p.capability} disagrees with inference-core supports_zdr=${p.inference_supports_zdr}; a connection is supported exactly when its provider reports supports_zdr`,
        );
      }
    }
    if (!isNonEmptyString(p.evidence)) out.push(`${path}.evidence: must say what the assessment rests on`);
  }
  return rows;
}

function checkFallback(doc, wording, out) {
  const f = doc.fallback;
  if (!exactKeys(f, ['basis_ref', 'capability', 'basis', 'waiver_route', 'wording_key'], 'fallback', out)) return;
  if (!/^fallback@[0-9]+$/.test(f.basis_ref)) out.push('fallback.basis_ref: must be "fallback@<row revision>"');
  if (f.capability !== 'cannot_verify') out.push('fallback.capability: an unassessed connection is cannot_verify, never supported or not_supported');
  if (f.basis !== 'not_assessed') out.push('fallback.basis: must be not_assessed');
  if (f.waiver_route !== false) out.push('fallback.waiver_route: an unassessed provider has no waiver route');
  const w = wording.get(f.wording_key);
  if (!w || w.state !== 'cannot_verify') out.push('fallback.wording_key: must be a cannot_verify wording key');
}

function checkDigest(doc, providers, out) {
  const d = doc.digest;
  if (!exactKeys(d, ['algorithm', 'encoding', 'fields', 'canonical_form', 'vectors'], 'digest', out)) return;
  if (d.algorithm !== 'sha256') out.push('digest.algorithm: must be sha256');
  if (d.encoding !== 'lowercase_hex') out.push('digest.encoding: must be lowercase_hex');
  if (!Array.isArray(d.fields) || !sameList(d.fields, DIGEST_FIELDS)) {
    out.push(`digest.fields: must be exactly ${DIGEST_FIELDS.join(', ')} in ascending order`);
  }
  if (!isNonEmptyString(d.canonical_form)) out.push('digest.canonical_form: must describe the canonical form');
  if (!Array.isArray(d.vectors) || d.vectors.length < 3) {
    out.push('digest.vectors: at least three vectors (the current digest, a catalog_version bump, a capability change)');
    return;
  }
  const seen = new Set();
  const hashes = new Set();
  for (const v of d.vectors) {
    const path = `digest.vectors[${isObject(v) && isString(v.id) ? v.id : '?'}]`;
    if (!exactKeys(v, ['id', 'note', 'input', 'canonical', 'sha256'], path, out)) continue;
    if (!SNAKE.test(v.id) || seen.has(v.id)) out.push(`${path}.id: must be unique snake_case`);
    seen.add(v.id);
    if (!isNonEmptyString(v.note)) out.push(`${path}.note: must say what the vector shows`);
    if (!exactKeys(v.input, DIGEST_FIELDS, `${path}.input`, out)) continue;
    const canonical = canonicalJson(v.input);
    if (v.canonical !== canonical) {
      out.push(`${path}.canonical: ${JSON.stringify(v.canonical)} does not equal the canonical form of input ${JSON.stringify(canonical)}`);
    }
    const expected = sha256Hex(canonical);
    if (!SHA256_HEX.test(String(v.sha256)) || v.sha256 !== expected) {
      out.push(`${path}.sha256: ${JSON.stringify(v.sha256)} does not match the sha256 of canonical (${expected})`);
    }
    if (hashes.has(v.sha256)) out.push(`${path}.sha256: two vectors share a digest, so one of them shows nothing`);
    hashes.add(v.sha256);
  }
  // The assessor must be able to check its own digest against a vector for every provider row at
  // the current catalog_version; a bump without a new vector is caught here.
  for (const p of providers) {
    const want = canonicalJson({
      provider_key: p.provider_key,
      auth_mode: p.auth_mode,
      capability: p.capability,
      basis: p.basis,
      catalog_version: doc.catalog_version,
    });
    if (!d.vectors.some((v) => isObject(v) && isObject(v.input) && canonicalJson(v.input) === want)) {
      out.push(`digest.vectors: no vector covers ${p.basis_ref} at catalog_version ${doc.catalog_version}`);
    }
  }
}

function assessmentFor(doc, input) {
  const hit = (doc.providers ?? []).find(
    (p) => isObject(p) && p.provider_key === input.provider_key && p.auth_mode === input.auth_mode,
  );
  const row = hit ?? doc.fallback;
  return {
    basis_ref: row.basis_ref,
    capability: row.capability,
    basis: row.basis,
    waiver_route: row.waiver_route,
    wording_key: row.wording_key,
  };
}

function checkCases(doc, providers, out) {
  if (!Array.isArray(doc.cases) || doc.cases.length === 0) {
    out.push('cases: must be a non-empty list');
    return;
  }
  if (!isObject(doc.fallback)) return;
  const ids = new Set();
  for (const c of doc.cases) {
    const path = `cases[${isObject(c) && isString(c.id) ? c.id : '?'}]`;
    if (!exactKeys(c, ['id', 'input', 'expect'], path, out)) continue;
    if (!SNAKE.test(c.id) || ids.has(c.id)) out.push(`${path}.id: must be unique snake_case`);
    ids.add(c.id);
    const inputOk = exactKeys(c.input, ['provider_key', 'auth_mode'], `${path}.input`, out);
    const expectOk = exactKeys(
      c.expect,
      ['basis_ref', 'capability', 'basis', 'waiver_route', 'wording_key'],
      `${path}.expect`,
      out,
    );
    if (!inputOk || !expectOk) continue;
    if (!isString(c.input.provider_key) || !isString(c.input.auth_mode)) {
      out.push(`${path}.input: provider_key and auth_mode must be strings (empty is allowed)`);
      continue;
    }
    const want = assessmentFor(doc, c.input);
    if (canonicalJson(c.expect) !== canonicalJson(want)) {
      out.push(
        `${path}.expect: must equal the exact-match-else-fallback assessment ${JSON.stringify(want)}; only an exact (provider_key, auth_mode) row is assessed, anything else gets the fallback`,
      );
    }
  }
  for (const id of REQUIRED_CASE_IDS) {
    if (!ids.has(id)) out.push(`cases: required case "${id}" is missing`);
  }
  for (const p of providers) {
    if (!doc.cases.some((c) => isObject(c) && isObject(c.input) && c.input.provider_key === p.provider_key && c.input.auth_mode === p.auth_mode)) {
      out.push(`cases: no case assesses ${p.basis_ref}`);
    }
  }
}

// ---- cross-file checks and the refusal catalogue ----------------------------------------------

function extractRefusalCodes(refusals) {
  const list = Array.isArray(refusals) ? refusals : (refusals?.codes ?? refusals?.refusals);
  if (!Array.isArray(list)) return null;
  const codes = list.map((entry) => (isString(entry) ? entry : isObject(entry) ? entry.code : undefined));
  return codes.every(isString) ? codes : null;
}

/**
 * Every refusal code the matrix can expect must exist in the refusal catalogue file Z-05a creates
 * (apps/Model Plane/api/contracts/zdr-refusals.json). Z-05a adapts extractRefusalCodes to its own
 * file shape when it lists that file in the sync script.
 */
export function validateRefusalCoverage(matrix, refusals) {
  const out = [];
  const codes = extractRefusalCodes(refusals);
  if (!codes) {
    out.push('refusal catalogue: could not find the list of codes (expected "codes": [...] of strings or of {code} objects)');
    return out;
  }
  const used = new Set(matrix?.vocabulary?.refusal_codes ?? []);
  for (const table of TABLES) {
    for (const row of matrix?.[table] ?? []) {
      if (row?.expect?.decision === 'refuse' || row?.expect?.state === 'unknown') used.add(row.expect.code);
    }
  }
  for (const code of [...used].sort()) {
    if (!codes.includes(code)) out.push(`refusal code "${code}" is not in the refusal catalogue`);
  }
  return out;
}

export function validateAll({ matrix, capabilities, refusals }) {
  const out = [];
  out.push(...validateCapabilities(capabilities).map((f) => `${CAPABILITIES_PATH}: ${f}`));
  out.push(...validatePostureMatrix(matrix, capabilities).map((f) => `${MATRIX_PATH}: ${f}`));
  if (refusals !== undefined) out.push(...validateRefusalCoverage(matrix, refusals));
  return out;
}

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

function readGolden(root, relPath) {
  const text = readFileSync(resolve(root, relPath), 'utf8');
  return { text, json: JSON.parse(text) };
}

export function loadGoldens(root = REPO_ROOT) {
  return {
    matrix: readGolden(root, MATRIX_PATH).json,
    capabilities: readGolden(root, CAPABILITIES_PATH).json,
  };
}

/** Byte-level rules for a golden: LF only, one trailing newline, no BOM. They are hashed byte for byte. */
export function validateBytes(text, name) {
  const out = [];
  if (text.includes('\r')) out.push(`${name}: has CR line endings (goldens are LF, hashed byte for byte)`);
  if (text.charCodeAt(0) === 0xfeff) out.push(`${name}: starts with a byte order mark`);
  if (!text.endsWith('\n') || text.endsWith('\n\n')) out.push(`${name}: must end with exactly one newline`);
  return out;
}

// ---- command line ----------------------------------------------------------------------------

function main(argv) {
  const args = argv.slice(2);
  let root = REPO_ROOT;
  let refusalsPath;
  for (let n = 0; n < args.length; n++) {
    if (args[n] === '--refusals') refusalsPath = args[++n];
    else root = resolve(args[n]);
  }
  const findings = [];
  let docs;
  try {
    const m = readGolden(root, MATRIX_PATH);
    const c = readGolden(root, CAPABILITIES_PATH);
    findings.push(...validateBytes(m.text, MATRIX_PATH), ...validateBytes(c.text, CAPABILITIES_PATH));
    docs = { matrix: m.json, capabilities: c.json };
    if (refusalsPath) docs.refusals = readGolden(root, refusalsPath).json;
  } catch (error) {
    findings.push(`cannot load a golden (a missing golden is a failure, never a skip): ${error.message}`);
  }
  if (docs) findings.push(...validateAll(docs));
  if (findings.length === 0) {
    process.stdout.write(`ok - ${MATRIX_PATH} and ${CAPABILITIES_PATH} are well formed and consistent\n`);
    return 0;
  }
  for (const finding of findings) process.stderr.write(`not ok - ${finding}\n`);
  return 1;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv);
}
