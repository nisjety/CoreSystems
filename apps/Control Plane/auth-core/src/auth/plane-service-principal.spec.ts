import {
  chmodSync,
  mkdtempSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  authorizePlaneServicePrincipal,
  loadPlaneServicePrincipalRegistry,
} from './plane-service-principal';

const configured = JSON.stringify({
  'graph-worker': {
    credential: 'test-only-caller-key',
    audiences: ['data-plane'],
    orgIds: ['org-a'],
    scopes: ['graph:read'],
  },
});

describe('plane service-principal issuance policy', () => {
  it('binds caller, audience, tenant, scope, and reason from an allowlist', () => {
    expect(
      authorizePlaneServicePrincipal(configured, {
        serviceId: 'graph-worker',
        credential: 'test-only-caller-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['graph:read'],
        reason: 'serve graph retrieval',
      }),
    ).toEqual({
      serviceId: 'graph-worker',
      subject: 'service:graph-worker',
      orgId: 'org-a',
      scopes: ['graph:read'],
      reason: 'serve graph retrieval',
      zdr: true,
    });
  });

  it('derives persistent posture only from exact deployment-owned audience policy', () => {
    const persistenceAuthorized = JSON.stringify({
      'ingestion-writer': {
        credential: 'test-only-ingestion-key',
        audiences: ['data-plane'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['documents:write', 'org:data:write_all'],
        retentionByAudience: {
          'data-plane': 'persistent',
        },
      },
    });

    expect(
      authorizePlaneServicePrincipal(persistenceAuthorized, {
        serviceId: 'ingestion-writer',
        credential: 'test-only-ingestion-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['documents:write', 'org:data:write_all'],
        reason: 'persist approved ingestion fixture',
      }),
    ).toMatchObject({ zdr: false, orgId: 'org-a' });

    const callerDowngrade = {
      serviceId: 'graph-worker',
      credential: 'test-only-caller-key',
      audience: 'data-plane',
      orgId: 'org-a',
      requestedScopes: ['graph:read'],
      reason: 'attempt persistence downgrade',
      zdr: false,
    };
    expect(
      authorizePlaneServicePrincipal(configured, callerDowngrade),
    ).toMatchObject({ zdr: true, orgId: 'org-a' });
  });

  it.each([
    { orgId: 'org-b' },
    { requestedScopes: ['data:search:rebuild:global'] },
    { audience: 'model-plane' },
    { credential: 'wrong-test-key' },
    { reason: '' },
  ])('rejects an unbounded or ambiguous request: %o', (override) => {
    expect(() =>
      authorizePlaneServicePrincipal(configured, {
        serviceId: 'graph-worker',
        credential: 'test-only-caller-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['graph:read'],
        reason: 'serve graph retrieval',
        ...override,
      }),
    ).toThrow();
  });

  it('allows an explicitly deployment-authorized cross-tenant worker', () => {
    const crossTenant = JSON.stringify({
      'retrieval-engine': {
        credential: 'test-only-retrieval-key',
        audiences: ['control-policy'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['data:authorization:decide'],
        scopesByAudience: {
          'control-policy': ['data:authorization:decide'],
        },
      },
    });

    expect(
      authorizePlaneServicePrincipal(crossTenant, {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'control-policy',
        orgId: 'org-created-after-deploy',
        requestedScopes: ['data:authorization:decide'],
        reason: 'authorize retrieval request',
      }),
    ).toMatchObject({
      serviceId: 'retrieval-engine',
      orgId: 'org-created-after-deploy',
      scopes: ['data:authorization:decide'],
    });
  });

  it('allows the Space binding scope only for an explicitly registered service principal', () => {
    const binderRegistry = JSON.stringify({
      'application-space-binder': {
        credential: 'test-only-space-binder-key',
        audiences: ['data-plane'],
        orgIds: ['org-a'],
        scopes: ['data:space-binding:write'],
        scopesByAudience: {
          'data-plane': ['data:space-binding:write'],
        },
      },
    });

    expect(
      authorizePlaneServicePrincipal(binderRegistry, {
        serviceId: 'application-space-binder',
        credential: 'test-only-space-binder-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['data:space-binding:write'],
        reason: 'provision approved Space retrieval binding',
      }),
    ).toMatchObject({
      subject: 'service:application-space-binder',
      scopes: ['data:space-binding:write'],
    });

    expect(() =>
      authorizePlaneServicePrincipal(binderRegistry, {
        serviceId: 'application-space-binder',
        credential: 'test-only-space-binder-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['data:admin'],
        reason: 'attempt broader Data authority',
      }),
    ).toThrow();
  });

  it('selects exact scopes by audience for retrieval and graph inference', () => {
    const multiAudience = JSON.stringify({
      'retrieval-engine': {
        credential: 'test-only-retrieval-key',
        audiences: ['control-policy', 'inference-core'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['data:authorization:decide', 'inference:invoke'],
        scopesByAudience: {
          'control-policy': ['data:authorization:decide'],
          'inference-core': ['inference:invoke'],
        },
      },
      'graph-index': {
        credential: 'test-only-graph-index-key',
        audiences: ['inference-core'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['inference:invoke'],
        scopesByAudience: {
          'inference-core': ['inference:invoke'],
        },
      },
    });

    expect(
      authorizePlaneServicePrincipal(multiAudience, {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'control-policy',
        orgId: 'org-a',
        requestedScopes: ['data:authorization:decide'],
        reason: 'authorize retrieval request',
      }).scopes,
    ).toEqual(['data:authorization:decide']);
    expect(
      authorizePlaneServicePrincipal(multiAudience, {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'inference-core',
        orgId: 'org-a',
        requestedScopes: ['inference:invoke'],
        reason: 'embed retrieval query',
      }).scopes,
    ).toEqual(['inference:invoke']);
    expect(
      authorizePlaneServicePrincipal(multiAudience, {
        serviceId: 'graph-index',
        credential: 'test-only-graph-index-key',
        audience: 'inference-core',
        orgId: 'org-a',
        requestedScopes: ['inference:invoke'],
        reason: 'extract graph entities',
      }).scopes,
    ).toEqual(['inference:invoke']);

    for (const request of [
      {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'control-policy',
        requestedScopes: ['inference:invoke'],
      },
      {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'inference-core',
        requestedScopes: ['data:authorization:decide'],
      },
      {
        serviceId: 'graph-index',
        credential: 'test-only-graph-index-key',
        audience: 'inference-core',
        requestedScopes: ['data:authorization:decide'],
      },
    ]) {
      expect(() =>
        authorizePlaneServicePrincipal(multiAudience, {
          ...request,
          orgId: 'org-a',
          reason: 'attempt cross-audience authority',
        }),
      ).toThrow();
    }
  });

  it.each([
    {
      scopesByAudience: {
        'control-policy': ['data:authorization:decide'],
      },
    },
    {
      scopesByAudience: {
        'control-policy': ['data:authorization:decide'],
        'inference-core': ['inference:invoke'],
        unknown: ['inference:invoke'],
      },
    },
    {
      scopesByAudience: {
        'control-policy': ['data:authorization:decide'],
        'inference-core': ['inference:admin'],
      },
    },
  ])('rejects malformed per-audience scope mappings: %o', (override) => {
    const malformed = JSON.stringify({
      'retrieval-engine': {
        credential: 'test-only-retrieval-key',
        audiences: ['control-policy', 'inference-core'],
        orgIds: [],
        allowAnyOrg: true,
        scopes: ['data:authorization:decide', 'inference:invoke'],
        ...override,
      },
    });

    expect(() =>
      authorizePlaneServicePrincipal(malformed, {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'control-policy',
        orgId: 'org-a',
        requestedScopes: ['data:authorization:decide'],
        reason: 'authorize retrieval request',
      }),
    ).toThrow();
  });

  it('rejects malformed cross-tenant authorization policy', () => {
    const malformed = JSON.stringify({
      'retrieval-engine': {
        credential: 'test-only-retrieval-key',
        audiences: ['control-policy'],
        orgIds: [],
        allowAnyOrg: 'yes',
        scopes: ['data:authorization:decide'],
      },
    });

    expect(() =>
      authorizePlaneServicePrincipal(malformed, {
        serviceId: 'retrieval-engine',
        credential: 'test-only-retrieval-key',
        audience: 'control-policy',
        orgId: 'org-a',
        requestedScopes: ['data:authorization:decide'],
        reason: 'authorize retrieval request',
      }),
    ).toThrow();
  });

  it.each([
    { allowPersistentData: true },
    {
      retentionByAudience: {
        'data-plane': 'persistent',
        unknown: 'persistent',
      },
    },
    { retentionByAudience: {} },
    { retentionByAudience: { 'data-plane': 'sometimes' } },
  ])('rejects ambiguous retention policy: %o', (override) => {
    const malformed = JSON.stringify({
      'graph-worker': {
        credential: 'test-only-caller-key',
        audiences: ['data-plane'],
        orgIds: ['org-a'],
        scopes: ['graph:read'],
        ...override,
      },
    });

    expect(() =>
      authorizePlaneServicePrincipal(malformed, {
        serviceId: 'graph-worker',
        credential: 'test-only-caller-key',
        audience: 'data-plane',
        orgId: 'org-a',
        requestedScopes: ['graph:read'],
        reason: 'serve graph retrieval',
      }),
    ).toThrow();
  });
});

describe('plane service-principal registry loading', () => {
  let directory: string;
  let registryFile: string;

  beforeEach(() => {
    directory = mkdtempSync(join(tmpdir(), 'plane-service-principals-'));
    registryFile = join(directory, 'registry.json');
    writeFileSync(registryFile, configured, { mode: 0o600 });
  });

  afterEach(() => {
    rmSync(directory, { recursive: true, force: true });
  });

  it('loads a private absolute registry file and preserves development JSON compatibility', () => {
    expect(
      loadPlaneServicePrincipalRegistry({
        NODE_ENV: 'development',
        PLANE_SERVICE_PRINCIPALS_FILE: registryFile,
      }),
    ).toBe(configured);
    expect(
      loadPlaneServicePrincipalRegistry({
        NODE_ENV: 'development',
        PLANE_SERVICE_PRINCIPALS_JSON: configured,
      }),
    ).toBe(configured);
  });

  it.each(['production', 'staging', undefined])(
    'rejects unbound dynamic tenants from a %s file-backed registry',
    (runtime) => {
      const dynamicRegistry = JSON.stringify({
        'retrieval-engine': {
          credential: 'test-only-retrieval-key',
          audiences: ['inference-core'],
          orgIds: [],
          allowAnyOrg: true,
          scopes: ['inference:invoke'],
        },
      });
      writeFileSync(registryFile, dynamicRegistry, { mode: 0o600 });

      expect(() =>
        loadPlaneServicePrincipalRegistry({
          NODE_ENV: runtime,
          PLANE_SERVICE_PRINCIPALS_FILE: registryFile,
        }),
      ).toThrow(/fixed organization allowlists/);
    },
  );

  it('requires the file-backed registry outside development and tests', () => {
    expect(() =>
      loadPlaneServicePrincipalRegistry({
        NODE_ENV: 'production',
        PLANE_SERVICE_PRINCIPALS_JSON: configured,
      }),
    ).toThrow(/FILE is required/);
    expect(() =>
      loadPlaneServicePrincipalRegistry({
        NODE_ENV: 'staging',
        PLANE_SERVICE_PRINCIPALS_JSON: configured,
      }),
    ).toThrow(/FILE is required/);
  });

  it('rejects ambiguous file and environment registries', () => {
    expect(() =>
      loadPlaneServicePrincipalRegistry({
        PLANE_SERVICE_PRINCIPALS_FILE: registryFile,
        PLANE_SERVICE_PRINCIPALS_JSON: configured,
      }),
    ).toThrow(/both configured/);
  });

  it.each([
    ['relative path', () => 'registry.json', /normalized absolute path/],
    [
      'symbolic link',
      () => {
        const link = join(directory, 'registry-link.json');
        symlinkSync(registryFile, link);
        return link;
      },
      /private regular file/,
    ],
    [
      'oversized file',
      () => {
        const oversized = join(directory, 'oversized.json');
        writeFileSync(oversized, Buffer.alloc(1024 * 1024 + 1), {
          mode: 0o600,
        });
        return oversized;
      },
      /too large/,
    ],
    [
      'unreadable file',
      () => {
        const unreadable = join(directory, 'unreadable.json');
        writeFileSync(unreadable, configured, { mode: 0o600 });
        chmodSync(unreadable, 0o000);
        return unreadable;
      },
      /private regular file/,
    ],
  ])('rejects an unsafe %s', (_name, fileFactory, expectedError) => {
    expect(() =>
      loadPlaneServicePrincipalRegistry({
        PLANE_SERVICE_PRINCIPALS_FILE: fileFactory(),
      }),
    ).toThrow(expectedError);
  });

  it('rejects a registry file readable by group or other users', () => {
    chmodSync(registryFile, 0o644);

    expect(() =>
      loadPlaneServicePrincipalRegistry({
        PLANE_SERVICE_PRINCIPALS_FILE: registryFile,
      }),
    ).toThrow(/private/);
  });
});
