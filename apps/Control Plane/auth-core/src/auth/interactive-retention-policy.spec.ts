import {
  InteractiveRetentionPolicyConfigurationError,
  resolveInteractiveRetentionPosture,
} from './interactive-retention-policy';

describe('interactive retention policy', () => {
  it('defaults every organization to ZDR when no authoritative policy is configured', () => {
    expect(resolveInteractiveRetentionPosture(undefined, 'org-a')).toEqual({
      zdr: true,
      authority: 'interactive-org-retention-policy',
    });
  });

  it('permits persistent interactive processing only for an exact organization with evidence', () => {
    expect(
      resolveInteractiveRetentionPosture(
        JSON.stringify({
          version: 1,
          organizations: {
            'org-approved': {
              posture: 'persistent',
              policyEvidenceSha256:
                'a3b6c4a80f6bf3442d360bfb5e2aee5d1de7b28fd2049c1cad5a6d79e3b84fe3',
            },
          },
        }),
        'org-approved',
      ),
    ).toEqual({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
  });

  it('does not let an unlisted organization inherit persistent processing', () => {
    expect(
      resolveInteractiveRetentionPosture(
        JSON.stringify({
          version: 1,
          organizations: {
            'org-approved': {
              posture: 'persistent',
              policyEvidenceSha256:
                'a3b6c4a80f6bf3442d360bfb5e2aee5d1de7b28fd2049c1cad5a6d79e3b84fe3',
            },
          },
        }),
        'org-other',
      ),
    ).toEqual({
      zdr: true,
      authority: 'interactive-org-retention-policy',
    });
  });

  it.each([
    '{}',
    '{"version":2,"organizations":{}}',
    '{"version":1,"organizations":{"org-a":{"posture":"persistent"}}}',
    '{"version":1,"organizations":{"org-a":{"posture":"persistent","policyEvidenceSha256":"not-a-sha"}}}',
    '{"version":1,"organizations":{"org-a":{"posture":"unknown"}}}',
  ])('fails closed for malformed policy %s', (raw) => {
    expect(() => resolveInteractiveRetentionPosture(raw, 'org-a')).toThrow(
      InteractiveRetentionPolicyConfigurationError,
    );
  });
});
