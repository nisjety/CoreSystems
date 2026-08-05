import {
  BadRequestException,
  Controller,
  HttpCode,
  HttpException,
  HttpStatus,
  Param,
  Post,
  Req,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { and, eq } from 'drizzle-orm';
import type { Request as ExpressRequest } from 'express';

import { db } from '../db';
import { invitation, member } from '../db/schema';
import { auth } from './auth';
import {
  invitationAcceptanceInternalMarker,
  invitationActorRateLimitAddress,
} from './invitation-acceptance-rate-limit';
import { repairAcceptedInvitationForActor } from './invitation-acceptance-repair';

const INVITATION_ID = /^[A-Za-z0-9_-]{1,256}$/;

const invitationAuth = auth as unknown as {
  handler(request: globalThis.Request): Promise<Response>;
  api: {
    getSession(input: { headers: Headers }): Promise<{
      user: { id: string; email: string };
    } | null>;
  };
};

export interface AcceptedInvitationRecord {
  invitationId: string;
  invitationEmail: string;
  invitationOrganizationId: string;
  invitationStatus: string;
  invitationRole: string | null;
  memberId: string;
  memberUserId: string;
  memberOrganizationId: string;
  memberRole: string;
}

interface InvitationActor {
  userId: string;
  email: string;
}

export interface AcceptedInvitationResponse {
  invitation: { id: string; organizationId: string; status: 'accepted' };
  member: { id: string; organizationId: string; role: string };
}

export function acceptedInvitationRetryResponse(
  records: AcceptedInvitationRecord[],
  actor: InvitationActor,
): AcceptedInvitationResponse | null {
  if (records.length !== 1) return null;
  const record = records[0];
  if (
    record.invitationStatus !== 'accepted' ||
    record.invitationEmail.trim().toLowerCase() !==
      actor.email.trim().toLowerCase() ||
    record.memberUserId !== actor.userId ||
    record.invitationOrganizationId !== record.memberOrganizationId
  ) {
    return null;
  }

  return {
    invitation: {
      id: record.invitationId,
      organizationId: record.invitationOrganizationId,
      status: 'accepted',
    },
    member: {
      id: record.memberId,
      organizationId: record.memberOrganizationId,
      role: record.memberRole,
    },
  };
}

function toWebHeaders(
  source: Record<string, string | string[] | undefined>,
): Headers {
  const headers = new Headers();
  for (const [key, value] of Object.entries(source)) {
    if (typeof value === 'string') {
      headers.set(key, value);
    } else if (Array.isArray(value)) {
      headers.set(key, value.join(', '));
    }
  }
  return headers;
}

async function lookupAcceptedInvitation(
  invitationId: string,
  userId: string,
): Promise<AcceptedInvitationRecord[]> {
  return db
    .select({
      invitationId: invitation.id,
      invitationEmail: invitation.email,
      invitationOrganizationId: invitation.organizationId,
      invitationStatus: invitation.status,
      invitationRole: invitation.role,
      memberId: member.id,
      memberUserId: member.userId,
      memberOrganizationId: member.organizationId,
      memberRole: member.role,
    })
    .from(invitation)
    .innerJoin(
      member,
      and(
        eq(member.organizationId, invitation.organizationId),
        eq(member.userId, userId),
      ),
    )
    .where(
      and(eq(invitation.id, invitationId), eq(invitation.status, 'accepted')),
    )
    .limit(2);
}

function acceptanceRouterUrl(): string {
  const configuredOrigin =
    process.env.BETTER_AUTH_URL?.trim() || 'http://localhost:3011';
  return new URL(
    '/api/auth/organization/accept-invitation',
    configuredOrigin,
  ).toString();
}

async function callAcceptanceRouter(
  headers: Headers,
  invitationId: string,
  rateLimitAddress: string,
  internalMarker: string,
): Promise<Response> {
  const routerHeaders = new Headers(headers);
  routerHeaders.set('content-type', 'application/json');
  // Never trust or reuse a browser-supplied forwarding header for Auth's rate
  // key. This value is derived from the already-verified session actor.
  for (const name of [
    'cf-connecting-ip',
    'x-forwarded-for',
    'x-real-ip',
    'true-client-ip',
  ]) {
    routerHeaders.set(name, rateLimitAddress);
  }
  routerHeaders.set('x-verevon-invitation-acceptance', internalMarker);
  return invitationAuth.handler(
    new globalThis.Request(acceptanceRouterUrl(), {
      method: 'POST',
      headers: routerHeaders,
      body: JSON.stringify({ invitationId }),
    }),
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function parseAcceptedResponse(
  value: unknown,
  invitationId: string,
): AcceptedInvitationResponse | null {
  if (
    !isRecord(value) ||
    !isRecord(value.invitation) ||
    !isRecord(value.member)
  ) {
    return null;
  }
  const acceptedInvitation = value.invitation;
  const acceptedMember = value.member;
  if (
    acceptedInvitation.id !== invitationId ||
    typeof acceptedInvitation.organizationId !== 'string' ||
    !acceptedInvitation.organizationId ||
    acceptedInvitation.status !== 'accepted' ||
    typeof acceptedMember.id !== 'string' ||
    !acceptedMember.id ||
    acceptedMember.organizationId !== acceptedInvitation.organizationId ||
    typeof acceptedMember.role !== 'string' ||
    !acceptedMember.role
  ) {
    return null;
  }
  return {
    invitation: {
      id: invitationId,
      organizationId: acceptedInvitation.organizationId,
      status: 'accepted',
    },
    member: {
      id: acceptedMember.id,
      organizationId: acceptedInvitation.organizationId,
      role: acceptedMember.role,
    },
  };
}

function throwNormalizedAcceptanceFailure(status: number | null): never {
  if (status === HttpStatus.UNAUTHORIZED) {
    throw new UnauthorizedException('Authentication required');
  }
  if (status === HttpStatus.TOO_MANY_REQUESTS) {
    throw new HttpException(
      {
        code: 'RATE_LIMITED',
        message: 'Too many invitation attempts.',
      },
      HttpStatus.TOO_MANY_REQUESTS,
    );
  }
  if (status === null || status >= 500) {
    throw new ServiceUnavailableException({
      code: 'INVITATION_AUTHORITY_UNAVAILABLE',
      message: 'Invitation authority is unavailable.',
    });
  }
  throw new BadRequestException({
    code: 'INVITATION_NOT_FOUND',
    message: 'Invitation is invalid, expired, or unavailable.',
  });
}

@Controller('api/v1/organization/invitations')
export class InvitationAcceptanceController {
  @Post(':invitationId/accept')
  @HttpCode(HttpStatus.OK)
  async accept(
    @Param('invitationId') rawInvitationId: string,
    @Req() request: ExpressRequest,
  ): Promise<unknown> {
    const invitationId = rawInvitationId.trim();
    if (!INVITATION_ID.test(invitationId)) {
      throw new BadRequestException({
        code: 'VALIDATION_ERROR',
        message: 'Invitation id must be a bounded opaque identifier.',
      });
    }

    const headers = toWebHeaders(request.headers);
    let session: Awaited<
      ReturnType<typeof invitationAuth.api.getSession>
    > | null;
    try {
      session = await invitationAuth.api.getSession({ headers });
    } catch {
      throw new ServiceUnavailableException({
        code: 'INVITATION_AUTHORITY_UNAVAILABLE',
        message: 'Invitation authority is unavailable.',
      });
    }
    if (!session?.user?.id || !session.user.email) {
      throw new UnauthorizedException('Authentication required');
    }
    let rateLimitAddress: string;
    let internalMarker: string;
    try {
      const authSecret = process.env.BETTER_AUTH_SECRET ?? '';
      rateLimitAddress = invitationActorRateLimitAddress(
        session.user.id,
        authSecret,
      );
      internalMarker = invitationAcceptanceInternalMarker(
        invitationId,
        authSecret,
      );
    } catch {
      throw new ServiceUnavailableException({
        code: 'INVITATION_AUTHORITY_UNAVAILABLE',
        message: 'Invitation authority is unavailable.',
      });
    }

    let routerResponse: Response | null = null;
    try {
      routerResponse = await callAcceptanceRouter(
        headers,
        invitationId,
        rateLimitAddress,
        internalMarker,
      );
      if (routerResponse.ok) {
        const accepted = parseAcceptedResponse(
          await routerResponse.json().catch(() => null),
          invitationId,
        );
        if (accepted) return accepted;
        throwNormalizedAcceptanceFailure(null);
      }
    } catch {
      routerResponse = null;
    }
    // A rate-limited mutation must not fall through to the recovery query: that
    // would turn a bounded attempt into an unbounded database-read primitive.
    if (routerResponse?.status === HttpStatus.TOO_MANY_REQUESTS) {
      throwNormalizedAcceptanceFailure(HttpStatus.TOO_MANY_REQUESTS);
    }
    if (routerResponse?.status === HttpStatus.UNAUTHORIZED) {
      throwNormalizedAcceptanceFailure(HttpStatus.UNAUTHORIZED);
    }

    try {
      const repaired = await repairAcceptedInvitationForActor(invitationId, {
        userId: session.user.id,
        email: session.user.email,
      });
      if (repaired) {
        return {
          invitation: {
            id: repaired.invitationId,
            organizationId: repaired.organizationId,
            status: 'accepted',
          },
          member: {
            id: repaired.memberId,
            organizationId: repaired.organizationId,
            role: repaired.memberRole,
          },
        } satisfies AcceptedInvitationResponse;
      }
    } catch {
      throw new ServiceUnavailableException({
        code: 'INVITATION_AUTHORITY_UNAVAILABLE',
        message: 'Invitation authority is unavailable.',
      });
    }

    let records: AcceptedInvitationRecord[];
    try {
      records = await lookupAcceptedInvitation(invitationId, session.user.id);
    } catch {
      throw new ServiceUnavailableException({
        code: 'INVITATION_AUTHORITY_UNAVAILABLE',
        message: 'Invitation authority is unavailable.',
      });
    }
    const recovered = acceptedInvitationRetryResponse(records, {
      userId: session.user.id,
      email: session.user.email,
    });
    if (recovered) return recovered;
    throwNormalizedAcceptanceFailure(routerResponse?.status ?? null);
  }
}
