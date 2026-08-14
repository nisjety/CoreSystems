import {
  buildInvitationLink,
  canonicalPublicOrigin,
  escapeInvitationHtml,
} from './invitation-email';

describe('invitation email contract', () => {
  it('builds a browser-facing Verevon link and encodes the opaque invitation id', () => {
    expect(
      buildInvitationLink('https://verevon.example', 'invite/with space'),
    ).toBe('https://verevon.example/accept-invitation/invite%2Fwith%20space');
  });

  it('requires a canonical HTTPS origin in production', () => {
    expect(canonicalPublicOrigin('https://verevon.example/', true)).toBe(
      'https://verevon.example',
    );
    for (const value of [
      'http://verevon.example',
      'https://user:pass@verevon.example',
      'https://verevon.example/base',
      'https://verevon.example?tenant=acme',
      'https://verevon.example/#fragment',
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
    expect(() =>
      canonicalPublicOrigin('http://verevon.example', false),
    ).toThrow('canonical HTTPS origin');
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
