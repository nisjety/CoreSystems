function getExpectedServiceKey() {
  // Fail closed: no hardcoded default (a leaked image with no env set must NOT
  // authenticate against a public string). Control-Plane-owned shared key.
  const key =
    process.env.CONVEX_INTERNAL_SERVICE_KEY || process.env.INTERNAL_API_KEY;
  if (!key) {
    throw new Error('CONVEX_INTERNAL_SERVICE_KEY (or INTERNAL_API_KEY) must be set');
  }
  return key;
}

export function assertServiceKey(serviceKey: string) {
  if (!serviceKey || serviceKey !== getExpectedServiceKey()) {
    throw new Error('Unauthorized');
  }
}

export async function requireIdentity(ctx: any) {
  const identity = await ctx.auth.getUserIdentity();
  if (!identity) {
    throw new Error('Not authenticated');
  }

  const externalAuthId = identity['properties.externalAuthId'];
  if (typeof externalAuthId !== 'string' || externalAuthId.length === 0) {
    throw new Error('Invalid auth identity');
  }

  return {
    identity,
    externalAuthId,
  };
}

export async function requireIdentityForExternalUser(
  ctx: any,
  requestedExternalAuthId: string,
) {
  const viewer = await requireIdentity(ctx);
  if (viewer.externalAuthId !== requestedExternalAuthId) {
    throw new Error('Unauthorized');
  }
  return viewer;
}

export async function requireViewerMembership(ctx: any, externalOrgId: string) {
  const { identity, externalAuthId } = await requireIdentity(ctx);

  const organizations = await ctx.db
    .query('organizations')
    .withIndex('by_external_id', (q: any) => q.eq('externalOrgId', externalOrgId))
    .collect();

  const organization = organizations.find(
    (candidate: any) => candidate.syncStatus !== 'deleted',
  );

  if (!organization) {
    throw new Error('Organization not found');
  }

  const memberships = await ctx.db
    .query('users')
    .withIndex('by_external_and_org', (q: any) =>
      q.eq('externalAuthId', externalAuthId).eq('orgId', organization._id),
    )
    .collect();

  const membership = memberships.find(
    (candidate: any) => candidate.syncStatus !== 'deleted',
  );

  if (!membership) {
    throw new Error('Unauthorized');
  }

  return {
    identity,
    externalAuthId,
    organization,
    membership,
  };
}

export async function requireViewerMembershipByOrgId(ctx: any, orgId: string) {
  const { identity, externalAuthId } = await requireIdentity(ctx);
  const organization = await ctx.db.get(orgId);
  if (!organization || organization.syncStatus === 'deleted') {
    throw new Error('Unauthorized');
  }
  const memberships = await ctx.db
    .query('users')
    .withIndex('by_external_and_org', (q: any) =>
      q.eq('externalAuthId', externalAuthId).eq('orgId', organization._id),
    )
    .collect();
  const membership = memberships.find(
    (candidate: any) => candidate.syncStatus !== 'deleted',
  );
  if (!membership) throw new Error('Unauthorized');
  return { identity, externalAuthId, organization, membership };
}

export async function requireEditorMembership(ctx: any, externalOrgId: string) {
  const viewer = await requireViewerMembership(ctx, externalOrgId);
  if (!['admin', 'member'].includes(viewer.membership.role)) {
    throw new Error('Unauthorized');
  }
  return viewer;
}

export async function requireEditorMembershipByOrgId(ctx: any, orgId: string) {
  const viewer = await requireViewerMembershipByOrgId(ctx, orgId);
  if (!['admin', 'member'].includes(viewer.membership.role)) {
    throw new Error('Unauthorized');
  }
  return viewer;
}

/**
 * Membership check for the service-key gateway path, where Convex identity is
 * unavailable: the BFF holds a Control session, not a Convex one, so the caller
 * is presented as an `externalAuthId` + `externalOrgId` pair that this function
 * must verify against the projection rather than trust.
 *
 * `assertServiceKey` proves the *caller* is the gateway; this proves the
 * *subject* it is acting for really belongs to the org. Both are required —
 * the service key alone would let any gateway route read any org.
 */
export async function requireGatewayMember(
  ctx: any,
  externalAuthId: string,
  externalOrgId: string,
) {
  const organizations = await ctx.db
    .query('organizations')
    .withIndex('by_external_id', (q: any) => q.eq('externalOrgId', externalOrgId))
    .collect();
  const organization = organizations.find((candidate: any) => candidate.syncStatus !== 'deleted');
  if (!organization) throw new Error('Organization not found');
  const members = await ctx.db
    .query('users')
    .withIndex('by_external_and_org', (q: any) =>
      q.eq('externalAuthId', externalAuthId).eq('orgId', organization._id),
    )
    .collect();
  if (!members.some((candidate: any) => candidate.syncStatus !== 'deleted')) {
    throw new Error('Unauthorized');
  }
  return organization;
}

export async function requireConversationViewer(
  ctx: any,
  conversationId: string,
  externalOrgId: string,
) {
  const viewer = await requireViewerMembership(ctx, externalOrgId);
  const conversation = await ctx.db.get(conversationId);

  if (!conversation || conversation.status === 'deleted') {
    throw new Error('Conversation not found');
  }

  if (
    conversation.orgId !== viewer.organization._id ||
    conversation.userId !== viewer.membership._id
  ) {
    throw new Error('Unauthorized');
  }

  return {
    ...viewer,
    conversation,
  };
}
