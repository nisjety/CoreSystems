import {
  resetInteractiveRetentionCacheForTests,
  resolveInteractiveRetentionPosture,
} from './interactive-retention-policy';

type MockResponse = {
  ok: boolean;
  status: number;
  json: () => Promise<unknown>;
};

function jsonResponse(status: number, body: unknown): MockResponse {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  };
}

/**
 * Routes a mocked `fetch` by whether the requested URL is the organization
 * record endpoint or the entitlements endpoint, mirroring how
 * `resolveInteractiveRetentionPosture` calls both in parallel.
 */
function mockOrgCore({
  organization,
  entitlements,
  organizationStatus = 200,
  entitlementsStatus = 200,
}: {
  organization?: unknown;
  entitlements?: unknown;
  organizationStatus?: number;
  entitlementsStatus?: number;
}): jest.SpyInstance {
  return jest.spyOn(global, 'fetch').mockImplementation((input: unknown) => {
    const url = String(input);
    if (url.endsWith('/entitlements')) {
      return Promise.resolve(
        jsonResponse(entitlementsStatus, entitlements) as never,
      );
    }
    return Promise.resolve(
      jsonResponse(organizationStatus, organization) as never,
    );
  });
}

function entitlementsBody(enabled: boolean) {
  return {
    organization_id: 'org-a',
    entitlements: [
      {
        key: 'feature.zero_data_retention',
        enabled,
        updated_at: '2026-07-21T00:00:00.000Z',
      },
    ],
  };
}

describe('resolveInteractiveRetentionPosture', () => {
  beforeEach(() => {
    resetInteractiveRetentionCacheForTests();
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it('defaults to zdr:false (normal retention) — ZDR is opt-in, not the standard', async () => {
    const fetchSpy = mockOrgCore({
      organization: { id: 'org-new', metadata: {} },
      entitlements: entitlementsBody(false),
    });

    await expect(
      resolveInteractiveRetentionPosture('org-new'),
    ).resolves.toEqual({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
    expect(fetchSpy).toHaveBeenCalled();
  });

  it('resolves zdr:true only when stored intent is true AND the plan entitlement holds', async () => {
    mockOrgCore({
      organization: {
        id: 'org-qualified',
        metadata: { interactiveRetention: { zdr: true } },
      },
      entitlements: entitlementsBody(true),
    });

    await expect(
      resolveInteractiveRetentionPosture('org-qualified'),
    ).resolves.toEqual({
      zdr: true,
      authority: 'interactive-org-retention-policy',
    });
  });

  it('fails closed to zdr:false when stored intent is true but the plan entitlement does not hold (defense-in-depth catches a bypass attempt)', async () => {
    mockOrgCore({
      organization: {
        id: 'org-downgraded-plan',
        metadata: { interactiveRetention: { zdr: true } },
      },
      // Org-core's own server-computed entitlement says the current plan is
      // NOT entitled — e.g. the org was downgraded after toggling ZDR on, or
      // the stored intent was set some other way. The independent
      // entitlement check must still block it.
      entitlements: entitlementsBody(false),
    });

    await expect(
      resolveInteractiveRetentionPosture('org-downgraded-plan'),
    ).resolves.toEqual({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
  });

  it('resolves zdr:false for a brand-new org with no interactiveRetention metadata at all', async () => {
    mockOrgCore({
      organization: { id: 'org-brand-new', metadata: {} },
      entitlements: entitlementsBody(true),
    });

    await expect(
      resolveInteractiveRetentionPosture('org-brand-new'),
    ).resolves.toEqual({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
  });

  it('resolves zdr:false when metadata is entirely absent from the organization response', async () => {
    mockOrgCore({
      organization: { id: 'org-no-metadata' },
      entitlements: entitlementsBody(true),
    });

    await expect(
      resolveInteractiveRetentionPosture('org-no-metadata'),
    ).resolves.toEqual({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
  });

  it('fails closed to zdr:false — never open — when org-core is unreachable, and does not throw (login still succeeds)', async () => {
    jest.spyOn(global, 'fetch').mockRejectedValue(new Error('ECONNREFUSED'));

    await expect(
      resolveInteractiveRetentionPosture('org-outage'),
    ).resolves.toEqual({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
  });

  it('fails closed to zdr:false when org-core responds with a timeout-style abort', async () => {
    jest
      .spyOn(global, 'fetch')
      .mockRejectedValue(
        new DOMException('The operation was aborted', 'AbortError'),
      );

    await expect(
      resolveInteractiveRetentionPosture('org-timeout'),
    ).resolves.toEqual({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
  });

  it('fails closed to zdr:false on a non-2xx org-core response', async () => {
    mockOrgCore({
      organization: { error: 'organization not found' },
      organizationStatus: 404,
      entitlements: entitlementsBody(true),
    });

    await expect(
      resolveInteractiveRetentionPosture('org-not-found'),
    ).resolves.toEqual({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
  });

  it('fails closed to zdr:false for a syntactically invalid organization id without calling org-core', async () => {
    const fetchSpy = jest.spyOn(global, 'fetch');

    await expect(
      resolveInteractiveRetentionPosture('../not an org id'),
    ).resolves.toEqual({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it('caches a resolved posture for repeat lookups within the TTL window instead of calling org-core again', async () => {
    const fetchSpy = mockOrgCore({
      organization: {
        id: 'org-cached',
        metadata: { interactiveRetention: { zdr: true } },
      },
      entitlements: entitlementsBody(true),
    });

    await resolveInteractiveRetentionPosture('org-cached');
    const callsAfterFirstLookup = fetchSpy.mock.calls.length;
    await resolveInteractiveRetentionPosture('org-cached');

    expect(fetchSpy.mock.calls.length).toBe(callsAfterFirstLookup);
  });
});
