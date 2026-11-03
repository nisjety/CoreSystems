// W3 E3.2-f: per-origin consent defaults in the `web_policy` claim.
//
// org-core's document carries them in `caps` (tighten-only, validated there);
// the claim stays version 1 and carries them beside the other modes. A value
// outside the known set is a malformed answer: no token, never a partial
// policy.
import {
  WEB_POLICY_CONSENT_MAX_SCOPE_KEY,
  WEB_POLICY_ORIGIN_CONSENT_KEY,
  WEB_POLICY_ORIGIN_CONSENT_VALUES,
  webPolicyClaimFromOrgCore,
} from './web-policy';

function answer(caps: Record<string, unknown>): unknown {
  return { data: { revision: 3, rev: 'abcdef012345', policy: { caps } } };
}

function failureOf(body: unknown): unknown {
  try {
    webPolicyClaimFromOrgCore(body);
  } catch (error) {
    return error;
  }
  return undefined;
}

describe('web_policy claim: per-origin consent defaults (E3.2-f)', () => {
  it('names the two keys and their values', () => {
    expect(WEB_POLICY_ORIGIN_CONSENT_KEY).toBe('browser.consent.origin');
    expect(WEB_POLICY_CONSENT_MAX_SCOPE_KEY).toBe('browser.consent.max_scope');
    expect(WEB_POLICY_ORIGIN_CONSENT_VALUES).toEqual({
      'browser.consent.origin': ['disabled', 'allowed'],
      'browser.consent.max_scope': ['turn', 'conversation', 'always'],
    });
  });

  it('carries them in caps, sorted with the other modes, and stays version 1', () => {
    const claim = webPolicyClaimFromOrgCore(
      answer({
        'browser.consent.origin': 'allowed',
        'browser.act.upload': 'disabled',
        'browser.consent.max_scope': 'conversation',
      }),
    );
    expect(claim).toEqual({
      v: 1,
      rev: 'abcdef012345',
      caps: {
        'browser.act.upload': 'disabled',
        'browser.consent.max_scope': 'conversation',
        'browser.consent.origin': 'allowed',
      },
    });
    expect(Object.keys(claim.caps ?? {})).toEqual([
      'browser.act.upload',
      'browser.consent.max_scope',
      'browser.consent.origin',
    ]);
  });

  it.each(['turn', 'conversation', 'always'])('accepts scope %s', (scope) => {
    expect(
      webPolicyClaimFromOrgCore(answer({ 'browser.consent.max_scope': scope }))
        .caps,
    ).toEqual({ 'browser.consent.max_scope': scope });
  });

  it.each([
    ['a forced consent', { 'browser.consent.origin': 'forced' }],
    ['a scope on the ability', { 'browser.consent.origin': 'turn' }],
    ['a "once" scope', { 'browser.consent.max_scope': 'once' }],
    ['an upper-case scope', { 'browser.consent.max_scope': 'Turn' }],
    ['an empty scope', { 'browser.consent.max_scope': '' }],
    ['a mode on the scope', { 'browser.consent.max_scope': 'disabled' }],
  ])('refuses %s as a malformed answer', (_, caps) => {
    expect(failureOf(answer(caps))).toMatchObject({
      reason: 'org_core_malformed_response',
      detail: 'origin_consent',
    });
  });

  it('passes other unknown capability keys through unchanged (the edge counts them)', () => {
    expect(
      webPolicyClaimFromOrgCore(answer({ 'browser.consent.future': 'x' }))
        .caps,
    ).toEqual({ 'browser.consent.future': 'x' });
  });
});
