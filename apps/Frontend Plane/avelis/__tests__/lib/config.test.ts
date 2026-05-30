import { getAuthServiceUrl, getInternalApiKey } from '../../src/lib/config';

describe('config', () => {
  const originalEnv = process.env;

  beforeEach(() => {
    process.env = { ...originalEnv };
  });

  afterAll(() => {
    process.env = originalEnv;
  });

  it('throws when INTERNAL_API_KEY is missing', () => {
    delete process.env.INTERNAL_API_KEY;

    expect(() => getInternalApiKey()).toThrow(
      'INTERNAL_API_KEY must be configured',
    );
  });

  it('returns INTERNAL_API_KEY when configured', () => {
    process.env.INTERNAL_API_KEY = 'test-key';

    expect(getInternalApiKey()).toBe('test-key');
  });

  it('returns AUTH_SERVICE_URL when configured', () => {
    process.env.AUTH_SERVICE_URL = 'http://localhost:3011';

    expect(getAuthServiceUrl()).toBe('http://localhost:3011');
  });

  it('returns the default AUTH_SERVICE_URL when not configured', () => {
    delete process.env.AUTH_SERVICE_URL;

    expect(getAuthServiceUrl()).toBe('http://auth-core:3011');
  });
});