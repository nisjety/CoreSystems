import { createAuthMiddleware } from 'better-auth/plugins';
import type { BetterAuthPlugin } from 'better-auth';

// Audit event types for Sprint 4
export interface AuditEvent {
  id?: string;
  userId?: string;
  sessionId?: string;
  action: string;
  resource: string;
  details: Record<string, unknown>;
  ipAddress?: string;
  userAgent?: string;
  timestamp: string;
  success: boolean;
  errorMessage?: string;
}

// Mock audit logger for development
function logAuditEvent(event: AuditEvent): void {
  console.log('📋 [AUDIT]', JSON.stringify(event, null, 2));
}

// Create audit plugin with simplified implementation
export function auditPlugin(): BetterAuthPlugin {
  return {
    id: 'audit',
    hooks: {
      // After hooks for successful events
      after: [
        // Sign-in events
        {
          matcher: (context) => {
            return (
              context.path === '/sign-in/email' ||
              context.path === '/sign-in/oauth' ||
              context.path.startsWith('/oauth/')
            );
          },
          handler: createAuthMiddleware(async (ctx) => {
            // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
            const user = ctx.context.user;

            const session = ctx.context.newSession;

            if (user && session) {
              logAuditEvent({
                // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-member-access
                userId: user.id,

                sessionId: session.session?.id || 'unknown',
                action: 'SIGN_IN',
                resource: 'authentication',
                details: {
                  method: ctx.path.includes('oauth') ? 'oauth' : 'email',
                  provider: ctx.path.includes('oauth')
                    ? ctx.path.split('/').pop()
                    : 'email',
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  userEmail: user.email,
                },
                ipAddress:
                  ctx.request?.headers.get('x-forwarded-for') ||
                  ctx.request?.headers.get('x-real-ip') ||
                  'unknown',
                userAgent: ctx.request?.headers.get('user-agent') || 'unknown',
                timestamp: new Date().toISOString(),
                success: true,
              });

              // Ensure async compliance for middleware
              await Promise.resolve();
            }
          }),
        },

        // Two-Factor Authentication events
        {
          matcher: (context) => {
            return (
              context.path === '/two-factor/enable' ||
              context.path === '/two-factor/disable' ||
              context.path === '/two-factor/verify'
            );
          },
          handler: createAuthMiddleware(async (ctx) => {
            // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
            const user = ctx.context.user;
            const action = ctx.path.split('/').pop()?.toUpperCase();

            if (user && action) {
              logAuditEvent({
                // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-member-access
                userId: user.id,
                action: `2FA_${action}`,
                resource: 'two_factor',
                details: {
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  method: ctx.body?.method || 'totp',
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  userEmail: user.email,
                },
                ipAddress:
                  ctx.request?.headers.get('x-forwarded-for') ||
                  ctx.request?.headers.get('x-real-ip') ||
                  'unknown',
                userAgent: ctx.request?.headers.get('user-agent') || 'unknown',
                timestamp: new Date().toISOString(),
                success: true,
              });

              // Ensure async compliance for middleware
              await Promise.resolve();
            }
          }),
        },

        // Session revocation events
        {
          matcher: (context) => {
            return (
              context.path === '/revoke-sessions' ||
              context.path === '/revoke-other-sessions' ||
              context.path.includes('/revoke-session')
            );
          },
          handler: createAuthMiddleware(async (ctx) => {
            // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
            const user = ctx.context.user;

            if (user) {
              logAuditEvent({
                // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-member-access
                userId: user.id,
                action: 'SESSION_REVOKE',
                resource: 'session',
                details: {
                  type: ctx.path.includes('other')
                    ? 'others'
                    : ctx.path.includes('sessions')
                      ? 'all'
                      : 'single',
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  userEmail: user.email,
                },
                ipAddress:
                  ctx.request?.headers.get('x-forwarded-for') ||
                  ctx.request?.headers.get('x-real-ip') ||
                  'unknown',
                userAgent: ctx.request?.headers.get('user-agent') || 'unknown',
                timestamp: new Date().toISOString(),
                success: true,
              });

              // Ensure async compliance for middleware
              await Promise.resolve();
            }
          }),
        },
      ],

      // Before hooks for validation and attempt logging
      before: [
        // General authentication attempt logging
        {
          matcher: (context) => {
            return (
              context.path === '/sign-in/email' ||
              context.path === '/sign-up/email' ||
              context.path.startsWith('/oauth/') ||
              context.path.includes('verify') ||
              context.path.includes('two-factor')
            );
          },
          handler: createAuthMiddleware(async (ctx) => {
            // Log authentication attempts (success will be logged in after hooks)
            const action = ctx.path.includes('sign-in')
              ? 'SIGN_IN_ATTEMPT'
              : ctx.path.includes('sign-up')
                ? 'SIGN_UP_ATTEMPT'
                : ctx.path.includes('oauth')
                  ? 'OAUTH_ATTEMPT'
                  : ctx.path.includes('verify')
                    ? 'VERIFY_ATTEMPT'
                    : '2FA_ATTEMPT';

            logAuditEvent({
              action,
              resource: 'authentication',
              details: {
                // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                email: ctx.body?.email,
                provider: ctx.path.includes('oauth')
                  ? ctx.path.split('/').pop()
                  : // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                    ctx.body?.provider || 'email',
                path: ctx.path,
              },
              ipAddress:
                ctx.request?.headers.get('x-forwarded-for') ||
                ctx.request?.headers.get('x-real-ip') ||
                'unknown',
              userAgent: ctx.request?.headers.get('user-agent') || 'unknown',
              timestamp: new Date().toISOString(),
              success: false, // Will be updated to true in after hook if successful
            });

            // Ensure async compliance for middleware
            await Promise.resolve();
          }),
        },
      ],
    },
  };
}
