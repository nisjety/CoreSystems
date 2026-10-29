import { canonicalTokenContext } from './plane-token-membership';

describe('plane token canonical membership', () => {
  it('uses active Better Auth membership for the token tenant and role', () => {
    expect(
      canonicalTokenContext('org-a', {
        organizationId: 'org-a',
        userId: 'user-a',
        role: 'owner',
      }),
    ).toEqual({ orgId: 'org-a', role: 'owner' });
  });

  it('fails closed when the active organization has no canonical membership', () => {
    expect(canonicalTokenContext('org-a', null)).toBeNull();
  });
});
