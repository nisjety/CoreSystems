import {
  assertNotDisposableEmail,
  DisposableEmailError,
  emailDomain,
  isDisposableEmail,
  resetDisposableEmailDomainCacheForTest,
} from './disposable-email';

describe('disposable email blocking', () => {
  const originalEnv = process.env;

  beforeEach(() => {
    jest.resetModules();
    process.env = { ...originalEnv };
    delete process.env.DISPOSABLE_EMAIL_BLOCKLIST_ENABLED;
    delete process.env.DISPOSABLE_EMAIL_DOMAINS_FILE;
    resetDisposableEmailDomainCacheForTest();
  });

  afterAll(() => {
    process.env = originalEnv;
  });

  it('extracts the normalized email domain', () => {
    expect(emailDomain('Person@Mailinator.com.')).toBe('mailinator.com');
  });

  it('blocks fallback disposable domains', () => {
    expect(isDisposableEmail('user@mailinator.com')).toBe(true);
    expect(() => assertNotDisposableEmail('user@mailinator.com')).toThrow(
      DisposableEmailError,
    );
  });

  it('allows permanent domains', () => {
    expect(isDisposableEmail('user@example.com')).toBe(false);
    expect(() => assertNotDisposableEmail('user@example.com')).not.toThrow();
  });

  it('can be disabled by environment flag', () => {
    process.env.DISPOSABLE_EMAIL_BLOCKLIST_ENABLED = 'false';
    expect(isDisposableEmail('user@mailinator.com')).toBe(false);
  });
});
