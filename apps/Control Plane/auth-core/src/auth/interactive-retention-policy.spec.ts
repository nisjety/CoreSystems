import { Logger, ServiceUnavailableException } from '@nestjs/common';

import {
  RETENTION_POSTURE_FRESH_TTL_MS,
  RETENTION_STALE_NON_ZDR_MAX_AGE_MS,
  RETENTION_STALE_ZDR_MAX_AGE_MS,
  RetentionPostureUnavailableError,
  resetInteractiveRetentionCacheForTests,
  resolveInteractiveRetentionPosture,
} from './interactive-retention-policy';

const SERVICE_TOKEN = 'unit-spec-service-token-0123456789abcdef0123';
const ZDR = {
  zdr: true,
  authority: 'interactive-org-retention-policy',
} as const;
const NOT_ZDR = {
  zdr: false,
  authority: 'interactive-org-retention-policy',
} as const;

type Posture = { zdrIntent: boolean; entitled: boolean };
type FakeReply = {
  status?: number;
  body?: unknown;
  error?: Error;
  delayed?: Promise<void>;
};

function reply(status: number, body: unknown) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  };
}

/**
 * An org-core double that enforces the parts of org-core's contract the
 * resolver depends on, the way the real service does: no service credential is
 * a 401, and the posture route is the only path Auth Core's scope reaches (any
 * other path is a 403). The previous spec mocked fetch and enforced nothing,
 * which is how a resolver that sent no credentials passed its tests while
 * answering "not ZDR" for every org in production. The real middleware is
 * exercised by the cross-language contract (interactive-retention-policy
 * .org-core-contract.spec.ts and org-core's retention_posture_contract_test.go).
 */
function fakeOrgCore(
  respond: (orgId: string) => FakeReply | Promise<FakeReply>,
): jest.SpyInstance {
  return jest
    .spyOn(global, 'fetch')
    .mockImplementation(async (input: unknown, init?: RequestInit) => {
      const url = new URL(String(input));
      const headers = new Headers(init?.headers);
      if (
        headers.get('x-service-id') !== 'auth-core' ||
        headers.get('x-service-token') !== SERVICE_TOKEN
      ) {
        return reply(401, {
          error: 'service authentication required',
        }) as never;
      }
      const match = /^\/internal\/orgs\/([^/]+)\/retention-posture$/.exec(
        url.pathname,
      );
      if (!match || (init?.method ?? 'GET') !== 'GET') {
        return reply(403, { error: 'service scope denied' }) as never;
      }
      const result = await respond(decodeURIComponent(match[1]));
      if (result.delayed) await result.delayed;
      if (result.error) throw result.error;
      return reply(result.status ?? 200, result.body) as never;
    });
}

function servesPostures(postures: Record<string, Posture>): jest.SpyInstance {
  return fakeOrgCore((orgId) =>
    postures[orgId]
      ? { body: postures[orgId] }
      : { status: 404, body: { error: 'organization not found' } },
  );
}

function requestedUrl(spy: jest.SpyInstance, call: number): string {
  const calls = spy.mock.calls as unknown[][];
  return String(calls[call][0]);
}

const ZDR_ORG: Posture = { zdrIntent: true, entitled: true };
const PLAIN_ORG: Posture = { zdrIntent: false, entitled: true };
const LAPSED_ORG: Posture = { zdrIntent: true, entitled: false };

