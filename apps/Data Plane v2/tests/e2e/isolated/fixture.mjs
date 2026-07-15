import { createPrivateKey, createSign, generateKeyPairSync } from 'node:crypto';
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const command = process.argv[2];
const directory = process.argv[3];
if (!command || !directory) throw new Error('usage: fixture.mjs prepare|mint DIR');

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
  for (const producer of ['documents', 'index', 'embedding', 'wiki', 'retrieval']) {
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
