import {
  resolveSoleOrganizationId,
  withSoleOrganizationActivated,
} from './sole-org-auto-activation';

const mockSelect = jest.fn();

jest.mock('../db', () => ({
  db: {
    select: (...args: unknown[]): unknown => mockSelect(...args),
  },
}));

function mockMembershipRows(rows: Array<{ organizationId: string }>): void {
  mockSelect.mockReturnValue({
    from: jest.fn().mockReturnValue({
      where: jest.fn().mockReturnValue({
        limit: jest.fn().mockResolvedValue(rows),
      }),
    }),
  });
}

function mockMembershipLookupFailure(error: Error): void {
  mockSelect.mockImplementation(() => {
    throw error;
  });
}

describe('resolveSoleOrganizationId', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('returns the organization id when the user has exactly one membership', async () => {
    mockMembershipRows([{ organizationId: 'org-sole' }]);

    await expect(resolveSoleOrganizationId('user-1')).resolves.toBe('org-sole');
  });

  it('returns null when the user has no memberships', async () => {
    mockMembershipRows([]);

    await expect(resolveSoleOrganizationId('user-1')).resolves.toBeNull();
  });

  it('returns null when the user belongs to several organizations', async () => {
    mockMembershipRows([
      { organizationId: 'org-a' },
      { organizationId: 'org-b' },
    ]);

    await expect(resolveSoleOrganizationId('user-1')).resolves.toBeNull();
  });
});

describe('withSoleOrganizationActivated', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('activates the sole organization on a fresh session', async () => {
    mockMembershipRows([{ organizationId: 'org-sole' }]);
    const session = { userId: 'user-1', token: 'tok' };

    const result = await withSoleOrganizationActivated(session);

    expect(result).toEqual({
      userId: 'user-1',
      token: 'tok',
      activeOrganizationId: 'org-sole',
    });
    // Immutability: the original session object must not be patched in place.
    expect(session).not.toHaveProperty('activeOrganizationId');
  });

  it('leaves a multi-org session untouched — switching stays a user action', async () => {
    mockMembershipRows([
      { organizationId: 'org-a' },
      { organizationId: 'org-b' },
    ]);
    const session = { userId: 'user-1', token: 'tok' };

    const result = await withSoleOrganizationActivated(session);

    expect(result).toBe(session);
    expect(result).not.toHaveProperty('activeOrganizationId');
  });

  it('leaves an org-less user session untouched', async () => {
    mockMembershipRows([]);
    const session = { userId: 'user-1', token: 'tok' };

    const result = await withSoleOrganizationActivated(session);

    expect(result).toBe(session);
    expect(result).not.toHaveProperty('activeOrganizationId');
  });

  it('respects an explicitly pre-seeded active organization without querying', async () => {
    const session = {
      userId: 'user-1',
      token: 'tok',
      activeOrganizationId: 'org-preset',
    };

    const result = await withSoleOrganizationActivated(session);

    expect(result).toBe(session);
    expect(result.activeOrganizationId).toBe('org-preset');
    expect(mockSelect).not.toHaveBeenCalled();
  });

  it('fails open when the membership lookup throws — sign-in must not break', async () => {
    mockMembershipLookupFailure(new Error('database unavailable'));
    const session = { userId: 'user-1', token: 'tok' };

    const result = await withSoleOrganizationActivated(session);

    expect(result).toBe(session);
    expect(result).not.toHaveProperty('activeOrganizationId');
  });
});
