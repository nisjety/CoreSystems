import { selectNatsCredentials } from './nats-credentials';

describe('selectNatsCredentials', () => {
  it('prefers scoped user/password over a migration token', () => {
    expect(
      selectNatsCredentials({
        NATS_USER: 'control-runtime',
        NATS_PASSWORD: '0123456789abcdef0123456789abcdef',
        NATS_TOKEN: 'abcdef0123456789abcdef0123456789',
        NATS_ALLOW_TOKEN_FALLBACK: '1',
      }),
    ).toEqual({
      user: 'control-runtime',
      pass: '0123456789abcdef0123456789abcdef',
    });
  });

  it('requires the explicit migration switch for token auth', () => {
    expect(() =>
      selectNatsCredentials({ NATS_TOKEN: 'abcdef0123456789abcdef0123456789' }),
    ).toThrow(/fallback/i);
    expect(
      selectNatsCredentials({
        NATS_TOKEN: 'abcdef0123456789abcdef0123456789',
        NATS_ALLOW_TOKEN_FALLBACK: '1',
      }),
    ).toEqual({ token: 'abcdef0123456789abcdef0123456789' });
  });

  it('rejects partial scoped credentials', () => {
    expect(() =>
      selectNatsCredentials({ NATS_USER: 'control-runtime' }),
    ).toThrow(/together/i);
  });
});
