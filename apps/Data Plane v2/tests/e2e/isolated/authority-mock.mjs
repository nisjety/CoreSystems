import {
  createHash,
  createHmac,
  createSign,
  createVerify,
  timingSafeEqual,
} from 'node:crypto';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';

const jwks = JSON.parse(readFileSync('/fixtures/jwks.json', 'utf8'));
const policyPrivateKey = readFileSync('/fixtures/private.pem');
const policyPublicKey = readFileSync('/fixtures/public.pem');
const policyKey = required('CONTROL_POLICY_SERVICE_API_KEY');
const authIssuer = required('AUTH_CORE_ISSUER');
const policyAudience = 'control-policy';
const policyScope = 'data:authorization:decide';
const policyServiceId = 'retrieval-engine';
const policySubject = `service:${policyServiceId}`;
const policyReason = 'authorize retrieval request';
const serviceTokens = new Map([
  ['retrieval-engine', required('USER_CORE_RETRIEVAL_TOKEN')],
  ['documents-api', required('USER_CORE_DOCUMENTS_TOKEN')],
]);

function required(name) {
  const value = (process.env[name] ?? '').trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function json(response, status, body) {
  const encoded = JSON.stringify(body);
  response.writeHead(status, {
    'content-type': 'application/json',
    'content-length': Buffer.byteLength(encoded),
  });
  response.end(encoded);
}

async function bodyOf(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > 1_048_576) throw new Error('request body too large');
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

function equal(left, right) {
  const a = Buffer.from(left);
  const b = Buffer.from(right);
  return a.length === b.length && timingSafeEqual(a, b);
}

function encode(value) {
  return Buffer.from(JSON.stringify(value)).toString('base64url');
}

function signPolicyToken(orgId) {
  const now = Math.floor(Date.now() / 1000);
  const expiresInSeconds = 60;
  const header = encode({ alg: 'RS256', typ: 'JWT', kid: 'isolated-mvp-key' });
  const payload = encode({
    iss: authIssuer,
    aud: policyAudience,
    sub: policySubject,
    iat: now,
    nbf: now - 1,
    exp: now + expiresInSeconds,
    org_id: orgId,
    principal_type: 'service',
    service_id: policySubject,
    scopes: [policyScope],
    reason: policyReason,
  });
  const signingInput = `${header}.${payload}`;
  const signer = createSign('RSA-SHA256');
  signer.update(signingInput);
  signer.end();
  return {
    token: `${signingInput}.${signer.sign(policyPrivateKey).toString('base64url')}`,
    expiresAt: new Date((now + expiresInSeconds) * 1000).toISOString(),
    expiresInSeconds,
    issuer: authIssuer,
    audience: policyAudience,
  };
}

function verifyPolicyBearer(authorization) {
  if (!authorization.startsWith('Bearer ')) return null;
  const token = authorization.slice('Bearer '.length);
  const parts = token.split('.');
  if (parts.length !== 3) return null;
  try {
    const header = JSON.parse(Buffer.from(parts[0], 'base64url').toString('utf8'));
    const claims = JSON.parse(Buffer.from(parts[1], 'base64url').toString('utf8'));
    const verifier = createVerify('RSA-SHA256');
    verifier.update(`${parts[0]}.${parts[1]}`);
    verifier.end();
    const now = Math.floor(Date.now() / 1000);
    if (
      !verifier.verify(policyPublicKey, Buffer.from(parts[2], 'base64url')) ||
      header.alg !== 'RS256' || header.typ !== 'JWT' ||
      claims.iss !== authIssuer || claims.aud !== policyAudience ||
      claims.sub !== policySubject || claims.service_id !== policySubject ||
      claims.principal_type !== 'service' || claims.reason !== policyReason ||
      !Array.isArray(claims.scopes) || claims.scopes.length !== 1 ||
      claims.scopes[0] !== policyScope || !claims.org_id ||
      !Number.isInteger(claims.nbf) || !Number.isInteger(claims.exp) ||
      claims.nbf > now || claims.exp <= now || claims.exp - now > 60
    ) return null;
    return claims;
  } catch {
    return null;
  }
}

function verifyDelegation(request, body) {
  const header = (name) => (request.headers[name] ?? '').trim();
  const principal = header('x-service-id');
  const token = serviceTokens.get(principal);
  if (!token || !equal(token, header('x-service-token'))) return false;
  const timestamp = header('x-delegation-timestamp');
  const timestampMs = Date.parse(timestamp);
  if (!Number.isFinite(timestampMs) || Math.abs(Date.now() - timestampMs) > 30_000) return false;
  const digest = createHash('sha256').update(body).digest('base64url');
  if (!equal(digest, header('x-delegation-body-sha256'))) return false;
  const values = [
    'v2', principal, 'user-core', timestamp, request.method, request.url,
    header('x-user-id'), header('x-org-id'), header('x-delegation-operation'),
    header('x-delegation-resource-type'), header('x-delegation-resource-id'),
    header('x-delegation-reason'), header('x-delegation-zdr'),
    header('x-delegation-nonce'), digest,
  ];
  if (
    header('x-delegation-version') !== 'v2' ||
    header('x-delegation-operation') !== 'authz:visible' ||
    header('x-delegation-resource-type') !== 'document' ||
    header('x-delegation-zdr') !== 'true' ||
    header('x-user-id') === '' || header('x-org-id') === '' ||
    header('x-delegation-reason').length < 3 ||
    header('x-delegation-nonce').length < 16
  ) return false;
  const signature = createHmac('sha256', token).update(values.join('\n')).digest('base64url');
  return equal(signature, header('x-delegation-signature'));
}

const authServer = createServer(async (request, response) => {
  try {
    const body = await bodyOf(request);
    if (request.method === 'GET' && request.url === '/api/convex-auth/jwks') {
      return json(response, 200, jwks);
    }
    if (request.method === 'POST' && request.url === '/api/control-policy/internal-token') {
      if (
        request.headers['x-service-id'] !== policyServiceId ||
        !equal(policyKey, request.headers['x-service-api-key'] ?? '')
      ) return json(response, 403, { error: 'forbidden' });
      const input = JSON.parse(body.toString('utf8'));
      if (
        typeof input.orgId !== 'string' || input.orgId.length === 0 ||
        input.reason !== policyReason || !Array.isArray(input.scopes) ||
        input.scopes.length !== 1 || input.scopes[0] !== policyScope
      ) return json(response, 400, { error: 'invalid token request' });
      return json(response, 200, signPolicyToken(input.orgId));
    }
    if (request.method === 'POST' && request.url === '/api/v1/internal/authorization/data-plane/decision') {
      const principal = verifyPolicyBearer(request.headers.authorization ?? '');
      if (!principal) return json(response, 401, { error: 'unauthorized' });
      const input = JSON.parse(body.toString('utf8'));
      if (
        !input.orgId || !input.userId || input.action !== 'data.read' ||
        principal.org_id !== input.orgId
      ) {
        return json(response, 400, { error: 'invalid decision request' });
      }
      return json(response, 200, {
        version: 'v1',
        allowed: true,
        role: 'member',
        permissions: ['data:read'],
        membershipRevision: 'isolated-revision',
        reason: 'member',
      });
    }
    if (request.method === 'GET' && request.url === '/healthz') return json(response, 200, { ok: true });
    return json(response, 404, { error: 'not found' });
  } catch {
    return json(response, 400, { error: 'invalid request' });
  }
});

const userServer = createServer(async (request, response) => {
  try {
    const body = await bodyOf(request);
    if (request.method === 'GET' && request.url === '/healthz') return json(response, 200, { ok: true });
    if (request.method === 'GET' && request.url.startsWith('/api/v1/internal/authz/visible?')) {
      if (!verifyDelegation(request, body)) return json(response, 401, { error: 'unauthorized' });
      return json(response, 200, { ids: [], all_org: false });
    }
    return json(response, 404, { error: 'not found' });
  } catch {
    return json(response, 400, { error: 'invalid request' });
  }
});

authServer.listen(3011, '0.0.0.0');
userServer.listen(3012, '0.0.0.0');
