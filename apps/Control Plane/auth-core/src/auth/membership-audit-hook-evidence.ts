export type MembershipAuditHookAction = 'role_changed' | 'member_removed';

export type VerifiedMembershipAuditHookEvidence = Readonly<{
  action: MembershipAuditHookAction;
  actorUserId: string;
  expectedRevision: number;
  member: Readonly<{
    id: string;
    organizationId: string;
    userId: string;
    role: string;
  }>;
}>;

function nonEmptyString(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null;
}

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object'
    ? (value as Record<string, unknown>)
    : null;
}

function positiveSafeInteger(value: unknown): number | null {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0
    ? value
    : null;
}

function verifiedSessionActor(context: Record<string, unknown>): string | null {
  const session = record(context.session);
  const user = record(session?.user);
  const sessionRecord = record(session?.session);
  const userClaim = nonEmptyString(user?.id);
  const sessionClaim = nonEmptyString(sessionRecord?.userId);
  if (!userClaim || !sessionClaim || userClaim !== sessionClaim) return null;
  return userClaim;
}

export function verifiedMembershipAuditSessionActorUserId(
  session: unknown,
): string | null {
  return verifiedSessionActor({ session });
}

export function verifiedMembershipAuditSessionToken(
  session: unknown,
): string | null {
  if (!verifiedMembershipAuditSessionActorUserId(session)) return null;
  const sessionEnvelope = record(session);
  const sessionRecord = record(sessionEnvelope?.session);
  return nonEmptyString(sessionRecord?.token);
}

export function membershipAuditBearerSessionToken(
  authorization: unknown,
): string | null {
  if (typeof authorization !== 'string') return null;
  const match = /^Bearer ([^\s,]{16,2048})$/i.exec(authorization);
  return match ? match[1] : null;
}

function authoritativeSessionExpiresAtMillis(
  authoritativeSession: unknown,
): number {
  const authoritativeEnvelope = record(authoritativeSession);
  const authoritativeRecord = record(authoritativeEnvelope?.session);
  const expiresAt = authoritativeRecord?.expiresAt;
  return expiresAt instanceof Date
    ? expiresAt.getTime()
    : typeof expiresAt === 'string' || typeof expiresAt === 'number'
      ? new Date(expiresAt).getTime()
      : Number.NaN;
}

export function verifiedAuthoritativeMembershipAuditCredentialActorUserId(
  credentialSessionToken: unknown,
  authoritativeSession: unknown,
  now = Date.now(),
): string | null {
  const credentialToken = nonEmptyString(credentialSessionToken);
  const authoritativeActor =
    verifiedMembershipAuditSessionActorUserId(authoritativeSession);
  const authoritativeToken =
    verifiedMembershipAuditSessionToken(authoritativeSession);
  const expiresAtMillis =
    authoritativeSessionExpiresAtMillis(authoritativeSession);
  if (
    !credentialToken ||
    credentialToken !== authoritativeToken ||
    !authoritativeActor ||
    !Number.isFinite(expiresAtMillis) ||
    expiresAtMillis <= now
  ) {
    return null;
  }
  return authoritativeActor;
}

export function verifiedAuthoritativeMembershipAuditActorUserId(
  requestSession: unknown,
  authoritativeSession: unknown,
  now = Date.now(),
): string | null {
  const requestActor =
    verifiedMembershipAuditSessionActorUserId(requestSession);
  const authoritativeActor =
    verifiedMembershipAuditSessionActorUserId(authoritativeSession);
  const requestToken = verifiedMembershipAuditSessionToken(requestSession);
  const verifiedAuthoritativeActor =
    verifiedAuthoritativeMembershipAuditCredentialActorUserId(
      requestToken,
      authoritativeSession,
      now,
    );

  if (
    !requestActor ||
    requestActor !== authoritativeActor ||
    requestActor !== verifiedAuthoritativeActor
  ) {
    return null;
  }
  return verifiedAuthoritativeActor;
}

