import { Logger } from '@nestjs/common';

import {
  CONNECTION_POSTURE_TIMEOUT_MS,
  IntegrationConnectionPostureClient,
} from './integration-connection-posture.client';

// ZDR-Z-14e: Auth Core reads one connection's ZDR posture from
// integration-corev2 as the session user (E2). Only fetch and the token issuer
// are doubles.

const ZDR_POSTURE = {
  zdr: true,
  authority: 'interactive-org-retention-policy',
} as const;
const INPUT = {
  userId: 'user-a',
  orgId: 'org-a',
  connectionId: 'conn/1',
  retentionPosture: ZDR_POSTURE,
};
const USER_TOKEN = 'user-bound.ingestion.token';

const POSTURE_DATA = {
  connectionId: 'conn/1',
  providerKey: 'openai-codex-subscription',
  capability: 'not_supported',
  waiverRoute: true,
  ackState: 'acknowledged',
  ackReason: '',
  ackId: 'ack-1',
  digest: 'sha256:abc',
  textVersion: 'v1',
};

function reply(status: number, body: unknown, jsonFails = false) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () =>
      jsonFails
        ? Promise.reject(new SyntaxError('bad json'))
        : Promise.resolve(body),
  } as unknown as Response;
}

describe('IntegrationConnectionPostureClient (ZDR-Z-14e)', () => {
  const originalUrl = process.env.INTEGRATION_CORE_URL;
  let tokens: { issuePlaneToken: jest.Mock };
  let fetchSpy: jest.SpyInstance;
  let logged: string[];

  function client() {
    return new IntegrationConnectionPostureClient(tokens as never);
  }

  beforeEach(() => {
    delete process.env.INTEGRATION_CORE_URL;
    tokens = {
      issuePlaneToken: jest.fn().mockReturnValue({ token: USER_TOKEN }),
    };
    fetchSpy = jest
      .spyOn(global, 'fetch')
      .mockResolvedValue(reply(200, { data: POSTURE_DATA }));
    logged = [];
    for (const level of ['log', 'warn', 'error', 'debug'] as const) {
      jest
        .spyOn(Logger.prototype, level)
        .mockImplementation((...args: unknown[]) => {
          logged.push(JSON.stringify(args));
        });
    }
  });

  afterEach(() => {
    jest.useRealTimers();
    jest.restoreAllMocks();
    if (originalUrl === undefined) delete process.env.INTEGRATION_CORE_URL;
    else process.env.INTEGRATION_CORE_URL = originalUrl;
  });

  it('a typed 200 maps to posture, read as the session user with no internal key', async () => {
    const result = await client().readConnectionPosture(INPUT);
    expect(result).toEqual({
      kind: 'posture',
      providerKey: 'openai-codex-subscription',
      capability: 'not_supported',
      waiverRoute: true,
      ackState: 'acknowledged',
      ackReason: '',
      ackId: 'ack-1',
      digest: 'sha256:abc',
      textVersion: 'v1',
    });

    expect(tokens.issuePlaneToken).toHaveBeenCalledTimes(1);
    expect(tokens.issuePlaneToken).toHaveBeenCalledWith('ingestion', {
      userId: 'user-a',
      orgId: 'org-a',
      principalType: 'user',
      retentionPosture: ZDR_POSTURE,
    });

    expect(fetchSpy).toHaveBeenCalledTimes(1);
    const [url, init] = fetchSpy.mock.calls[0] as [string, RequestInit];
    expect(url).toBe(
      'http://integration-api:3026/api/v1/model-connections/conn%2F1/zdr-posture',
    );
    expect(init.method).toBe('GET');
    const headers = new Headers(init.headers);
    expect(headers.get('authorization')).toBe(`Bearer ${USER_TOKEN}`);
    expect(headers.has('x-internal-api-key')).toBe(false);
    expect(
      [...headers.keys()].some((key) => key.toLowerCase().includes('internal')),
    ).toBe(false);
    expect(init.signal).toBeInstanceOf(AbortSignal);
  });

  it('reads INTEGRATION_CORE_URL once, in the constructor', async () => {
    process.env.INTEGRATION_CORE_URL = 'http://integration.test:9000/';
    const c = client();
    process.env.INTEGRATION_CORE_URL = 'http://changed.test';
    await c.readConnectionPosture(INPUT);
    expect(String(fetchSpy.mock.calls[0][0])).toMatch(
      /^http:\/\/integration\.test:9000\/api\/v1\/model-connections\//,
    );
  });

  it.each(['supported', 'not_supported', 'cannot_verify'])(
    'capability %s is accepted',
    async (capability) => {
      fetchSpy.mockResolvedValue(
        reply(200, { data: { ...POSTURE_DATA, capability } }),
      );
      expect(await client().readConnectionPosture(INPUT)).toMatchObject({
        kind: 'posture',
        capability,
      });
    },
  );

  it.each(['unassessed', 'not_required', 'required', 'acknowledged'])(
    'ackState %s is accepted',
    async (ackState) => {
      fetchSpy.mockResolvedValue(
        reply(200, { data: { ...POSTURE_DATA, ackState } }),
      );
      expect(await client().readConnectionPosture(INPUT)).toMatchObject({
        kind: 'posture',
        ackState,
      });
    },
  );

  it.each([
    ['an unknown capability', { capability: 'maybe' }],
    ['an unknown ackState', { ackState: 'ACKNOWLEDGED' }],
    ['waiverRoute not a boolean', { waiverRoute: 'true' }],
    ['digest missing', { digest: undefined }],
    ['ackId not a string', { ackId: 7 }],
    ['textVersion missing', { textVersion: undefined }],
    ['providerKey missing', { providerKey: undefined }],
    ['ackReason null', { ackReason: null }],
    ['an answer about another connection', { connectionId: 'conn-2' }],
  ])('a 200 with %s is unavailable', async (_case, change) => {
    fetchSpy.mockResolvedValue(
      reply(200, { data: { ...POSTURE_DATA, ...change } }),
    );
    expect(await client().readConnectionPosture(INPUT)).toEqual({
      kind: 'unavailable',
    });
  });

  it('a 200 without the data envelope is unavailable', async () => {
    fetchSpy.mockResolvedValue(reply(200, POSTURE_DATA));
    expect(await client().readConnectionPosture(INPUT)).toEqual({
      kind: 'unavailable',
    });
  });

  it.each([
    [403, 'not_connection_owner'],
    [403, 'forbidden'],
    [404, 'connection_not_found'],
    [404, 'not_a_model_connection'],
  ])('%s %s maps to not_owner', async (status, code) => {
    fetchSpy.mockResolvedValue(
      reply(status, { error: { code, message: 'x' } }),
    );
    expect(await client().readConnectionPosture(INPUT)).toEqual({
      kind: 'not_owner',
    });
  });

  it.each([
    ['a bare 404 (routes switched off)', reply(404, 'Not Found', true)],
    ['a 404 with another code', reply(404, { error: { code: 'not_found' } })],
    [
      'a 403 with a not-found code',
      reply(403, { error: { code: 'connection_not_found' } }),
    ],
    [
      'a 404 with a forbidden code',
      reply(404, { error: { code: 'not_connection_owner' } }),
    ],
    ['a 401', reply(401, { error: { code: 'unauthorized' } })],
    ['a 500', reply(500, { error: { code: 'internal' } })],
    ['a 503', reply(503, {})],
    ['a 200 with a non-JSON body', reply(200, undefined, true)],
  ])('%s is unavailable', async (_case, response) => {
    fetchSpy.mockResolvedValue(response);
    expect(await client().readConnectionPosture(INPUT)).toEqual({
      kind: 'unavailable',
    });
  });

  it('a transport error is unavailable, and there is no retry', async () => {
    fetchSpy.mockRejectedValue(new TypeError('fetch failed'));
    expect(await client().readConnectionPosture(INPUT)).toEqual({
      kind: 'unavailable',
    });
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });

  it('a token mint failure is unavailable and nothing is fetched', async () => {
    tokens.issuePlaneToken.mockImplementation(() => {
      throw new Error('no key');
    });
    expect(await client().readConnectionPosture(INPUT)).toEqual({
      kind: 'unavailable',
    });
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it('a 2 s timeout is unavailable even when fetch ignores its signal', async () => {
    jest.useFakeTimers();
    fetchSpy.mockImplementation(() => new Promise<Response>(() => undefined));
    const pending = client().readConnectionPosture(INPUT);
    await jest.advanceTimersByTimeAsync(CONNECTION_POSTURE_TIMEOUT_MS + 1);
    expect(await pending).toEqual({ kind: 'unavailable' });
    expect(CONNECTION_POSTURE_TIMEOUT_MS).toBe(2000);
  });

  it('the timeout aborts the request signal', async () => {
    jest.useFakeTimers();
    let signal: AbortSignal | undefined;
    fetchSpy.mockImplementation((_url: unknown, init?: RequestInit) => {
      signal = init?.signal ?? undefined;
      return new Promise<Response>(() => undefined);
    });
    const pending = client().readConnectionPosture(INPUT);
    await jest.advanceTimersByTimeAsync(CONNECTION_POSTURE_TIMEOUT_MS + 1);
    await pending;
    expect(signal?.aborted).toBe(true);
  });

  it('does not cache: each read goes to integration-corev2', async () => {
    const c = client();
    await c.readConnectionPosture(INPUT);
    await c.readConnectionPosture(INPUT);
    expect(fetchSpy).toHaveBeenCalledTimes(2);
  });

  it('never logs the token', async () => {
    for (const response of [
      reply(500, {}),
      reply(404, 'x', true),
      reply(200, { data: {} }),
    ]) {
      fetchSpy.mockResolvedValue(response);
      await client().readConnectionPosture(INPUT);
    }
    fetchSpy.mockRejectedValue(new Error(`boom ${USER_TOKEN}`));
    await client().readConnectionPosture(INPUT);
    expect(logged.length).toBeGreaterThan(0);
    expect(logged.join('\n')).not.toContain(USER_TOKEN);
  });

  it('missing ids are unavailable without a mint or a fetch', async () => {
    for (const change of [
      { userId: '' },
      { orgId: '' },
      { connectionId: '' },
    ]) {
      expect(
        await client().readConnectionPosture({ ...INPUT, ...change }),
      ).toEqual({ kind: 'unavailable' });
    }
    expect(tokens.issuePlaneToken).not.toHaveBeenCalled();
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
