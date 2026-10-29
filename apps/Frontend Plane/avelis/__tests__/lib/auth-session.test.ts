import { NextRequest } from 'next/server';

import { getSessionUser } from '../../src/lib/auth-session';

describe('getSessionUser', () => {
  const originalEnv = process.env;
  const originalFetch = global.fetch;

  beforeEach(() => {
    process.env = {
      ...originalEnv,
      AUTH_SERVICE_URL: 'http://auth-core:3011',
      INTERNAL_API_KEY: 'internal-key',
    };
    global.fetch = jest.fn();
    jest.spyOn(console, 'error').mockImplementation(() => undefined);
  });

  afterEach(() => {
    process.env = originalEnv;
    global.fetch = originalFetch;
    jest.restoreAllMocks();
  });

  it('forwards only allowlisted headers', async () => {
    (global.fetch as jest.Mock).mockResolvedValue({
      ok: true,
      json: async () => ({ user: { id: 'user-1', email: 'test@example.com' } }),
    });

    const request = new NextRequest('http://localhost/api/status', {
      headers: {
        cookie: 'auth_session=abc',
        'user-agent': 'jest-test',
        'accept-language': 'en-US',
        'x-malicious': 'blocked',
      },
    });

    await getSessionUser(request);

    const [, options] = (global.fetch as jest.Mock).mock.calls[0] as [
      string,
      { headers: Headers }
    ];

    expect(options.headers.get('cookie')).toBe('auth_session=abc');
    expect(options.headers.get('user-agent')).toBe('jest-test');
    expect(options.headers.get('accept-language')).toBe('en-US');
    expect(options.headers.get('x-malicious')).toBeNull();
    expect(options.headers.get('x-internal-api-key')).toBe('internal-key');
  });

  it('returns null when auth service rejects the request', async () => {
    (global.fetch as jest.Mock).mockResolvedValue({
      ok: false,
      status: 401,
    });

    const request = new NextRequest('http://localhost/api/status');

    await expect(getSessionUser(request)).resolves.toBeNull();
  });

  it('returns null for malformed optional session fields', async () => {
    (global.fetch as jest.Mock).mockResolvedValue({
      ok: true,
      json: async () => ({ user: { id: 'user-1', email: 42 } }),
    });

    const request = new NextRequest('http://localhost/api/status');

    await expect(getSessionUser(request)).resolves.toBeNull();
  });

  it('returns null when fetch throws', async () => {
    (global.fetch as jest.Mock).mockRejectedValue(new Error('network')); 

    const request = new NextRequest('http://localhost/api/status');

    await expect(getSessionUser(request)).resolves.toBeNull();
  });
});