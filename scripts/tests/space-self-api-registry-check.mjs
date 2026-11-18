#!/usr/bin/env node
// Structural rules for the Space self-API registry golden (apps/Model Plane/api/contracts/
// space-self-api-registry.json). scripts/tests/space-self-api-registry-sync.test.sh runs this on the
// golden and on its tampered self-test copies; the byte identity of the copies and the digest pin are
// checked by that shell test, not here.
//
// The reference loader is apps/Model Plane/rust/crates/mp-contracts/src/space_self_api.rs. The rules
// below are the ones the sync test owns, restated so that a registry edit fails here before any Rust
// or Go consumer is built:
//   * the action id tail after "space." and every argument name carry no forbidden part;
//   * only tickets.create may be a reservation (the owner reservation table admits that one action);
//   * schema_sha256 is the sha256 of parameters_json, except the legacy tickets.create row, whose hash
//     is pinned (hash_basis owner_contract) and is exempt;
//   * argument_order names each property of parameters_json exactly once and nothing else;
//   * the digest vectors reproduce their idempotency_key and payload_digest, framed as in the Rust
//     payload_digest (domain space-self-api/v1, every field a u32 big-endian length then its bytes).
//
//   node scripts/tests/space-self-api-registry-check.mjs <golden.json>
//
// Prints one "ok - <rule>" or "not ok - <rule>: <detail>" line per rule. Exit 0 only when every rule
// holds; 2 on a usage error.
import crypto from 'node:crypto';
import fs from 'node:fs';

const FORBIDDEN_NAME_PARTS = [
  'member',
  'role',
  'delete',
  'credential',
  'grant',
  'policy',
  'thread_id',
  'space',
  'org',
  'user',
  'actor',
];
const LEGACY_ACTION_ID = 'tickets.create';
const PAYLOAD_DIGEST_DOMAIN = 'space-self-api/v1';
const SHA256_COMMITMENT = /^sha256:[0-9a-f]{64}$/;
const SPACE_PREFIX = 'space.';

const sha256Hex = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
const forbiddenPart = (name) => FORBIDDEN_NAME_PARTS.find((part) => name.includes(part));

// Every field is framed as a u32 big-endian byte length followed by its UTF-8 bytes, so no value can
// forge a boundary (apps/Model Plane/rust/crates/mp-contracts/src/space_self_api.rs, frame()).
function frame(text) {
  const bytes = Buffer.from(text, 'utf8');
  const length = Buffer.alloc(4);
  length.writeUInt32BE(bytes.length);
  return Buffer.concat([length, bytes]);
}

function payloadDigest(actionId, runId, orgId, idempotencyKey, argumentPairs) {
  const parts = [
    Buffer.from(`${PAYLOAD_DIGEST_DOMAIN}\u0000`, 'utf8'),
    frame(actionId),
    frame(runId),
    frame(orgId),
    frame(idempotencyKey),
  ];
  for (const [name, value] of argumentPairs) parts.push(frame(name), frame(value));
  return `sha256:${sha256Hex(Buffer.concat(parts))}`;
}

function idempotencyKey(actionId, runId, stepId) {
  const digest = sha256Hex(Buffer.from(`${actionId}\u0000${runId}\u0000${stepId}`, 'utf8'));
  return `${actionId}:${digest}`;
}