export function verifiedMembershipAuditActorUserId(
  hookContext: unknown,
): string | null {
  const hook = record(hookContext);
  const context = record(hook?.context);
  return context ? verifiedSessionActor(context) : null;
}

function returnedMember(
  returned: unknown,
): VerifiedMembershipAuditHookEvidence['member'] | null {
  const response = record(returned);
  const candidate = record(response?.member) ?? response;
  const id = nonEmptyString(candidate?.id);
  const organizationId = nonEmptyString(candidate?.organizationId);
  const userId = nonEmptyString(candidate?.userId);
  const role = nonEmptyString(candidate?.role);
  if (!id || !organizationId || !userId || !role) return null;
  return Object.freeze({ id, organizationId, userId, role });
}

function membershipMutationEvidence(hookContext: unknown): Readonly<{
  action: MembershipAuditHookAction;
  member: VerifiedMembershipAuditHookEvidence['member'];
}> | null {
  const hook = record(hookContext);
  const context = record(hook?.context);
  const action =
    hook?.path === '/organization/update-member-role'
      ? 'role_changed'
      : hook?.path === '/organization/remove-member'
        ? 'member_removed'
        : null;
  if (!action) return null;

  const member = returnedMember(context?.returned);
  return member ? Object.freeze({ action, member }) : null;
}

function verifiedEvidenceForActor(
  hookContext: unknown,
  actorUserId: string,
): VerifiedMembershipAuditHookEvidence | null {
  const hook = record(hookContext);
  const operation = record(hook?.membershipAuditOperation);
  const operationActor = nonEmptyString(operation?.actorUserId);
  const expectedRevision = positiveSafeInteger(operation?.expectedRevision);
  const operationMember = returnedMember(operation?.member);
  const mutation = membershipMutationEvidence(hookContext);
  if (
    !operationActor ||
    operationActor !== actorUserId ||
    !expectedRevision ||
    operation?.mutationExpected !== true ||
    !operationMember ||
    !mutation ||
    operation?.action !== mutation.action ||
    operationMember.id !== mutation.member.id ||
    operationMember.organizationId !== mutation.member.organizationId ||
    operationMember.userId !== mutation.member.userId ||
    operationMember.role !== mutation.member.role
  ) {
    return null;
  }
  return Object.freeze({ ...mutation, actorUserId, expectedRevision });
}

export function verifiedMembershipAuditHookEvidence(
  hookContext: unknown,
): VerifiedMembershipAuditHookEvidence | null {
  const actorUserId = verifiedMembershipAuditActorUserId(hookContext);
  if (!actorUserId) return null;
  return verifiedEvidenceForActor(hookContext, actorUserId);
}

export async function readVerifiedMembershipAuditHookEvidence(
  hookContext: unknown,
  resolveVerifiedSession?: () => Promise<unknown>,
): Promise<VerifiedMembershipAuditHookEvidence | null> {
  const hook = record(hookContext);
  const context = record(hook?.context);
  const returned = context?.returned;
  let normalizedHookContext = hookContext;
  if (returned instanceof Response) {
    if (returned.status !== 200) return null;

    try {
      const responseBody: unknown = await returned.clone().json();
      normalizedHookContext = Object.freeze({
        ...hook,
        context: Object.freeze({ ...context, returned: responseBody }),
      });
    } catch {
      return null;
    }
  }

  if (!membershipMutationEvidence(normalizedHookContext)) return null;

  if (resolveVerifiedSession) {
    try {
      const verifiedSession = await resolveVerifiedSession();
      const actorUserId =
        verifiedMembershipAuditSessionActorUserId(verifiedSession);
      return actorUserId
        ? verifiedEvidenceForActor(normalizedHookContext, actorUserId)
        : null;
    } catch {
      return null;
    }
  }

  return verifiedMembershipAuditHookEvidence(normalizedHookContext);
}
