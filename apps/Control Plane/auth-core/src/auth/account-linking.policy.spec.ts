import {
  canImplicitlyLinkProviderIdentity,
  normalizeIdentityEmail,
} from './account-linking.policy';

describe('account linking policy', () => {
  it('normalizes only case and surrounding whitespace', () => {
    expect(normalizeIdentityEmail(' Ima.DaCosta+work@Coresystem.com ')).toBe(
      'ima.dacosta+work@coresystem.com',
    );
  });

  it.each(['microsoft', 'google', 'apple'])(
    'allows verified exact-email %s identities',
    (provider) => {
      expect(
        canImplicitlyLinkProviderIdentity({
          provider,
          providerEmail: 'Ima.DaCosta@Coresystem.com',
          providerEmailVerified: true,
          canonicalEmail: 'ima.dacosta@coresystem.com',
        }),
      ).toBe(true);
    },
  );

  it('rejects unverified same-email identities', () => {
    expect(
      canImplicitlyLinkProviderIdentity({
        provider: 'microsoft',
        providerEmail: 'ima.dacosta@coresystem.com',
        providerEmailVerified: false,
        canonicalEmail: 'ima.dacosta@coresystem.com',
      }),
    ).toBe(false);
  });

  it.each(['github', 'vipps', 'okta'])(
    'allows %s only when its provider verifies the exact email',
    (provider) => {
      expect(
        canImplicitlyLinkProviderIdentity({
          provider,
          providerEmail: 'ima.dacosta@coresystem.com',
          providerEmailVerified: true,
          canonicalEmail: 'ima.dacosta@coresystem.com',
        }),
      ).toBe(true);
    },
  );

  it('does not equate Apple relay and corporate addresses', () => {
    expect(
      canImplicitlyLinkProviderIdentity({
        provider: 'apple',
        providerEmail: 'random@privaterelay.appleid.com',
        providerEmailVerified: true,
        canonicalEmail: 'ima.dacosta@coresystem.com',
      }),
    ).toBe(false);
  });
});
