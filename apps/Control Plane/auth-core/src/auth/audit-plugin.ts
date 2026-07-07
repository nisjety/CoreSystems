import { createAuthMiddleware } from 'better-auth/api';
import type { BetterAuthPlugin } from 'better-auth';

/**
 * Minimal core-publish surface the audit plugin needs. Satisfied by
 * DirectNatsService (the LOCAL control-plane bus → controlplane-nats, where
 * audit-core's primary subscription listens). Audit deliberately uses the local
 * bus, not the shared velion-nats bus (which carries cross-plane domain/ACL
 * events), so it never depends on the shared bus being up.
 */
interface AuditNatsPublisher {
  publishPlain(subject: string, payload: Record<string, unknown>): void;
}

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

/**
 * audit-core AuditEvent schema (velion.audit.v1.control.<event>)
 * org_id, plane, and event are REQUIRED fields — omit the publish when org_id is absent.
 */
interface VelionAuditEvent {
  occurred_at: string;
  org_id: string;
  user_id?: string;
  actor_role?: string;
  plane: string;
  event: string;
  subject?: string;
  resource_id?: string;
  outcome: 'ok' | 'denied' | 'error';
  details?: Record<string, unknown>;
  request_id?: string;
  ip_address?: string;
  user_agent?: string;
}

// Module-level singleton — set by setAuditNatsPublisher() called from auth-service.initializer
let auditNats: AuditNatsPublisher | null = null;

export function setAuditNatsPublisher(svc: AuditNatsPublisher): void {
  auditNats = svc;
}

/**
 * Publish to velion.audit.v1.control.<event> over the LOCAL control-plane bus
 * (controlplane-nats), only when org_id is known (audit-core rejects events
 * without it). Core publish matches audit-core's core QueueSubscribe.
 */
function publishVelionAudit(evt: VelionAuditEvent): void {
  if (!auditNats) return;
  const subject = `velion.audit.v1.control.${evt.event}`;
  auditNats.publishPlain(subject, evt as unknown as Record<string, unknown>);
}

// Dev-only fallback: when NATS is unconfigured the event can't reach
// audit-core, so we at least surface it on stdout. When NATS IS configured
// this is a no-op — the durable record goes to velion.audit.v1.control.<event>.
function logAuditEvent(event: AuditEvent): void {
  if (auditNats) return;
  console.log('[AUDIT:dev-fallback]', JSON.stringify(event, null, 2));
}

/**
 * Resolve the active org from the Better Auth session. audit-core REQUIRES
 * org_id, so events without an active org are dropped at publish time.
 */
