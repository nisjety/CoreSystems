import { validateLifecycleFixtureTarget } from './lifecycle-test-fixture';

describe('lifecycle database target guard', () => {
  const fixtureID = '0123456789abcdef0123456789abcdef0123456789abcdef';

  it('allows only the exact loopback fixture database and generated marker id', () => {
    expect(() =>
      validateLifecycleFixtureTarget(
        'postgres://fixture:secret@127.0.0.1:5432/auth_invitation?sslmode=disable',
        'auth_invitation',
        fixtureID,
      ),
    ).not.toThrow();

    for (const candidate of [
      'postgres://fixture:secret@control-db:5432/auth_invitation',
      'postgres://fixture:secret@127.0.0.1:5432/controlplane',
    ]) {
      expect(() =>
        validateLifecycleFixtureTarget(candidate, 'auth_invitation', fixtureID),
      ).toThrow();
    }
    expect(() =>
      validateLifecycleFixtureTarget(
        'postgres://fixture:secret@127.0.0.1:5432/auth_invitation',
        'auth_invitation',
        'placeholder',
      ),
    ).toThrow();
  });
});
