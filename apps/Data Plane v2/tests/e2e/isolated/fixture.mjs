import { createPrivateKey, createSign, generateKeyPairSync } from 'node:crypto';
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const command = process.argv[2];
const directory = process.argv[3];
if (!command || !directory) throw new Error('usage: fixture.mjs prepare|mint|mint-service DIR');

if (command === 'prepare') {
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const { privateKey, publicKey } = generateRsaPair();
  const privatePem = privateKey.export({ format: 'pem', type: 'pkcs8' });
  const publicPem = publicKey.export({ format: 'pem', type: 'spki' });
  const jwk = publicKey.export({ format: 'jwk' });
  const kid = 'isolated-mvp-key';
  writeFileSync(join(directory, 'private.pem'), privatePem, { mode: 0o600 });
  writeFileSync(join(directory, 'public.pem'), publicPem, { mode: 0o600 });
  writeFileSync(join(directory, 'jwks.json'), JSON.stringify({ keys: [{ ...jwk, kid, alg: 'RS256', use: 'sig' }] }), { mode: 0o600 });
  for (const producer of ['documents', 'index', 'embedding', 'wiki', 'retrieval', 'graph']) {
    const pair = generateRsaPair();
    writeFileSync(
      join(directory, `${producer}-events.pem`),
      pair.privateKey.export({ format: 'pem', type: 'pkcs8' }),
      { mode: 0o600 },
    );
    writeFileSync(
      join(directory, `${producer}-events.pub`),
      pair.publicKey.export({ format: 'pem', type: 'spki' }),
      { mode: 0o600 },
    );
  }
  chmodSync(directory, 0o700);
  process.exit(0);
}

if (command === 'mint') {
  const now = Math.floor(Date.now() / 1000);
  const issuer = required('JWT_ISSUER');
  const orgId = required('JWT_ORG_ID');
  const userId = required('JWT_USER_ID');
  const scopes = required('JWT_SCOPES').split(',').map((scope) => scope.trim()).filter(Boolean);
  const zdrText = (process.env.JWT_ZDR ?? 'true').trim().toLowerCase();
  if (zdrText !== 'true' && zdrText !== 'false') throw new Error('JWT_ZDR must be true or false');
  const zdr = zdrText === 'true';
  const header = encode({ alg: 'RS256', typ: 'JWT', kid: 'isolated-mvp-key' });
  const payload = encode({
    iss: issuer, aud: 'data-plane', sub: userId, user_id: userId,
    principal_type: 'user', org_id: orgId, scopes, zdr,
    iat: now, nbf: now - 1, exp: now + 900,
  });
  const signingInput = `${header}.${payload}`;
  const signer = createSign('RSA-SHA256');
  signer.update(signingInput);
  signer.end();
  const key = createPrivateKey(readFileSync(join(directory, 'private.pem')));
  process.stdout.write(`${signingInput}.${signer.sign(key).toString('base64url')}`);
  process.exit(0);
}

if (command === 'mint-service') {
  // QB-T08: the quarry-edge service bearer of the isolated matrix's service rows. The claims are the ones
  // the data plane's principal classifier reads: sub and service_id name the same service:<name>, and
  // principal_type is service. The scopes are exactly the ones asked for. It is signed with the key the
  // data plane trusts for the user bearers, so the route table and the scope check are the data plane's
  // own. It is NOT an Auth Core mint: no isolated stack here has org-core to answer the directory-bound
  // mint (docs/web-rerank-v1.md, section 12).
  // zdr follows JWT_ZDR (default true, as for mint). A signed zdr claim skips every rerank call, so the
  // paid call in the runbook's step 3 needs a bearer minted with JWT_ZDR=false.
  const now = Math.floor(Date.now() / 1000);
  const issuer = required('JWT_ISSUER');
  const orgId = required('JWT_ORG_ID');
  const scopes = required('JWT_SCOPES').split(',').map((scope) => scope.trim()).filter(Boolean);
  const zdrText = (process.env.JWT_ZDR ?? 'true').trim().toLowerCase();
  if (zdrText !== 'true' && zdrText !== 'false') throw new Error('JWT_ZDR must be true or false');
  const subject = 'service:quarry-edge';
  const header = encode({ alg: 'RS256', typ: 'JWT', kid: 'isolated-mvp-key' });
  const payload = encode({
    iss: issuer, aud: 'data-plane', sub: subject, service_id: subject,
    principal_type: 'service', org_id: orgId, scopes, zdr: zdrText === 'true',
    iat: now, nbf: now - 1, exp: now + 900,
  });
  const signingInput = `${header}.${payload}`;
  const signer = createSign('RSA-SHA256');
  signer.update(signingInput);
  signer.end();
  const key = createPrivateKey(readFileSync(join(directory, 'private.pem')));
  process.stdout.write(`${signingInput}.${signer.sign(key).toString('base64url')}`);
  process.exit(0);
}

throw new Error(`unknown command: ${command}`);

function encode(value) {
  return Buffer.from(JSON.stringify(value)).toString('base64url');
}

function required(name) {
  const value = (process.env[name] ?? '').trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function generateRsaPair() {
  return generateKeyPairSync('rsa', { modulusLength: 2048 });
}
