import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(
  new URL('./redact-container-logs.mjs', import.meta.url),
);

test('redacts supported secret shapes and bounds diagnostic output', () => {
  const secrets = [
    'json-token-value',
    'nats-user',
    'nats-password',
    'header-service-secret',
    'lowercase-secret-value',
    'bearer-secret-value',
    'cookie-secret-value',
    'pem-secret-line',
  ];
  const fixture = [
    'startup stage: database wait',
    '{"token":"json-token-value","safe":"visible"}',
    'connecting nats://nats-user:nats-password@broker:4222',
    'x-service-auth: header-service-secret',
    'client_secret=lowercase-secret-value',
    'Authorization: Bearer bearer-secret-value',
    'Cookie: sid=cookie-secret-value',
    '-----BEGIN PRIVATE KEY-----',
    'pem-secret-line',
    '-----END PRIVATE KEY-----',
    ...Array.from({ length: 20 }, (_, index) => `safe line ${index}`),
  ].join('\n');

  const result = spawnSync(process.execPath, [script], {
    input: fixture,
    encoding: 'utf8',
    env: {
      ...process.env,
      MAX_LOG_INPUT_BYTES: '4096',
      MAX_LOG_OUTPUT_BYTES: '240',
      MAX_LOG_LINES: '8',
    },
  });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /startup stage: database wait/);
  assert.match(result.stdout, /\[REDACTED/);
  for (const secret of secrets) {
    assert.doesNotMatch(result.stdout, new RegExp(secret));
  }
  assert.ok(Buffer.byteLength(result.stdout) <= 240);
  assert.ok(result.stdout.trimEnd().split('\n').length <= 8);
});

test('redacts an unterminated PEM block without echoing its payload', () => {
  const result = spawnSync(process.execPath, [script], {
    input: 'safe\n-----BEGIN PRIVATE KEY-----\nunterminated-sensitive-payload\n',
    encoding: 'utf8',
  });

  assert.equal(result.status, 0, result.stderr);
  assert.doesNotMatch(result.stdout, /unterminated-sensitive-payload/);
  assert.match(result.stdout, /\[REDACTED PEM\]/);
});

test('redacts an incomplete JSON secret when input truncation splits its value', () => {
  const result = spawnSync(process.execPath, [script], {
    input: 'safe\n{"token":"boundary-secret-value-without-a-closing-quote',
    encoding: 'utf8',
    env: {
      ...process.env,
      MAX_LOG_INPUT_BYTES: '36',
      MAX_LOG_OUTPUT_BYTES: '4096',
      MAX_LOG_LINES: '80',
    },
  });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /safe/);
  assert.match(result.stdout, /\[REDACTED\]/);
  assert.doesNotMatch(result.stdout, /boundary-secret/);
});
