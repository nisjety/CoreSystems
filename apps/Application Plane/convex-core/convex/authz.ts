const DEFAULT_INTERNAL_SERVICE_KEY = 'change-me-internal-service-secret';

function getExpectedServiceKey() {
  return (
    process.env.CONVEX_INTERNAL_SERVICE_KEY ||
    process.env.INTERNAL_API_KEY ||
    DEFAULT_INTERNAL_SERVICE_KEY
  );
}

export function assertServiceKey(serviceKey: string) {
  if (!serviceKey || serviceKey !== getExpectedServiceKey()) {
    throw new Error('Unauthorized');
  }
}

async function requireIdentity(ctx: any) {
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
