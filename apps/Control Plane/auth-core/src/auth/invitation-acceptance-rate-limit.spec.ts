import {
  invitationAcceptanceInternalMarker,
  invitationActorRateLimitAddress,
  verifyInvitationAcceptanceInternalMarker,
} from './invitation-acceptance-rate-limit';

describe('invitation acceptance actor rate limit', () => {
  it('uses a stable private address per authenticated user', () => {
    const first = invitationActorRateLimitAddress('user-a', 'secret-a');
    expect(first).toMatch(/^fd[0-9a-f]{2}(?::[0-9a-f]{4}){7}$/);
    expect(invitationActorRateLimitAddress('user-a', 'secret-a')).toBe(first);
    expect(invitationActorRateLimitAddress('user-b', 'secret-a')).not.toBe(
      first,
    );
    expect(invitationActorRateLimitAddress('user-a', 'secret-b')).not.toBe(
      first,
    );
  });

  it('signs a short-lived wrapper-only marker for the exact invitation', () => {
    const now = 1_784_066_400_000;
    const marker = invitationAcceptanceInternalMarker(
      'inv-123',
      'secret-a',
      now,
    );
    expect(marker).toMatch(/^v1\.1784066400\.[A-Za-z0-9_-]{43}$/);
    expect(
      verifyInvitationAcceptanceInternalMarker(
        marker,
        'inv-123',
        'secret-a',
        now + 5_000,
      ),
    ).toBe(true);
    expect(
      verifyInvitationAcceptanceInternalMarker(
        marker,
        'other-invitation',
        'secret-a',
        now + 5_000,
      ),
    ).toBe(false);
    expect(
      verifyInvitationAcceptanceInternalMarker(
        marker,
        'inv-123',
        'secret-a',
        now + 31_000,
      ),
    ).toBe(false);
    expect(
      verifyInvitationAcceptanceInternalMarker(
        `${marker.slice(0, -1)}x`,
        'inv-123',
        'secret-a',
        now,
      ),
    ).toBe(false);
    expect(
      verifyInvitationAcceptanceInternalMarker(
        'v2.1784066400.invalid',
        'inv-123',
        'secret-a',
        now,
      ),
    ).toBe(false);
    expect(
      verifyInvitationAcceptanceInternalMarker(
        `v1.1784066400.${'a'.repeat(97)}`,
        'inv-123',
        'secret-a',
        now,
      ),
    ).toBe(false);
    expect(
      verifyInvitationAcceptanceInternalMarker(marker, '', 'secret-a', now),
    ).toBe(false);
  });

  it('fails closed for an invalid actor or missing secret', () => {
    expect(() => invitationActorRateLimitAddress('   ', 'secret-a')).toThrow(
      'authenticated user id is required',
    );
    expect(() => invitationActorRateLimitAddress('user-a', '   ')).toThrow(
      'Better Auth secret is required',
    );
  });
});