describe('resolveInteractiveRetentionPosture', () => {
  const originalEnv = { ...process.env };
  let errorLog: jest.SpyInstance;
  let warnLog: jest.SpyInstance;

  beforeEach(() => {
    jest.useFakeTimers({ now: new Date('2026-10-03T10:00:00.000Z') });
    resetInteractiveRetentionCacheForTests();
    process.env.ORG_SERVICE_URL = 'http://org-core.spec.test:8080';
    delete process.env.ORG_CORE_URL;
    process.env.ORG_CORE_SERVICE_TOKEN = SERVICE_TOKEN;
    errorLog = jest.spyOn(Logger.prototype, 'error').mockImplementation();
    warnLog = jest.spyOn(Logger.prototype, 'warn').mockImplementation();
  });

  afterEach(() => {
    jest.useRealTimers();
    jest.restoreAllMocks();
    process.env = { ...originalEnv };
  });

  describe('the org-core request', () => {
    it('authenticates as auth-core and reads one narrow endpoint, never the unauthenticated organization and entitlement reads', async () => {
      const fetchSpy = servesPostures({ 'org-a': ZDR_ORG });

      await resolveInteractiveRetentionPosture('org-a');

      expect(fetchSpy).toHaveBeenCalledTimes(1);
      const [url, init] = fetchSpy.mock.calls[0] as [string, RequestInit];
      expect(new URL(url).origin).toBe('http://org-core.spec.test:8080');
      expect(new URL(url).pathname).toBe(
        '/internal/orgs/org-a/retention-posture',
      );
      expect(init.method).toBe('GET');
      const headers = new Headers(init.headers);
      expect(headers.get('x-service-id')).toBe('auth-core');
      expect(headers.get('x-service-token')).toBe(SERVICE_TOKEN);
      expect(init.signal).toBeInstanceOf(AbortSignal);
    });

    it('url-encodes the organization id and prefers ORG_SERVICE_URL, then ORG_CORE_URL, then the in-cluster default', async () => {
      const fetchSpy = servesPostures({});
      await resolveInteractiveRetentionPosture('org.a:b-1').catch(() => null);
      expect(requestedUrl(fetchSpy, 0)).toBe(
        'http://org-core.spec.test:8080/internal/orgs/org.a%3Ab-1/retention-posture',
      );

      resetInteractiveRetentionCacheForTests();
      delete process.env.ORG_SERVICE_URL;
      process.env.ORG_CORE_URL = 'http://from-org-core-url:9/';
      await resolveInteractiveRetentionPosture('org-a').catch(() => null);
      expect(requestedUrl(fetchSpy, 1)).toBe(
        'http://from-org-core-url:9/internal/orgs/org-a/retention-posture',
      );

      resetInteractiveRetentionCacheForTests();
      delete process.env.ORG_CORE_URL;
      await resolveInteractiveRetentionPosture('org-a').catch(() => null);
      expect(requestedUrl(fetchSpy, 2)).toBe(
        'http://org-core:8080/internal/orgs/org-a/retention-posture',
      );
    });

    it('is not accepted by an org-core that requires service authentication when the credential is wrong', async () => {
      const fetchSpy = servesPostures({ 'org-a': ZDR_ORG });
      process.env.ORG_CORE_SERVICE_TOKEN = `${SERVICE_TOKEN}-rotated-wrongly`;

      await expect(
        resolveInteractiveRetentionPosture('org-a'),
      ).rejects.toBeInstanceOf(RetentionPostureUnavailableError);
      expect(fetchSpy).toHaveBeenCalledTimes(1);
    });
  });

  describe('resolving a posture', () => {
    it('resolves zdr:false for an org that did not turn ZDR on', async () => {
      servesPostures({ 'org-new': PLAIN_ORG });
      await expect(
        resolveInteractiveRetentionPosture('org-new'),
      ).resolves.toEqual(NOT_ZDR);
    });

    it('resolves zdr:true only when the stored intent AND the plan entitlement both hold', async () => {
      servesPostures({
        'org-zdr': ZDR_ORG,
        'org-no-intent': PLAIN_ORG,
        'org-no-plan': LAPSED_ORG,
        'org-neither': { zdrIntent: false, entitled: false },
      });

      await expect(
        resolveInteractiveRetentionPosture('org-zdr'),
      ).resolves.toEqual(ZDR);
      await expect(
        resolveInteractiveRetentionPosture('org-no-intent'),
      ).resolves.toEqual(NOT_ZDR);
      await expect(
        resolveInteractiveRetentionPosture('org-no-plan'),
      ).resolves.toEqual(NOT_ZDR);
      await expect(
        resolveInteractiveRetentionPosture('org-neither'),
      ).resolves.toEqual(NOT_ZDR);
    });

    it('says so when a plan lapse silently turns a ZDR org non-ZDR', async () => {
      servesPostures({ 'org-lapsed': LAPSED_ORG });

      await resolveInteractiveRetentionPosture('org-lapsed');

      expect(warnLog).toHaveBeenCalledWith(
        expect.stringContaining('plan_not_entitled'),
      );
    });

    it('treats an org org-core has no record of as not ZDR without caching that answer', async () => {
      const fetchSpy = servesPostures({});

      await expect(
        resolveInteractiveRetentionPosture('org-unknown'),
      ).resolves.toEqual(NOT_ZDR);
      await expect(
        resolveInteractiveRetentionPosture('org-unknown'),
      ).resolves.toEqual(NOT_ZDR);
      expect(fetchSpy).toHaveBeenCalledTimes(2);
    });

    it('keeps orgs apart', async () => {
      servesPostures({ 'org-a': ZDR_ORG, 'org-b': PLAIN_ORG });

      await expect(
        resolveInteractiveRetentionPosture('org-a'),
      ).resolves.toEqual(ZDR);
      await expect(
        resolveInteractiveRetentionPosture('org-b'),
      ).resolves.toEqual(NOT_ZDR);
    });

    it('rejects a syntactically invalid organization id without calling org-core, and never as a plain "not ZDR"', async () => {
      const fetchSpy = jest.spyOn(global, 'fetch');

      await expect(
        resolveInteractiveRetentionPosture('../not an org id'),
      ).rejects.toBeInstanceOf(RetentionPostureUnavailableError);
      expect(fetchSpy).not.toHaveBeenCalled();
    });

    it('does not put a malformed organization id into a log line', async () => {
      await resolveInteractiveRetentionPosture(
        'org-a\nERROR forged log line',
      ).catch(() => null);

      const logged = JSON.stringify(errorLog.mock.calls);
      expect(logged).toContain('invalid_organization_id');
      expect(logged).not.toContain('forged log line');
    });
  });

  describe('the cache', () => {
    it('serves a resolved posture for the fresh TTL without calling org-core again', async () => {
      const fetchSpy = servesPostures({ 'org-a': ZDR_ORG });

      await resolveInteractiveRetentionPosture('org-a');
      jest.advanceTimersByTime(RETENTION_POSTURE_FRESH_TTL_MS - 1);
      await resolveInteractiveRetentionPosture('org-a');

      expect(fetchSpy).toHaveBeenCalledTimes(1);
    });

    it('re-reads org-core once the TTL passes and follows a change in either direction', async () => {
      const postures: Record<string, Posture> = { 'org-a': PLAIN_ORG };
      const fetchSpy = servesPostures(postures);

      await expect(
        resolveInteractiveRetentionPosture('org-a'),
      ).resolves.toEqual(NOT_ZDR);
      postures['org-a'] = ZDR_ORG;
      jest.advanceTimersByTime(RETENTION_POSTURE_FRESH_TTL_MS);
      await expect(
        resolveInteractiveRetentionPosture('org-a'),
      ).resolves.toEqual(ZDR);
      postures['org-a'] = PLAIN_ORG;
      jest.advanceTimersByTime(RETENTION_POSTURE_FRESH_TTL_MS);
      await expect(
        resolveInteractiveRetentionPosture('org-a'),
      ).resolves.toEqual(NOT_ZDR);
      expect(fetchSpy).toHaveBeenCalledTimes(3);
    });

    it('shares one org-core read between concurrent mints for the same org', async () => {
      let release: () => void = () => undefined;
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const fetchSpy = fakeOrgCore(() => ({ body: ZDR_ORG, delayed: gate }));

      const first = resolveInteractiveRetentionPosture('org-a');
      const second = resolveInteractiveRetentionPosture('org-a');
      const other = resolveInteractiveRetentionPosture('org-b');
      release();

      await expect(Promise.all([first, second, other])).resolves.toEqual([
        ZDR,
        ZDR,
        ZDR,
      ]);
      expect(
        fetchSpy.mock.calls
          .map(([url]) => new URL(String(url)).pathname)
          .sort(),
      ).toEqual([
        '/internal/orgs/org-a/retention-posture',
        '/internal/orgs/org-b/retention-posture',
      ]);
    });
  });

  /**
   * The outage contract. A posture that cannot be read is never a posture:
   * the resolver serves what it last knew for a bounded time, and otherwise
   * refuses to mint (503 retention_posture_unavailable) instead of minting a
   * token that might retain a ZDR org's content.
   */
  describe('when org-core cannot answer', () => {
    const outages: Array<[string, FakeReply]> = [
      ['a 500', { status: 500, body: { error: 'failed' } }],
      ['a 503', { status: 503, body: { error: 'down' } }],
      ['a 401 (credential rejected)', { status: 401, body: {} }],
      ['a 403 (scope not granted)', { status: 403, body: {} }],
      [
        'a 404 that is not org-core\x27s own "organization not found"',
        { status: 404, body: '404 page not found' },
      ],
      [
        'a 404 with an unrelated JSON body',
        { status: 404, body: { message: 'nope' } },
      ],
      ['a refused connection', { error: new Error('ECONNREFUSED') }],
      [
        'a timeout',
        {
          error: new DOMException('The operation was aborted', 'TimeoutError'),
        },
      ],
      ['a malformed body', { body: { unexpected: true } }],
      ['non-boolean fields', { body: { zdrIntent: 'true', entitled: 1 } }],
      ['a null body', { body: null }],
      ['an array body', { body: [true, true] }],
    ];

    describe.each(outages)('%s', (_name, outage) => {
      it('keeps serving zdr:true for an org last known to be ZDR', async () => {
        const state: { down: boolean } = { down: false };
        fakeOrgCore(() => (state.down ? outage : { body: ZDR_ORG }));
        await expect(
          resolveInteractiveRetentionPosture('org-zdr'),
        ).resolves.toEqual(ZDR);

        state.down = true;
        jest.advanceTimersByTime(RETENTION_POSTURE_FRESH_TTL_MS);

        await expect(
          resolveInteractiveRetentionPosture('org-zdr'),
        ).resolves.toEqual(ZDR);
      });

      it('refuses to mint (503) for an org it has never resolved, and never answers "not ZDR"', async () => {
        fakeOrgCore(() => outage);

        const attempt = resolveInteractiveRetentionPosture('org-never-seen');

        await expect(attempt).rejects.toBeInstanceOf(
          RetentionPostureUnavailableError,
        );
        await expect(attempt).rejects.toBeInstanceOf(
          ServiceUnavailableException,
        );
        await expect(attempt).rejects.toMatchObject({
          response: {
            statusCode: 503,
            error: 'retention_posture_unavailable',
          },
        });
        expect(errorLog).toHaveBeenCalledWith(
          expect.stringContaining('retention_posture_unavailable'),
        );
      });

      it('does not cache the failure: the next mint asks org-core again and sees the recovery at once', async () => {
        const state: { down: boolean } = { down: true };
        const fetchSpy = fakeOrgCore(() =>
          state.down ? outage : { body: ZDR_ORG },
        );

        await expect(
          resolveInteractiveRetentionPosture('org-a'),
        ).rejects.toBeInstanceOf(RetentionPostureUnavailableError);
        state.down = false;

        // No clock advance: a cached failure would still be answering here.
        await expect(
          resolveInteractiveRetentionPosture('org-a'),
        ).resolves.toEqual(ZDR);
        expect(fetchSpy).toHaveBeenCalledTimes(2);
      });
    });

    it('logs which kind of outage it was (error name and transport code) without the error message, which can carry the URL', async () => {
      fakeOrgCore(() => ({
        error: Object.assign(
          new TypeError('fetch failed for http://org-core.internal:8080/x'),
          { cause: { code: 'ECONNREFUSED' } },
        ),
      }));

      await resolveInteractiveRetentionPosture('org-a').catch(() => null);

      const logged = JSON.stringify(errorLog.mock.calls);
      expect(logged).toContain('org_core_unreachable');
      expect(logged).toContain('TypeError/ECONNREFUSED');
      expect(logged).not.toContain('org-core.internal');
    });

    it('never leaks the service credential or the org-core response into logs or the client error', async () => {
      fakeOrgCore(() => ({ status: 500, body: { detail: 'secret-detail' } }));

      const error = await resolveInteractiveRetentionPosture('org-a').catch(
        (caught: unknown) => caught,
      );

      const surface = JSON.stringify([
        errorLog.mock.calls,
        warnLog.mock.calls,
        (error as RetentionPostureUnavailableError).getResponse(),
      ]);
      expect(surface).not.toContain(SERVICE_TOKEN);
      expect(surface).not.toContain('secret-detail');
    });

    it('keeps serving an unknown-then-known ZDR org through the whole stale window, then refuses rather than downgrade it', async () => {
      const state: { down: boolean } = { down: false };
      fakeOrgCore(() =>
        state.down ? { status: 500, body: {} } : { body: ZDR_ORG },
      );
      await resolveInteractiveRetentionPosture('org-zdr');
      state.down = true;

      jest.advanceTimersByTime(RETENTION_STALE_ZDR_MAX_AGE_MS - 1);
      await expect(
        resolveInteractiveRetentionPosture('org-zdr'),
      ).resolves.toEqual(ZDR);

      // The window is measured from the last successful read, not from the
      // last stale answer: serving stale never extends it.
      jest.advanceTimersByTime(2);
      await expect(
        resolveInteractiveRetentionPosture('org-zdr'),
      ).rejects.toBeInstanceOf(RetentionPostureUnavailableError);
    });

    it('serves a last-known non-ZDR org only for the shorter window, then refuses', async () => {
      const state: { down: boolean } = { down: false };
      fakeOrgCore(() =>
        state.down ? { status: 500, body: {} } : { body: PLAIN_ORG },
      );
      await resolveInteractiveRetentionPosture('org-plain');
      state.down = true;

      jest.advanceTimersByTime(RETENTION_STALE_NON_ZDR_MAX_AGE_MS - 1);
      await expect(
        resolveInteractiveRetentionPosture('org-plain'),
      ).resolves.toEqual(NOT_ZDR);

      jest.advanceTimersByTime(2);
      await expect(
        resolveInteractiveRetentionPosture('org-plain'),
      ).rejects.toBeInstanceOf(RetentionPostureUnavailableError);
    });

    it('pins the windows: changing one is a data-protection decision, and a stale "false" (the one that can leak) gets the short one', () => {
      expect(RETENTION_POSTURE_FRESH_TTL_MS).toBe(45 * 1000);
      expect(RETENTION_STALE_NON_ZDR_MAX_AGE_MS).toBe(15 * 60 * 1000);
      expect(RETENTION_STALE_ZDR_MAX_AGE_MS).toBe(24 * 60 * 60 * 1000);
      expect(RETENTION_STALE_NON_ZDR_MAX_AGE_MS).toBeGreaterThan(
        RETENTION_POSTURE_FRESH_TTL_MS,
      );
      expect(RETENTION_STALE_NON_ZDR_MAX_AGE_MS).toBeLessThan(
        RETENTION_STALE_ZDR_MAX_AGE_MS,
      );
    });

    it('retries org-core on every mint after the TTL while it serves a stale posture', async () => {
      const state: { down: boolean } = { down: false };
      const fetchSpy = fakeOrgCore(() =>
        state.down ? { status: 503, body: {} } : { body: ZDR_ORG },
      );
      await resolveInteractiveRetentionPosture('org-zdr');
      state.down = true;

      for (let attempt = 0; attempt < 3; attempt += 1) {
        jest.advanceTimersByTime(RETENTION_POSTURE_FRESH_TTL_MS);
        await expect(
          resolveInteractiveRetentionPosture('org-zdr'),
        ).resolves.toEqual(ZDR);
      }
      expect(fetchSpy).toHaveBeenCalledTimes(4);
      expect(warnLog).toHaveBeenCalledWith(
        expect.stringContaining('serving_stale_posture'),
      );
    });

    it('does not let a stale ZDR answer hide a recovery to a changed posture', async () => {
      const postures: { current: Posture | 'down' } = { current: ZDR_ORG };
      fakeOrgCore(() =>
        postures.current === 'down'
          ? { status: 500, body: {} }
          : { body: postures.current },
      );
      await resolveInteractiveRetentionPosture('org-a');
      postures.current = 'down';
      jest.advanceTimersByTime(RETENTION_POSTURE_FRESH_TTL_MS);
      await expect(
        resolveInteractiveRetentionPosture('org-a'),
      ).resolves.toEqual(ZDR);

      postures.current = PLAIN_ORG;

      await expect(
        resolveInteractiveRetentionPosture('org-a'),
      ).resolves.toEqual(NOT_ZDR);
    });

    it('does not downgrade a known ZDR org when org-core answers 404 for it', async () => {
      const state: { gone: boolean } = { gone: false };
      fakeOrgCore(() =>
        state.gone
          ? { status: 404, body: { error: 'organization not found' } }
          : { body: ZDR_ORG },
      );
      await resolveInteractiveRetentionPosture('org-zdr');
      state.gone = true;
      jest.advanceTimersByTime(RETENTION_POSTURE_FRESH_TTL_MS);

      await expect(
        resolveInteractiveRetentionPosture('org-zdr'),
      ).resolves.toEqual(ZDR);
      expect(warnLog).toHaveBeenCalledWith(
        expect.stringContaining('serving_stale_posture'),
      );
    });

    it('does not mix up orgs during an outage: org B has no history even though org A does', async () => {
      const state: { down: boolean } = { down: false };
      fakeOrgCore((orgId) =>
        state.down
          ? { status: 500, body: {} }
          : { body: orgId === 'org-a' ? ZDR_ORG : PLAIN_ORG },
      );
      await resolveInteractiveRetentionPosture('org-a');
      state.down = true;

      await expect(
        resolveInteractiveRetentionPosture('org-b'),
      ).rejects.toBeInstanceOf(RetentionPostureUnavailableError);
    });
  });

  describe('credential problems are loud, and are not mistaken for an answer', () => {
    it('logs a rejected credential at error level with its own reason, so the next 401 is not silent', async () => {
      fakeOrgCore(() => ({ status: 401, body: {} }));

      await resolveInteractiveRetentionPosture('org-a').catch(() => null);

      expect(errorLog).toHaveBeenCalledWith(
        expect.stringContaining('org_core_credential_rejected'),
      );
    });

    it.each([
      ['missing', undefined],
      ['empty', ''],
      ['too short', 'short'],
      ['a placeholder', 'placeholder-token-0123456789abcdef0123456789'],
    ])(
      'refuses to mint, without calling org-core, when the service token is %s',
      async (_label, value) => {
        const fetchSpy = servesPostures({ 'org-a': PLAIN_ORG });
        if (value === undefined) delete process.env.ORG_CORE_SERVICE_TOKEN;
        else process.env.ORG_CORE_SERVICE_TOKEN = value;

        await expect(
          resolveInteractiveRetentionPosture('org-a'),
        ).rejects.toMatchObject({ reason: 'org_core_credential_missing' });
        expect(fetchSpy).not.toHaveBeenCalled();
        expect(errorLog).toHaveBeenCalledWith(
          expect.stringContaining('org_core_credential_missing'),
        );
      },
    );
  });
});
