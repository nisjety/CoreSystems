const mockRows = jest.fn();
jest.mock('../db', () => ({ db: { select: () => ({ from: () => ({ where: () => ({ limit: mockRows }) }) }) } }));
jest.mock('better-auth/api', () => ({
  createAuthMiddleware: (handler: unknown) => handler,
  APIError: class extends Error {},
}));

import { setAuthIntegrationService, userServiceIntegrationPlugin } from './user-service-integration.plugin';
import type { AuthIntegrationService } from '../internal/auth-integration.service';

const mockIntegration = {
  handleUserRegistration: jest.fn().mockResolvedValue(undefined),
  handleAccountLinked: jest.fn().mockResolvedValue(undefined),
  handleUserLogin: jest.fn().mockResolvedValue(undefined),
};

async function callback(context: object, provider = 'microsoft') {
  const plugin = userServiceIntegrationPlugin();
  const hook = plugin.hooks!.after!.find(hook => hook.matcher({ path: '/callback/:id' } as never));
  if (!hook) throw new Error('OAuth callback hook missing');
  await hook.handler({ context: { ...context, internalAdapter: { findAccounts: mockRows } }, params: { id: provider }, request: new Request('http://localhost/callback/microsoft') } as never);
}

describe('OAuth Control integration', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    setAuthIntegrationService(mockIntegration as unknown as AuthIntegrationService);
    mockRows.mockResolvedValue([{ id: 'account-1', accountId: 'microsoft-subject', providerId: 'microsoft', scope: 'openid profile email' }]);
  });

  it('syncs a new Microsoft user and session from the actual Better Auth callback shape', async () => {
    await callback({ newSession: {
      user: { id: 'user-1', email: 'person@example.com', name: 'Person', emailVerified: true, createdAt: new Date() },
      session: { id: 'session-1', activeOrganizationId: 'org-1' },
    } });
    expect(mockIntegration.handleUserRegistration).toHaveBeenCalledWith(expect.objectContaining({ id: 'user-1', provider: 'microsoft' }));
    expect(mockIntegration.handleUserLogin).toHaveBeenCalledWith(expect.objectContaining({ userId: 'user-1', sessionId: 'session-1', provider: 'microsoft', activeOrganizationId: 'org-1' }));
  });

  it('syncs an existing Microsoft account and its fresh session', async () => {
    await callback({ newSession: {
      user: { id: 'user-1', email: 'person@example.com', createdAt: '2020-01-01' },
      session: { id: 'session-2' },
    } });
    expect(mockIntegration.handleUserRegistration).not.toHaveBeenCalled();
    expect(mockIntegration.handleAccountLinked).toHaveBeenCalledWith(expect.objectContaining({ id: 'user-1', provider: 'microsoft' }));
    expect(mockIntegration.handleUserLogin).toHaveBeenCalledTimes(1);
  });

  it('also handles email sign-in from newSession without context.user', async () => {
    const plugin = userServiceIntegrationPlugin();
    const hook = plugin.hooks!.after!.find(hook => hook.matcher({ path: '/sign-in/email' } as never));
    if (!hook) throw new Error('Email sign-in hook missing');
    await hook.handler({ context: { newSession: {
      user: { id: 'user-1', email: 'person@example.com' },
      session: { id: 'email-session', activeOrganizationId: 'org-1' },
    } } } as never);
    expect(mockIntegration.handleUserLogin).toHaveBeenCalledWith(expect.objectContaining({ sessionId: 'email-session', provider: 'email' }));
  });

  it('does not provision users for unsuccessful callbacks', async () => {
    await callback({});
    expect(mockRows).not.toHaveBeenCalled();
    expect(mockIntegration.handleUserLogin).not.toHaveBeenCalled();
  });
});
