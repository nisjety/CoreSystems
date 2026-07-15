import { createHash } from 'node:crypto';

import { issuedTokenAuditIdentity } from './audit-event-identity';

describe('issuedTokenAuditIdentity', () => {
  const tokenWith = (claims: Record<string, unknown>): string =>
    [
      'header',
      Buffer.from(JSON.stringify(claims)).toString('base64url'),
      'signature',
    ].join('.');

  it('derives stable identity and occurrence time from the signed artifact', () => {
    const token = tokenWith({ iat: 1784073600 });

    expect(issuedTokenAuditIdentity(token, 'plane-token')).toEqual({
      eventId: `plane-token:${createHash('sha256')
        .update(token)
        .digest('hex')}`,
      occurredAt: '2026-07-15T00:00:00.000Z',
    });
  });

  it.each([
    ['missing JWT segments', 'not-a-jwt', 'stable audit identity'],
    ['malformed JWT payload', 'header.!.signature', 'stable audit identity'],
    [
      'missing issued-at claim',
      tokenWith({ sub: 'service:worker' }),
      'producer occurrence time',
    ],
  ])('rejects %s', (_case, token, message) => {
    expect(() => issuedTokenAuditIdentity(token, 'model-token')).toThrow(
      message,
    );
  });
});
