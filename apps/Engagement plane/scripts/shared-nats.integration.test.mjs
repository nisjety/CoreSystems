import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { randomBytes } from 'node:crypto';
import { createConnection } from 'node:net';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

// Opt-in integration test: four disposable brokers, synthetic credentials and
// messages only. Actual plane ACLs and leaf configs remain unchanged; a
// STANDARD-only fixture operator is added to copied configs so the test can
// observe denied subjects and create source-local JetStream streams.
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const apps = dirname(root);
const frontend = join(apps, 'Frontend Plane/verevonv3');
const image = 'nats:2.14.2-alpine@sha256:952d157e28d5394a211229bd57a7b37ff9f184e58e2c8486a08fa909fd254e32';
const enabled = process.env.RUN_SHARED_NATS_INTEGRATION === '1';
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

function docker(args, { allowFailure = false } = {}) {
  const result = spawnSync('docker', args, { encoding: 'utf8', timeout: 20_000 });
  if (!allowFailure) assert.equal(result.status, 0, `Docker fixture operation failed: ${result.stderr}`);
  return (args[0] === 'logs' ? result.stdout + result.stderr : result.stdout).trim();
}

function operator(config, password) {
  return config.replace(/users:\s*\[/, match => `${match}\n{ user: "fixture-operator", password: "${password}", allowed_connection_types: [STANDARD], permissions: { publish: ">", subscribe: ">" } },`);
}

class Client {
  constructor(socket) {
    this.socket = socket;
    this.buffer = Buffer.alloc(0);
    this.messages = [];
    this.errors = [];
    this.sid = 0;
    this.pongs = [];
    socket.on('error', error => this.errors.push(error.message));
    socket.on('data', bytes => { this.buffer = Buffer.concat([this.buffer, bytes]); this.parse(); });
  }
  parse() {
    while (true) {
      const end = this.buffer.indexOf('\r\n');
      if (end < 0) return;
      const line = this.buffer.subarray(0, end).toString();
      if (line.startsWith('MSG ') || line.startsWith('HMSG ')) {
        const fields = line.split(' ');
        const length = Number(fields.at(-1));
        if (this.buffer.length < end + 2 + length + 2) return;
        const headerLength = line.startsWith('HMSG ') ? Number(fields.at(-2)) : 0;
        const hasReply = fields.length === (line.startsWith('HMSG ') ? 6 : 5);
        this.messages.push({ subject: fields[1], sid: fields[2], reply: hasReply ? fields[3] : undefined, body: this.buffer.subarray(end + 2 + headerLength, end + 2 + length).toString() });
        this.buffer = this.buffer.subarray(end + 2 + length + 2);
        continue;
      }
      this.buffer = this.buffer.subarray(end + 2);
      if (line === 'PING') this.socket.write('PONG\r\n');
      if (line === 'PONG') this.pongs.shift()?.();
      if (line.startsWith('-ERR')) this.errors.push(line);
    }
  }
  static async connect(port, credentials) {
    const socket = createConnection({ host: '127.0.0.1', port });
    const client = new Client(socket);
    await new Promise((resolve, reject) => { socket.once('connect', resolve); socket.once('error', reject); });
    socket.write(`CONNECT ${JSON.stringify({ verbose: false, pedantic: true, headers: true, protocol: 1, ...credentials })}\r\n`);
    await client.flush();
    assert.equal(client.errors.length, 0, `Fixture authentication failed: ${client.errors.join('; ')}`);
    return client;
  }
  flush() {
    return Promise.race([
      new Promise(resolve => { this.pongs.push(resolve); this.socket.write('PING\r\n'); }),
      delay(1500).then(() => { throw new Error(`NATS protocol flush timed out: ${this.errors.join('; ')}`); }),
    ]);
  }
  subscribe(subject) {
    const sid = String(++this.sid);
    this.socket.write(`SUB ${subject} ${sid}\r\n`);
    return sid;
  }
  publish(subject, body, reply) {
    this.socket.write(`PUB ${subject}${reply ? ` ${reply}` : ''} ${Buffer.byteLength(body)}\r\n${body}\r\n`);
  }
  async request(subject, payload, prefix = '_INBOX.fixture') {
    const inbox = `${prefix}.${randomBytes(8).toString('hex')}`;
    const sid = this.subscribe(inbox);
    await this.flush();
    this.publish(subject, JSON.stringify(payload), inbox);
    const response = await this.waitFor(message => message.sid === sid);
    return JSON.parse(response.body);
  }
  async waitFor(predicate, timeout = 3000) {
    const deadline = Date.now() + timeout;
    while (Date.now() < deadline) {
      const message = this.messages.find(predicate);
      if (message) return message;
      await delay(25);
    }
    throw new Error('Expected synthetic event was not delivered');
  }
  close() { this.socket.destroy(); }
}

test('real shared hub federates only permitted metadata; private JetStream stays local', { skip: !enabled, timeout: 100_000 }, async () => {
  const directory = mkdtempSync(join(tmpdir(), 'engagement-nats-integration-'));
  const suffix = randomBytes(6).toString('hex');
  const network = `engagement-nats-test-${suffix}`;
  const names = [];
  const clients = [];
  const password = randomBytes(32).toString('hex');
  const leafPasswords = Object.fromEntries(['studio', 'control', 'ingestion'].map(name => [name, randomBytes(32).toString('hex')]));
  const hubConfig = join(directory, 'hub.conf');
  writeFileSync(hubConfig, operator(readFileSync(join(frontend, 'nats.shared.conf'), 'utf8'), password), { mode: 0o600 });
  try {
    // Docker Desktop does not expose published loopback ports on --internal
    // networks. A dedicated bridge is used solely for these four fixtures;
    // no fixture performs any external request.
    docker(['network', 'create', network]);
    const start = (plane, args) => {
      const name = `${network}-${plane}`;
      names.push(name);
      docker(['run', '-d', '--name', name, '--network', network, '-p', '127.0.0.1::4222', ...args]);
      return name;
    };
    const hub = start('hub', ['--network-alias', 'verevon-nats', '--mount', `type=bind,src=${hubConfig},dst=/etc/nats/shared.conf,readonly`,
      '-e', `VEREVON_NATS_TOKEN=${JSON.stringify(password)}`, ...Object.entries(leafPasswords).flatMap(([name, value]) => ['-e', `NATS_${name.toUpperCase()}_LEAF_PASSWORD=${JSON.stringify(value)}`]), image, '-c', '/etc/nats/shared.conf']);
    const hubDeadline = Date.now() + 10_000;
    while (Date.now() < hubDeadline) {
      const health = docker(['exec', hub, 'wget', '-qO-', 'http://127.0.0.1:8222/healthz'], { allowFailure: true });
      if (health.includes('ok')) break;
      await delay(100);
    }
    const definitions = [
      ['studio', root, 'nats.conf', 'STUDIO'],
      ['control', join(apps, 'Control Plane'), 'control-shared-nats.conf', 'CONTROL_SHARED'],
      ['ingestion', join(apps, 'Ingestion Plane'), 'nats.local.conf', 'INGESTION'],
    ];
    const brokers = { hub };
    for (const [plane, planeRoot, baseName, domain] of definitions) {
      const base = readFileSync(join(planeRoot, baseName), 'utf8');
      const config = join(directory, `${plane}.conf`);
      // Ingestion's singleton token is already a local privileged fixture client.
      writeFileSync(config, plane === 'ingestion' ? base : operator(base, password), { mode: 0o600 });
      const secret = join(directory, `${plane}.password`);
      writeFileSync(secret, `${leafPasswords[plane]}\n`, { mode: 0o600 });
      const variables = [...new Set([...base.matchAll(/\$([A-Z][A-Z0-9_]*)/g)].map(match => match[1]))];
      brokers[plane] = start(plane, ['--entrypoint', '/bin/sh',
        '--mount', `type=bind,src=${config},dst=/etc/nats/nats.conf,readonly`,
        '--mount', `type=bind,src=${join(planeRoot, 'nats.shared-leaf.conf')},dst=/etc/nats/shared-leaf.conf,readonly`,
        '--mount', `type=bind,src=${join(root, 'scripts/shared-nats-leaf-entrypoint.sh')},dst=/opt/entrypoint.sh,readonly`,
        '--mount', `type=bind,src=${secret},dst=/run/secrets/shared_nats_leaf_password,readonly`,
        '-e', `NATS_LEAF_DOMAIN=${domain}`, '-e', `INGESTION_NATS_TOKEN=${password}`,
        ...variables.filter(name => name !== 'INGESTION_NATS_CONFIG_TOKEN').flatMap(name => ['-e', `${name}=${JSON.stringify(password)}`]), image, '/opt/entrypoint.sh']);
    }
    const deadline = Date.now() + 45_000;
    let connected = false;
    while (Date.now() < deadline) {
      const info = docker(['exec', hub, 'wget', '-qO-', 'http://127.0.0.1:8222/leafz'], { allowFailure: true });
      try { if (JSON.parse(info).leafnodes === 3) { connected = true; break; } } catch { /* startup only */ }
      await delay(200);
    }
    assert.ok(connected, 'All three synthetic leaf connections must become ready; ' + Object.entries(brokers).map(([plane, name]) => `${plane}: ${docker(['logs', '--tail', '6', name], { allowFailure: true })}`).join('; '));
    const connections = {};
    const ports = {};
    for (const [plane, name] of Object.entries(brokers)) {
      ports[plane] = Number(docker(['port', name, '4222/tcp']).split(':').at(-1));
      const client = await Client.connect(ports[plane], plane === 'ingestion' ? { auth_token: password } : { user: 'fixture-operator', pass: password });
      clients.push(client);
      connections[plane] = client;
      client.subscribe('>');
      await client.flush();
    }
    // Allow interest propagation, not sleeps between every message.
    await delay(400);
    const publicSubjects = { hub: 'verevon.frontend.events.fixture', studio: 'verevon.studio.events.fixture', control: 'controlplane.org.updated', ingestion: 'quarry.events.fixture' };
    for (const [source, subject] of Object.entries(publicSubjects)) {
      const marker = `synthetic-public-${source}-${suffix}`;
      connections[source].publish(subject, marker);
      await Promise.all(Object.values(connections).map(client => client.waitFor(message => message.body === marker)));
    }
    const frontendClient = await Client.connect(ports.hub, { user: 'verevon-events', pass: password });
    clients.push(frontendClient);
    const frontendMarker = `synthetic-scoped-frontend-${suffix}`;
    frontendClient.publish(publicSubjects.hub, frontendMarker);
    await Promise.all(Object.values(connections).map(client => client.waitFor(message => message.body === frontendMarker)));
    const frontendSpoof = `synthetic-scoped-frontend-spoof-${suffix}`;
    frontendClient.publish(publicSubjects.control, frontendSpoof);
    frontendClient.subscribe('$JS.API.>');
    await frontendClient.flush();
    assert.ok(frontendClient.errors.some(error => error.includes('Permissions Violation')), 'Scoped frontend identity must reject authority spoofing/private subscriptions');
    await delay(100);
    assert.ok(Object.values(connections).every(client => !client.messages.some(message => message.body === frontendSpoof)));
    // Use unrestricted local fixture operators to prove each leaf boundary,
    // not merely a subscriber's own denial, blocks sensitive subjects.
    const forbidden = ['verevon.gdpr.fixture', 'verevon.audit.fixture', 'verevon.application.social.fixture', '_INBOX.fixture.forbidden', '$JS.API.fixture'];
    const negativeMarkers = [];
    for (const [source, client] of Object.entries(connections)) {
      for (const subject of forbidden) {
        const marker = `synthetic-private-${source}-${negativeMarkers.length}-${suffix}`;
        negativeMarkers.push({ source, marker });
        client.publish(subject, marker);
      }
    }
    for (const [source, subject] of [['studio', publicSubjects.control], ['control', publicSubjects.ingestion], ['ingestion', publicSubjects.studio]]) {
      const marker = `synthetic-spoof-${source}-${suffix}`;
      negativeMarkers.push({ source, marker });
      connections[source].publish(subject, marker);
    }
    await Promise.all(Object.values(connections).map(client => client.flush()));
    await delay(500);
    for (const { source, marker } of negativeMarkers) {
      for (const [destination, client] of Object.entries(connections)) {
        if (destination !== source) assert.ok(!client.messages.some(message => message.body === marker), `${source} must not leak/spoof a subject into ${destination}`);
      }
    }
    // A real local JetStream create/publish roundtrip must still receive its
    // private inbox acknowledgement. No stream may appear on another broker.
    const stream = `FIXTURE_${suffix.toUpperCase()}`;
    const created = await connections.studio.request('$JS.STUDIO.API.STREAM.CREATE.' + stream, { name: stream, subjects: ['verevon.studio.events.fixture.stream'], storage: 'memory' });
    assert.equal(created.config?.name, stream);
    const ack = await connections.studio.request('verevon.studio.events.fixture.stream', { synthetic: true });
    assert.equal(ack.stream, stream);
    assert.equal(ack.seq, 1);
    for (const [plane, domain] of [['hub', 'VEREVON'], ['control', 'CONTROL_SHARED'], ['ingestion', 'INGESTION']]) {
      const info = await connections[plane].request(`$JS.${domain}.API.STREAM.INFO.${stream}`, {});
      assert.equal(info.error?.code, 404, `${plane} must not own the Studio fixture stream`);
    }
    // Regression: adding a JetStream domain can change generated ACK reply
    // subjects. Exercise the *actual* gateway GDPR identity's narrow existing
    // ACL, not the unrestricted fixture operator, through pull + explicit ACK.
    const controlStream = 'CONTROLPLANE';
    const durable = 'verevon-gateway-gdpr-erasure-v1';
    const controlCreated = await connections.control.request(`$JS.API.STREAM.CREATE.${controlStream}`, {
      name: controlStream, subjects: ['verevon.gdpr.fixture'], storage: 'memory',
    });
    assert.equal(controlCreated.config?.name, controlStream);
    const consumer = await connections.control.request(`$JS.API.CONSUMER.DURABLE.CREATE.${controlStream}.${durable}`, {
      stream_name: controlStream, config: { durable_name: durable, ack_policy: 'explicit', filter_subject: 'verevon.gdpr.fixture' },
    });
    assert.equal(consumer.name, durable);
    const gdpr = await Client.connect(ports.control, { user: 'verevon-gateway-gdpr', pass: password });
    clients.push(gdpr);
    const deliveryInbox = `_INBOX.VEREVON_GATEWAY_GDPR.${suffix}`;
    const deliverySid = gdpr.subscribe(deliveryInbox);
    await gdpr.flush();
    await connections.control.request('verevon.gdpr.fixture', { synthetic: true });
    gdpr.publish(`$JS.API.CONSUMER.MSG.NEXT.${controlStream}.${durable}`, JSON.stringify({ batch: 1, expires: 1_000_000_000 }), deliveryInbox);
    const delivery = await gdpr.waitFor(message => message.sid === deliverySid && Boolean(message.reply)).catch(error => {
      throw new Error(`${error.message}; scoped GDPR errors=${JSON.stringify(gdpr.errors)} messages=${JSON.stringify(gdpr.messages)}`);
    });
    assert.ok(delivery.reply.startsWith('$JS.ACK.'), 'Consumer delivery must include its source-private ACK subject');
    gdpr.publish(delivery.reply, '+ACK');
    await gdpr.flush();
    assert.deepEqual(gdpr.errors, [], `Scoped GDPR ACK must remain authorized: ${delivery.reply}`);
    const afterAck = await connections.control.request(`$JS.API.CONSUMER.INFO.${controlStream}.${durable}`, {});
    assert.equal(afterAck.num_ack_pending, 0, 'Domain-bearing Control consumer ACK must be accepted by its scoped ACL');
    // Leaf-only identities cannot be reused as ordinary clients, and a legacy
    // root token cannot bypass the new scoped username/password policy.
    for (const credentials of [{ user: 'studio-leaf', pass: leafPasswords.studio }, { auth_token: password }]) {
      await assert.rejects(Client.connect(ports.hub, credentials));
    }
  } finally {
    for (const client of clients) client.close();
    for (const name of names.reverse()) docker(['rm', '-f', name], { allowFailure: true });
    docker(['network', 'rm', network], { allowFailure: true });
    rmSync(directory, { recursive: true, force: true });
  }
});