function checkRegistry(golden) {
  const results = [];
  const check = (name, violations) => {
    results.push(
      violations.length === 0
        ? { name, ok: true }
        : { name, ok: false, detail: violations.join('; ') },
    );
  };
  const rows = Array.isArray(golden.rows) ? golden.rows : [];
  const vectors = Array.isArray(golden.digest_vectors) ? golden.digest_vectors : [];

  const idViolations = rows.length === 0 ? ['the golden has no rows'] : [];
  for (const row of rows) {
    const id = String(row.action_id);
    const tail = id.startsWith(SPACE_PREFIX) ? id.slice(SPACE_PREFIX.length) : id;
    const part = forbiddenPart(tail);
    if (part) idViolations.push(`${id} names the forbidden part "${part}"`);
  }
  check('action id tail names no forbidden part', idViolations);

  const argumentViolations = [];
  for (const row of rows) {
    for (const name of Array.isArray(row.argument_order) ? row.argument_order : []) {
      const part = forbiddenPart(String(name));
      if (part) argumentViolations.push(`${row.action_id}.${name} names the forbidden part "${part}"`);
    }
  }
  check('argument names carry no forbidden part', argumentViolations);

  check(
    'reservation admits tickets.create only',
    rows
      .filter((row) => row.reservation === true && row.action_id !== LEGACY_ACTION_ID)
      .map((row) => `${row.action_id} is a reservation row`),
  );

  const hashViolations = [];
  for (const row of rows) {
    const id = row.action_id;
    if (!SHA256_COMMITMENT.test(String(row.schema_sha256))) {
      hashViolations.push(`${id} schema_sha256 is not a sha256 commitment`);
    }
    if (row.hash_basis === 'parameters_json') {
      const derived = `sha256:${sha256Hex(Buffer.from(String(row.parameters_json), 'utf8'))}`;
      if (row.schema_sha256 !== derived) {
        hashViolations.push(`${id} schema_sha256 is not the sha256 of its parameters_json`);
      }
    } else if (row.hash_basis === 'owner_contract') {
      if (id !== LEGACY_ACTION_ID) {
        hashViolations.push(`${id} is pinned as owner_contract, which only ${LEGACY_ACTION_ID} may be`);
      }
    } else {
      hashViolations.push(`${id} has an unknown hash_basis`);
    }
  }
  check(
    'schema hash is the sha256 of parameters_json (the legacy tickets.create hash is pinned)',
    hashViolations,
  );

  const orderViolations = [];
  for (const row of rows) {
    let schema;
    try {
      schema = JSON.parse(String(row.parameters_json));
    } catch {
      orderViolations.push(`${row.action_id} parameters_json does not parse`);
      continue;
    }
    if (schema?.type !== 'object' || schema.additionalProperties !== false) {
      orderViolations.push(`${row.action_id} parameters_json is not a closed object`);
    }
    const order = Array.isArray(row.argument_order) ? row.argument_order : [];
    const properties = Object.keys(schema?.properties ?? {});
    if (new Set(order).size !== order.length) {
      orderViolations.push(`${row.action_id} argument_order repeats a name`);
    }
    const namesMatch =
      order.length === properties.length && properties.every((name) => order.includes(name));
    if (!namesMatch) {
      orderViolations.push(
        `${row.action_id} argument_order ${JSON.stringify(order)} is not its properties ${JSON.stringify(properties)}`,
      );
    }
  }
  check('argument_order names each parameter exactly once', orderViolations);

  const digestViolations = [];
  for (const vector of vectors) {
    const key = idempotencyKey(vector.action_id, vector.run_id, vector.step_id);
    if (key !== vector.idempotency_key) {
      digestViolations.push(`${vector.name} idempotency_key recomputes to ${key}`);
    }
    const digest = payloadDigest(
      vector.action_id,
      vector.run_id,
      vector.org_id,
      vector.idempotency_key,
      Array.isArray(vector.argument_pairs) ? vector.argument_pairs : [],
    );
    if (digest !== vector.payload_digest) {
      digestViolations.push(`${vector.name} payload_digest recomputes to ${digest}`);
    }
  }
  check('digest vectors reproduce idempotency_key and payload_digest', digestViolations);

  const hasRenameVector = vectors.some((vector) => vector.action_id === 'space.thread.rename');
  check(
    'a space.thread.rename digest vector is present',
    hasRenameVector ? [] : ['no digest vector names space.thread.rename'],
  );

  return results;
}

function main(argv) {
  const file = argv[2];
  if (!file) {
    console.error('usage: space-self-api-registry-check.mjs <golden.json>');
    return 2;
  }
  let golden;
  try {
    golden = JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (error) {
    console.log(`not ok - the golden parses as one JSON document: ${error.message}`);
    return 1;
  }
  const results = checkRegistry(golden);
  for (const result of results) {
    console.log(result.ok ? `ok - ${result.name}` : `not ok - ${result.name}: ${result.detail}`);
  }
  return results.every((result) => result.ok) ? 0 : 1;
}

process.exitCode = main(process.argv);