function activeOrgFrom(ctx: {
  context: { session?: unknown };
}): string | undefined {
  const session = ctx.context.session as
    | { activeOrganizationId?: string }
    | undefined;
  return session?.activeOrganizationId;
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
            const path = context.path ?? '';
            return (
              path === '/sign-in/email' ||
              path === '/sign-in/oauth' ||
              path.startsWith('/oauth/')
            );
          },
          handler: createAuthMiddleware(async (ctx) => {
            // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
            const user = ctx.context.user;

            const session = ctx.context.newSession;

            if (user && session) {
              const method = ctx.path.includes('oauth') ? 'oauth' : 'email';
              const provider = ctx.path.includes('oauth')
                ? ctx.path.split('/').pop()
                : 'email';
              const ipAddress =
                ctx.request?.headers.get('x-forwarded-for') ||
                ctx.request?.headers.get('x-real-ip') ||
                'unknown';
              const userAgent =
                ctx.request?.headers.get('user-agent') || 'unknown';

              logAuditEvent({
                // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-member-access
                userId: user.id,
                sessionId: session.session?.id || 'unknown',
                action: 'SIGN_IN',
                resource: 'authentication',
                details: {
                  method,
                  provider,
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  userEmail: user.email,
                },
                ipAddress,
                userAgent,
                timestamp: new Date().toISOString(),
                success: true,
              });

              // Durable audit — no-ops when no active org is selected.
              const orgId = activeOrgFrom(ctx);
              if (orgId) {
                publishVelionAudit({
                  occurred_at: new Date().toISOString(),
                  org_id: orgId,
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  user_id: user.id as string,
                  plane: 'control',
                  event: 'sign_in',
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  subject: user.email as string,
                  outcome: 'ok',
                  details: { method, provider },
                  ip_address: ipAddress,
                  user_agent: userAgent,
                });
              }

              // Ensure async compliance for middleware
              await Promise.resolve();
            }
          }),
        },

        // Two-Factor Authentication events
        {
          matcher: (context) => {
            const path = context.path ?? '';
            return (
              path === '/two-factor/enable' ||
              path === '/two-factor/disable' ||
              path === '/two-factor/verify'
            );
          },
          handler: createAuthMiddleware(async (ctx) => {
            // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
            const user = ctx.context.user;
            const action = ctx.path.split('/').pop()?.toUpperCase();

            if (user && action) {
              const ipAddress =
                ctx.request?.headers.get('x-forwarded-for') ||
                ctx.request?.headers.get('x-real-ip') ||
                'unknown';
              const userAgent =
                ctx.request?.headers.get('user-agent') || 'unknown';

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
                ipAddress,
                userAgent,
                timestamp: new Date().toISOString(),
                success: true,
              });

              // Publish to velion.audit.v1.control.* only when org context is known.
              // Better Auth sets activeOrganizationId on the session when the user has
              // selected an active org; without it audit-core would reject the event.
              const orgId = activeOrgFrom(ctx);
              if (orgId) {
                const eventName =
                  action === 'ENABLE'
                    ? 'twofa_enable'
                    : action === 'DISABLE'
                      ? 'twofa_disable'
                      : 'twofa_verify';
                publishVelionAudit({
                  occurred_at: new Date().toISOString(),
                  org_id: orgId,
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  user_id: user.id as string,
                  plane: 'control',
                  event: eventName,
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  subject: user.email as string,
                  outcome: 'ok',
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  details: { method: ctx.body?.method || 'totp' },
                  ip_address: ipAddress,
                  user_agent: userAgent,
                });
              }

              // Ensure async compliance for middleware
              await Promise.resolve();
            }
          }),
        },

        // Session revocation events
        {
          matcher: (context) => {
            const path = context.path ?? '';
            return (
              path === '/revoke-sessions' ||
              path === '/revoke-other-sessions' ||
              path.includes('/revoke-session')
            );
          },
          handler: createAuthMiddleware(async (ctx) => {
            // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
            const user = ctx.context.user;

            if (user) {
              const ipAddress =
                ctx.request?.headers.get('x-forwarded-for') ||
                ctx.request?.headers.get('x-real-ip') ||
                'unknown';
              const userAgent =
                ctx.request?.headers.get('user-agent') || 'unknown';
              const revokeType = ctx.path.includes('other')
                ? 'others'
                : ctx.path.includes('sessions')
                  ? 'all'
                  : 'single';

              logAuditEvent({
                // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-member-access
                userId: user.id,
                action: 'SESSION_REVOKE',
                resource: 'session',
                details: {
                  type: revokeType,
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  userEmail: user.email,
                },
                ipAddress,
                userAgent,
                timestamp: new Date().toISOString(),
                success: true,
              });

              // Publish to velion.audit.v1.control.* only when org context is known.
              const orgId = activeOrgFrom(ctx);
              if (orgId) {
                publishVelionAudit({
                  occurred_at: new Date().toISOString(),
                  org_id: orgId,
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  user_id: user.id as string,
                  plane: 'control',
                  event: 'session_revoke',
                  // eslint-disable-next-line @typescript-eslint/no-unsafe-member-access
                  subject: user.email as string,
                  outcome: 'ok',
                  details: { revoke_type: revokeType },
                  ip_address: ipAddress,
                  user_agent: userAgent,
                });
              }

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
            const path = context.path ?? '';
            return (
              path === '/sign-in/email' ||
              path === '/sign-up/email' ||
              path.startsWith('/oauth/') ||
              path.includes('verify') ||
              path.includes('two-factor')
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
