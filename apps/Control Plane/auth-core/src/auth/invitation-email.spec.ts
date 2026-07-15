import {
  buildInvitationLink,
  canonicalPublicOrigin,
  escapeInvitationHtml,
} from './invitation-email';

describe('invitation email contract', () => {
  it('builds a browser-facing Velion link and encodes the opaque invitation id', () => {
    expect(
      buildInvitationLink('https://velion.example', 'invite/with space'),
    ).toBe('https://velion.example/accept-invitation/invite%2Fwith%20space');
  });

  it('requires a canonical HTTPS origin in production', () => {
    expect(canonicalPublicOrigin('https://velion.example/', true)).toBe(
      'https://velion.example',
    );
    for (const value of [
      'http://velion.example',
      'https://user:pass@velion.example',
      'https://velion.example/base',
      'https://velion.example?tenant=acme',
      'https://velion.example/#fragment',
    ]) {
      expect(() => canonicalPublicOrigin(value, true)).toThrow(
        'canonical HTTPS origin',
      );
    }
  });

  it('allows loopback HTTP only outside production', () => {
    expect(canonicalPublicOrigin('http://localhost:5173', false)).toBe(
      'http://localhost:5173',
    );
    expect(canonicalPublicOrigin('http://127.0.0.1:5173', false)).toBe(
      'http://127.0.0.1:5173',
    );
    expect(() => canonicalPublicOrigin('http://velion.example', false)).toThrow(
      'canonical HTTPS origin',
    );
    expect(() => canonicalPublicOrigin('http://localhost:5173', true)).toThrow(
      'canonical HTTPS origin',
    );
  });

  it('escapes untrusted inviter and organization fields before HTML interpolation', () => {
    expect(escapeInvitationHtml('<img src=x onerror="alert(1)"> & Acme')).toBe(
      '&lt;img src=x onerror=&quot;alert(1)&quot;&gt; &amp; Acme',
    );
  });
});
