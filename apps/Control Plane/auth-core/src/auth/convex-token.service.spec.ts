import { ConvexTokenService } from './convex-token.service';
import { generateKeyPairSync } from 'crypto';
import { mkdtempSync, rmSync, writeFileSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';

const keyEnvNames = [
  'CONVEX_AUTH_PRIVATE_KEY_FILE',
  'CONVEX_AUTH_PUBLIC_KEY_FILE',
  'CONVEX_AUTH_PRIVATE_KEY_PEM',
  'CONVEX_AUTH_PUBLIC_KEY_PEM',
] as const;

describe('ConvexTokenService production key and principal posture', () => {
  const originalEnv = { ...process.env };

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it('refuses production startup without a stable complete signing keypair', () => {
    process.env.NODE_ENV = 'production';
    for (const name of keyEnvNames) delete process.env[name];

    expect(() => new ConvexTokenService()).toThrow(
      /stable RS256 signing keypair/i,
    );
  });

  it('refuses production startup when the configured public key does not match the private key', () => {
    const directory = mkdtempSync(join(tmpdir(), 'auth-keypair-test-'));
    const first = generateKeyPairSync('rsa', { modulusLength: 2048 });
    const second = generateKeyPairSync('rsa', { modulusLength: 2048 });
    const privatePath = join(directory, 'private.pem');
    const publicPath = join(directory, 'public.pem');

    try {
      writeFileSync(
        privatePath,
        first.privateKey.export({ type: 'pkcs8', format: 'pem' }),
      );
      writeFileSync(
        publicPath,
        second.publicKey.export({ type: 'spki', format: 'pem' }),
      );
      process.env.NODE_ENV = 'production';
      process.env.CONVEX_AUTH_PRIVATE_KEY_FILE = privatePath;
      process.env.CONVEX_AUTH_PUBLIC_KEY_FILE = publicPath;
      delete process.env.CONVEX_AUTH_PRIVATE_KEY_PEM;
      delete process.env.CONVEX_AUTH_PUBLIC_KEY_PEM;

      expect(() => new ConvexTokenService()).toThrow(/keypair does not match/i);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  it.each([
    {
      name: 'an EC keypair',
      generate: () => generateKeyPairSync('ec', { namedCurve: 'P-256' }),
    },
    {
      name: 'a weak RSA keypair',
      generate: () => generateKeyPairSync('rsa', { modulusLength: 1024 }),
    },
  ])('refuses production startup with $name', ({ generate }) => {
    const directory = mkdtempSync(join(tmpdir(), 'auth-key-strength-test-'));
    const keyPair = generate();
    const privatePath = join(directory, 'private.pem');
    const publicPath = join(directory, 'public.pem');

    try {
      writeFileSync(
        privatePath,
        keyPair.privateKey.export({ type: 'pkcs8', format: 'pem' }),
      );
      writeFileSync(
        publicPath,
        keyPair.publicKey.export({ type: 'spki', format: 'pem' }),
      );
      process.env.NODE_ENV = 'production';
      process.env.CONVEX_AUTH_PRIVATE_KEY_FILE = privatePath;
      process.env.CONVEX_AUTH_PUBLIC_KEY_FILE = publicPath;
      delete process.env.CONVEX_AUTH_PRIVATE_KEY_PEM;
      delete process.env.CONVEX_AUTH_PUBLIC_KEY_PEM;

      expect(() => new ConvexTokenService()).toThrow(/RSA.*2048/i);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  it('fails closed when a configured production key file is unreadable', () => {
    process.env.NODE_ENV = 'production';
    process.env.CONVEX_AUTH_PRIVATE_KEY_FILE = join(
      tmpdir(),
      'missing-auth-private-key.pem',
    );
    process.env.CONVEX_AUTH_PUBLIC_KEY_FILE = join(
      tmpdir(),
      'missing-auth-public-key.pem',
    );
    delete process.env.CONVEX_AUTH_PRIVATE_KEY_PEM;
    delete process.env.CONVEX_AUTH_PUBLIC_KEY_PEM;

    expect(() => new ConvexTokenService()).toThrow(/key file is not readable/i);
  });

  it('stamps an unambiguous bounded service principal into plane tokens', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();
    const bundle = service.issuePlaneToken('data-plane', {
      userId: 'service:graph-worker',
      orgId: 'org-a',
      scopes: ['graph:read'],
      principalType: 'service',
      serviceId: 'graph-worker',
      reason: 'serve graph retrieval',
    });
    const payload = JSON.parse(
      Buffer.from(bundle.token.split('.')[1], 'base64url').toString('utf8'),
    ) as Record<string, unknown>;

    expect(payload).toMatchObject({
      sub: 'service:graph-worker',
      org_id: 'org-a',
      principal_type: 'service',
      service_id: 'service:graph-worker',
      scopes: ['graph:read'],
    });
    expect(payload).not.toHaveProperty('user_id');
  });

  it('accepts non-ZDR posture only through the service-policy authority marker', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();
    const bundle = service.issuePlaneToken('data-plane', {
      userId: 'service:ingestion-writer',
      orgId: 'org-a',
      scopes: ['documents:write'],
      principalType: 'service',
      serviceId: 'ingestion-writer',
      reason: 'persist approved ingestion',
      retentionPosture: {
        zdr: false,
        authority: 'service-principal-policy',
      },
    });
    const payload = JSON.parse(
      Buffer.from(bundle.token.split('.')[1], 'base64url').toString('utf8'),
    ) as Record<string, unknown>;

    expect(payload.zdr).toBe(false);
  });

  it('reserves the control-policy audience for service-principal issuance', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();

    expect(service.isKnownPlaneAudience('control-policy')).toBe(true);
    expect(service.isInteractivePlaneAudience('control-policy')).toBe(false);
    expect(service.isInteractivePlaneAudience('data-plane')).toBe(true);
    expect(service.isInteractivePlaneAudience('session-core')).toBe(true);

    const sessionBundle = service.issuePlaneToken('session-core', {
      userId: 'user-a',
      orgId: 'org-a',
    });
    const sessionPayload = JSON.parse(
      Buffer.from(sessionBundle.token.split('.')[1], 'base64url').toString(
        'utf8',
      ),
    ) as Record<string, unknown>;
    expect(sessionPayload).toMatchObject({
      aud: 'session-core',
      sub: 'user-a',
      user_id: 'user-a',
      org_id: 'org-a',
      principal_type: 'user',
    });
  });

  it('recognizes the closed Model service audience contract without accepting typos', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();

    for (const audience of [
      'model-gateway',
      'session-core',
      'inference-core',
      'execution-core',
      'cost-core',
      'capability-core',
      'letta-bridge',
      'browser-broker',
      'sandbox-manager',
      'bridge-core',
    ]) {
      expect(service.isKnownAuthAudience(audience)).toBe(true);
    }
    expect(service.isKnownAuthAudience('model-plane')).toBe(false);
    expect(service.isKnownAuthAudience('inference')).toBe(false);
  });

  it.each([
    'session-core',
    'inference-core',
    'execution-core',
    'cost-core',
    'capability-core',
    'letta-bridge',
    'browser-broker',
    'sandbox-manager',
    'bridge-core',
  ] as const)(
    'issues an exact %s audience with mandatory issuer ZDR that caller input cannot downgrade',
    (audience) => {
      process.env.NODE_ENV = 'test';
      for (const name of keyEnvNames) delete process.env[name];
      const service = new ConvexTokenService();
      const bundle = service.issuePlaneToken(audience, {
        userId: 'user-a',
        orgId: 'org-a',
        zdr: false,
      } as never);
      const payload = JSON.parse(
        Buffer.from(bundle.token.split('.')[1], 'base64url').toString('utf8'),
      ) as Record<string, unknown>;

      expect(bundle.audience).toBe(audience);
      expect(payload).toMatchObject({ aud: audience, zdr: true });
    },
  );

  it('caps policy-caller credentials at a five-minute lifetime', () => {
    process.env.NODE_ENV = 'test';
    process.env.PLANE_TOKEN_TTL_CONTROL_POLICY_SECONDS = '3600';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();
    const bundle = service.issuePlaneToken('control-policy', {
      userId: 'service:retrieval-engine',
      orgId: 'org-a',
      scopes: ['data:authorization:decide'],
      principalType: 'service',
      serviceId: 'retrieval-engine',
      reason: 'authorize retrieval request',
    });

    expect(bundle.expiresInSeconds).toBe(300);
  });

  it('fails closed for empty, malformed, and undecodable policy tokens', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();

    expect(() => service.verifyPlaneServiceToken('control-policy', '')).toThrow(
      /not verifiable/i,
    );
    expect(() =>
      service.verifyPlaneServiceToken('control-policy', 'a.b.*'),
    ).toThrow(/malformed/i);
    expect(() =>
      service.verifyPlaneServiceToken('control-policy', 'a.b.c'),
    ).toThrow(/verification failed/i);
  });

  it('does not issue ambiguous plane principal shapes', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();

    expect(() =>
      service.issuePlaneToken('control-policy', {
        userId: 'service:retrieval-engine',
        orgId: 'org-a',
        principalType: 'service',
      }),
    ).toThrow(/ambiguous/i);
    expect(() =>
      service.issuePlaneToken('control-policy', {
        userId: 'user-a',
        orgId: 'org-a',
        principalType: 'user',
        serviceId: 'retrieval-engine',
      }),
    ).toThrow(/user token/i);
  });

  it('does not issue ambiguous Model principal shapes', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();

    expect(() =>
      service.issueModelPlaneToken({
        userId: 'service:model-worker',
        orgId: 'org-a',
        principalType: 'service',
      }),
    ).toThrow(/ambiguous/i);
    expect(() =>
      service.issueModelPlaneToken({
        userId: 'user-a',
        orgId: 'org-a',
        principalType: 'user',
        serviceId: 'model-worker',
      }),
    ).toThrow(/user token/i);
  });

  it('round-trips only canonical signed service principals', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();
    const bundle = service.issuePlaneToken('control-policy', {
      userId: 'service:retrieval-engine',
      orgId: 'org-a',
      scopes: ['data:authorization:decide', 'data:authorization:decide'],
      principalType: 'service',
      serviceId: 'retrieval-engine',
      reason: 'authorize retrieval request',
    });

    expect(
      service.verifyPlaneServiceToken('control-policy', bundle.token),
    ).toEqual({
      subject: 'service:retrieval-engine',
      serviceId: 'service:retrieval-engine',
      orgId: 'org-a',
      scopes: ['data:authorization:decide'],
      reason: 'authorize retrieval request',
    });
    expect(() =>
      service.verifyPlaneServiceToken('data-plane', bundle.token),
    ).toThrow(/claims are invalid/i);
  });

  it('rejects a signed user token at a service-principal verification boundary', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();
    const bundle = service.issuePlaneToken('control-policy', {
      userId: 'user-a',
      orgId: 'org-a',
      scopes: ['data:authorization:decide'],
    });

    expect(() =>
      service.verifyPlaneServiceToken('control-policy', bundle.token),
    ).toThrow(/claims are invalid/i);
  });

  it('rejects unknown audiences and incomplete stable key configuration', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();
    expect(() =>
      service.issuePlaneToken('unknown' as never, {
        userId: 'user-a',
        orgId: 'org-a',
      }),
    ).toThrow(/unknown plane audience/i);

    process.env.CONVEX_AUTH_PRIVATE_KEY_PEM = 'test-incomplete-key-material';
    expect(() => new ConvexTokenService()).toThrow(/complete stable RS256/i);
  });

  it('publishes only the trusted RS256 verification key', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();

    expect(service.getJwks()).toMatchObject({
      keys: [{ alg: 'RS256', use: 'sig', kid: 'convex-auth-rs256' }],
    });
  });

  it('uses the same non-impersonating service shape for Model tokens', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();
    const bundle = service.issueModelPlaneToken({
      userId: 'service:model-worker',
      orgId: 'org-a',
      scopes: ['runs:submit'],
      principalType: 'service',
      serviceId: 'model-worker',
      reason: 'submit scheduled evaluation',
    });
    const payload = JSON.parse(
      Buffer.from(bundle.token.split('.')[1], 'base64url').toString('utf8'),
    ) as Record<string, unknown>;
    expect(payload).toMatchObject({
      sub: 'service:model-worker',
      service_id: 'service:model-worker',
      principal_type: 'service',
      org_id: 'org-a',
    });
    expect(payload).not.toHaveProperty('user_id');
  });

  it('keeps Model users ZDR and accepts non-ZDR Model services only through the policy marker', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    const service = new ConvexTokenService();

    const userClaimsWithoutPosture = {
      userId: 'user-a',
      orgId: 'org-a',
    } as unknown as Parameters<ConvexTokenService['issueModelPlaneToken']>[0];
    const userBundle = service.issueModelPlaneToken(userClaimsWithoutPosture);
    const serviceClaims = {
      userId: 'service:model-worker',
      orgId: 'org-a',
      principalType: 'service',
      serviceId: 'model-worker',
      reason: 'scheduled evaluation',
      retentionPosture: {
        zdr: false,
        authority: 'service-principal-policy',
      },
    } as unknown as Parameters<ConvexTokenService['issueModelPlaneToken']>[0];
    const serviceBundle = service.issueModelPlaneToken(serviceClaims);

    const userPayload = JSON.parse(
      Buffer.from(userBundle.token.split('.')[1], 'base64url').toString('utf8'),
    ) as Record<string, unknown>;
    const servicePayload = JSON.parse(
      Buffer.from(serviceBundle.token.split('.')[1], 'base64url').toString(
        'utf8',
      ),
    ) as Record<string, unknown>;
    expect(userPayload.zdr).toBe(true);
    expect(servicePayload.zdr).toBe(false);
  });

  it('uses an exact Control-Plane organization policy for interactive persistence', () => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    process.env.AUTH_CORE_INTERACTIVE_RETENTION_POLICY_JSON = JSON.stringify({
      version: 1,
      organizations: {
        'org-approved': {
          posture: 'persistent',
          policyEvidenceSha256:
            'a3b6c4a80f6bf3442d360bfb5e2aee5d1de7b28fd2049c1cad5a6d79e3b84fe3',
        },
      },
    });
    const service = new ConvexTokenService();
    const approved = service.issuePlaneToken('inference-core', {
      userId: 'user-a',
      orgId: 'org-approved',
    });
    const unlisted = service.issuePlaneToken('inference-core', {
      userId: 'user-b',
      orgId: 'org-unlisted',
    });

    const decodePayload = (token: string) =>
      JSON.parse(
        Buffer.from(token.split('.')[1], 'base64url').toString('utf8'),
      ) as Record<string, unknown>;
    expect(decodePayload(approved.token).zdr).toBe(false);
    expect(decodePayload(unlisted.token).zdr).toBe(true);
  });
});
