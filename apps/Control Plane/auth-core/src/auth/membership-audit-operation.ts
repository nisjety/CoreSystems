import {
  normalizeOutboxRevision,
  type OutboxRevision,
} from './outbox-revision';
import type { MembershipAuditHookAction } from './membership-audit-hook-evidence';

export type MembershipAuditOperationMember = Readonly<{
  id: string;
  organizationId: string;
  userId: string;
  role: string;
}>;

export type CapturedMembershipAuditOperation = Readonly<{
  action: MembershipAuditHookAction;
  kind: 'role_change' | 'admin_remove' | 'self_leave';
  actorUserId: string;
  observedRevision: number;
  mutationExpected: boolean;
  previousRole: string;
  member: MembershipAuditOperationMember;
}>;

export type MembershipAuditMemberSnapshot = Readonly<{
  id: string;
  organizationId: string;
  userId: string;
  role: string;
  revision: OutboxRevision;
}>;

export type FindMembershipAuditMember = (
  organizationId: string,
  memberIdOrEmail: string,
) => Promise<ReadonlyArray<MembershipAuditMemberSnapshot>>;

export type MembershipAuditOperationCaptureResult =
  | Readonly<{
      status: 'captured';
      operation: CapturedMembershipAuditOperation;
    }>
  | Readonly<{ status: 'not_found' }>
  | Readonly<{ status: 'bad_request' }>
  | Readonly<{ status: 'invalid' }>;

const CANONICAL_PROJECT_ROLES = new Set(['owner', 'admin', 'member', 'viewer']);
const MEMBERSHIP_MUTATION_PATHS = new Set([
  '/organization/update-member-role',
  '/organization/remove-member',
  '/organization/leave',
]);

export function isMembershipMutationPath(path: unknown): path is string {
  return typeof path === 'string' && MEMBERSHIP_MUTATION_PATHS.has(path);
}

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object'
    ? (value as Record<string, unknown>)
    : null;
}

function boundedString(value: unknown, maxLength: number): string | null {
  if (typeof value !== 'string') return null;
  const normalized = value.trim();
  return normalized && normalized.length <= maxLength ? normalized : null;
}

function canonicalProjectRole(value: unknown): string | null {
  if (typeof value !== 'string' || value.includes(',')) return null;
  const normalized = value.trim().toLowerCase();
  return CANONICAL_PROJECT_ROLES.has(normalized) ? normalized : null;
}

export async function captureMembershipAuditOperationResult(
  hookContext: unknown,
  actorUserIdValue: unknown,
  findMember: FindMembershipAuditMember,
): Promise<MembershipAuditOperationCaptureResult> {
  const hook = record(hookContext);
  const body = record(hook?.body);
  const actorUserId = boundedString(actorUserIdValue, 255);
  const organizationId = boundedString(body?.organizationId, 255);
  const action: MembershipAuditHookAction | null =
    hook?.path === '/organization/update-member-role'
      ? 'role_changed'
      : hook?.path === '/organization/remove-member' ||
          hook?.path === '/organization/leave'
        ? 'member_removed'
        : null;
  const kind: CapturedMembershipAuditOperation['kind'] | null =
    hook?.path === '/organization/update-member-role'
      ? 'role_change'
      : hook?.path === '/organization/remove-member'
        ? 'admin_remove'
        : hook?.path === '/organization/leave'
          ? 'self_leave'
          : null;
  const memberIdOrEmail = boundedString(
    hook?.path === '/organization/leave'
      ? actorUserId
      : action === 'role_changed'
        ? body?.memberId
        : body?.memberIdOrEmail,
    320,
  );
  if (!actorUserId || !organizationId || !action || !kind || !memberIdOrEmail) {
    return Object.freeze({ status: 'invalid' });
  }
  const requestedRole =
    action === 'role_changed' ? canonicalProjectRole(body?.role) : null;
  if (action === 'role_changed' && !requestedRole) {
    return Object.freeze({ status: 'bad_request' });
  }

  const rows = await findMember(organizationId, memberIdOrEmail);
  if (rows.length === 0) return Object.freeze({ status: 'not_found' });
  if (rows.length !== 1) return Object.freeze({ status: 'invalid' });
  const snapshot = rows[0];
  const id = boundedString(snapshot.id, 255);
  const snapshotOrganizationId = boundedString(snapshot.organizationId, 255);
  const userId = boundedString(snapshot.userId, 255);
  const previousRole = boundedString(snapshot.role, 64);
  if (
    !id ||
    snapshotOrganizationId !== organizationId ||
    !userId ||
    !previousRole
  ) {
    return Object.freeze({ status: 'invalid' });
  }

  const role = action === 'role_changed' ? requestedRole : previousRole;
  if (!role) return Object.freeze({ status: 'invalid' });

  try {
    const currentRevision = normalizeOutboxRevision(snapshot.revision);
    const mutationExpected =
      action === 'member_removed' || role !== previousRole;
    // A same-role request still acquires the next-revision guard. Without it,
    // a concurrent role change can turn the apparent no-op into a real stale
    // write before Better Auth executes the handler.
    if (!Number.isSafeInteger(currentRevision + 1)) {
      return Object.freeze({ status: 'invalid' });
    }
    return Object.freeze({
      status: 'captured',
      operation: Object.freeze({
        action,
        kind,
        actorUserId,
        observedRevision: currentRevision,
        mutationExpected,
        previousRole,
        member: Object.freeze({
          id,
          organizationId,
          userId,
          role,
        }),
      }),
    });
  } catch {
    return Object.freeze({ status: 'invalid' });
  }
}

export async function captureMembershipAuditOperation(
  hookContext: unknown,
  actorUserIdValue: unknown,
  findMember: FindMembershipAuditMember,
): Promise<CapturedMembershipAuditOperation | null> {
  const result = await captureMembershipAuditOperationResult(
    hookContext,
    actorUserIdValue,
    findMember,
  );
  return result.status === 'captured' ? result.operation : null;
}
