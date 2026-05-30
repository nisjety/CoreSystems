/**
 * Fastify preHandler hooks that gate routes by organization plan tier
 * and feature entitlements.
 *
 * Install AFTER auth middleware — requires `request.principal` to be populated.
 * Internal callers (`request.isInternalCall === true`) always bypass.
 */
import { FastifyReply, FastifyRequest } from 'fastify';

import { HttpError } from '../http/http-error';
import { OrgClient, PlanTier } from './org-client';

// ─── Plan ranking ─────────────────────────────────────────────────────────────

const planRank: Record<PlanTier, number> = {
  free: 0,
  starter: 1,
  pro: 2,
  enterprise: 3
};

// ─── requirePlan ──────────────────────────────────────────────────────────────

/**
 * Returns a Fastify preHandler that rejects requests from organizations
 * whose plan is lower than `minimumPlan`.
 *
 * Internal service calls always pass through.
 *
 * On rejection returns 403 with:
 * ```json
 * { "code": "PLAN_REQUIRED", "requiredPlan": "pro", "currentPlan": "free" }
 * ```
 */
export function requirePlan(
  orgClient: OrgClient,
  minimumPlan: PlanTier
): (request: FastifyRequest, reply: FastifyReply) => Promise<void> {
  const requiredRank = planRank[minimumPlan];

  return async (request: FastifyRequest, reply: FastifyReply): Promise<void> => {
    // Internal service calls bypass plan checks.
    if (request.isInternalCall) {
      return;
    }

    const principal = request.principal;

    if (!principal) {
      throw new HttpError(401, 'unauthorized', 'Authentication required');
    }

    const orgId = principal.organizationId;
    const userId = principal.userId;

    if (!orgId) {
      throw new HttpError(403, 'PLAN_REQUIRED', 'Organization context is missing from principal', {
        requiredPlan: minimumPlan
      });
    }

    const orgPlan = await orgClient.getOrgPlan(orgId, userId);
    const currentRank = planRank[orgPlan.plan] ?? 0;

    if (currentRank < requiredRank) {
      throw new HttpError(403, 'PLAN_REQUIRED', `This feature requires a ${minimumPlan} plan or higher`, {
        requiredPlan: minimumPlan,
        currentPlan: orgPlan.plan
      });
    }

    void reply;
  };
}

// ─── requireEntitlement ───────────────────────────────────────────────────────

/**
 * Returns a Fastify preHandler that rejects requests when the organization
 * does not have the given feature entitlement enabled.
 *
 * Internal service calls always pass through.
 */
export function requireEntitlement(
  orgClient: OrgClient,
  feature: string
): (request: FastifyRequest, reply: FastifyReply) => Promise<void> {
  return async (request: FastifyRequest, reply: FastifyReply): Promise<void> => {
    if (request.isInternalCall) {
      return;
    }

    const principal = request.principal;

    if (!principal) {
      throw new HttpError(401, 'unauthorized', 'Authentication required');
    }

    const orgId = principal.organizationId;
    const userId = principal.userId;

    if (!orgId) {
      throw new HttpError(403, 'ENTITLEMENT_REQUIRED', 'Organization context is missing', {
        requiredFeature: feature
      });
    }

    const orgPlan = await orgClient.getOrgPlan(orgId, userId);

    if (!orgPlan.entitlements[feature]) {
      throw new HttpError(403, 'ENTITLEMENT_REQUIRED', `Feature "${feature}" is not enabled for this organization`, {
        requiredFeature: feature,
        currentPlan: orgPlan.plan
      });
    }

    void reply;
  };
}
