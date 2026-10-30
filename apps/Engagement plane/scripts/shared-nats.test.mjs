import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const hub = fileURLToPath(new URL('../../Frontend Plane/verevonv3/nats.shared.conf', import.meta.url));
test('shared hub keeps separate leaf principals and excludes private subjects', () => {
  const config = readFileSync(hub, 'utf8');
  for (const plane of ['studio', 'control', 'ingestion']) assert.ok(config.includes(`user: "${plane}-leaf"`));
  for (const subject of ['$JS.>', '$SYS.>', '_INBOX.>', '_VEREVON.>', 'verevon.gdpr.>', 'verevon.audit.>']) {
    assert.ok(config.includes(`"${subject}"`));
  }
  assert.match(config, /domain: VEREVON/);
  assert.doesNotMatch(config, /allow:\s*\[[^\]]*verevon\.application\.social/, 'draft content stays on Studio broker');
  assert.ok(!/password:\s*"[a-z0-9]{32,}"/.test(config));
});
test('real pinned NATS server validates synthetic hub configuration', () => {
  const args = ['run', '--rm', '--network', 'none', '--mount', `type=bind,src=${hub},dst=/etc/nats/shared.conf,readonly`];
  for (const name of ['VEREVON_NATS_TOKEN', 'NATS_STUDIO_LEAF_PASSWORD', 'NATS_CONTROL_LEAF_PASSWORD', 'NATS_INGESTION_LEAF_PASSWORD']) {
    args.push('-e', `${name}=synthetic-non-secret-fixture`);
  }
  args.push('nats:2.14.2-alpine@sha256:952d157e28d5394a211229bd57a7b37ff9f184e58e2c8486a08fa909fd254e32', '-t', '-c', '/etc/nats/shared.conf');
  const result = spawnSync('docker', args, { encoding: 'utf8', timeout: 30_000 });
  assert.equal(result.status, 0, result.stderr);
});
test('hub health check uses the monitor IPv4 loopback address', () => {
  const compose = readFileSync(new URL('../../Frontend Plane/verevonv3/docker-compose.yml', import.meta.url), 'utf8');
  const broker = compose.slice(compose.indexOf('  nats:'), compose.indexOf('\n  gateway:'));
  assert.ok(broker.includes('http://127.0.0.1:8222/healthz'));
});
