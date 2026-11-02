import {
  BadRequestException,
  Body,
  ConflictException,
  Controller,
  Delete,
  Get,
  Headers,
  HttpCode,
  HttpStatus,
  Logger,
  NotFoundException,
  Param,
  Post,
  Query,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { createHash, randomUUID, timingSafeEqual } from 'node:crypto';
import { and, asc, eq, gt, inArray, sql } from 'drizzle-orm';

import { db } from '../db';
import { member, ssoProvider, ssoSigninEvidence, user } from '../db/schema';
import { auth } from './auth';
import { emailDomain, isOrganizationAdmin } from './sso-enforcement';

/**
 * Internal SSO administration (WS05-S7), called by org-core only.
 *
 * org-core owns the SSO policy and the proof of the e-mail domain; auth-core
 * owns the `sso_provider` rows, the sign-in evidence and the sessions. Provider
 * registration is reachable through this surface alone: the plugin's own
 * /sso/register, /sso/update-provider and /sso/delete-provider paths are
 * disabled in auth.ts (`disabledPaths`), so a provider can only be added for a
 * domain org-core has verified for the organisation.
 *
 * Authentication is the dedicated ORG_CORE_IDENTITY_SERVICE_TOKEN credential in
 * `x-sso-admin-token` (the pattern of org-group-grant.controller.ts). The
 * client secret is write-only: it is stored, never returned, never logged.
 */

const PROVIDER_ID = /^[a-z0-9][a-z0-9-]{2,62}$/;
const DOMAIN = /^(?=.{1,253}$)([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$/;
const MAX_FIELD = 512;
const MAX_USER_IDS = 10;
const MAX_REVOKE_BATCH = 200;
const MAX_EVIDENCE_WINDOW_SECONDS = 7 * 24 * 3600;

function credentialMatches(expected: string, received?: string): boolean {
  const expectedDigest = createHash('sha256').update(expected).digest();
  const receivedDigest = createHash('sha256')
    .update((received ?? '').trim())
    .digest();
  return timingSafeEqual(expectedDigest, receivedDigest);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function text(value: unknown, max = MAX_FIELD): string {
  return typeof value === 'string' ? value.trim().slice(0, max) : '';
}

function stringList(value: unknown, max: number): string[] {
  if (!Array.isArray(value) || value.length > max) {
    throw new BadRequestException('a bounded list of strings is required');
  }
  return value.map((entry) => {
    const item = text(entry, 128);
    if (!item) throw new BadRequestException('list entries must not be empty');
    return item;
  });
}

function validIssuer(raw: string): boolean {
  try {
    const url = new URL(raw);
    return (
      url.protocol === 'https:' &&
      !url.username &&
      !url.password &&
      !url.search &&
      !url.hash &&
      url.hostname !== ''
    );
  } catch {
    return false;
  }
}

function providerView(row: {
  providerId: string;
  domain: string;
  issuer: string;
  oidcConfig: string | null;
  samlConfig: string | null;
}) {
  return {
    providerId: row.providerId,
    domain: row.domain,
    issuer: row.issuer,
    type: row.oidcConfig ? 'oidc' : row.samlConfig ? 'saml' : 'unknown',
  };
}

const sessionAuth = auth as unknown as {
  $context: Promise<{
    internalAdapter: {
      deleteSessions(userIdOrTokens: string | string[]): Promise<void>;
    };
  }>;
};

@Controller('api/v1/internal/sso')
export class SsoProviderAdminController {
  private readonly logger = new Logger(SsoProviderAdminController.name);
  private readonly expectedToken = (
    process.env.ORG_CORE_IDENTITY_SERVICE_TOKEN ?? ''
  ).trim();

  private authenticate(callerToken: string | undefined): void {
    if (this.expectedToken.length < 32) {
      // Fail closed rather than accepting every caller when unconfigured.
      this.logger.error(
        'ORG_CORE_IDENTITY_SERVICE_TOKEN is not configured; refusing SSO administration',
      );
      throw new ServiceUnavailableException('SSO administration unavailable');
    }
    if (!credentialMatches(this.expectedToken, callerToken)) {
      throw new UnauthorizedException('Valid SSO administration credential required');
    }
  }

  private unavailable(action: string, error: unknown): never {
    this.logger.error(
      `sso ${action} failed: ${error instanceof Error ? error.name : 'unknown'}`,
    );
    throw new ServiceUnavailableException('SSO administration unavailable');
  }

  @Get('providers')
  async list(
    @Headers('x-sso-admin-token') token: string | undefined,
    @Query('organizationId') organizationId: string | undefined,
  ) {
    this.authenticate(token);
    const orgId = text(organizationId, 128);
    if (!orgId) throw new BadRequestException('organizationId is required');
    try {
      const rows = await db
        .select({
          providerId: ssoProvider.providerId,
          domain: ssoProvider.domain,
          issuer: ssoProvider.issuer,
          oidcConfig: ssoProvider.oidcConfig,
          samlConfig: ssoProvider.samlConfig,
        })
        .from(ssoProvider)
        .where(eq(ssoProvider.organizationId, orgId));
      return { providers: rows.map(providerView) };
    } catch (error) {
      this.unavailable('list', error);
    }
  }

  @Post('providers')
  @HttpCode(HttpStatus.CREATED)
  async register(
    @Headers('x-sso-admin-token') token: string | undefined,
    @Body() body: unknown,
  ) {
    this.authenticate(token);
    if (!isRecord(body)) throw new BadRequestException('an object is required');
    const organizationId = text(body.organizationId, 128);
    const actorUserId = text(body.actorUserId, 128);
    const providerId = text(body.providerId, 64).toLowerCase();
    const domain = text(body.domain, 253).toLowerCase();
    const issuer = text(body.issuer, 2048);
    const clientId = text(body.clientId);
    const clientSecret = text(body.clientSecret);
    if (!organizationId || !actorUserId || !clientId || !clientSecret) {
      throw new BadRequestException('missing required fields');
    }
    if (!PROVIDER_ID.test(providerId) || !DOMAIN.test(domain)) {
      throw new BadRequestException('providerId or domain is not valid');
    }
    if (!validIssuer(issuer)) {
      throw new BadRequestException({
        code: 'invalid_issuer',
        message: 'The issuer must be an https url.',
      });
    }

    try {
      // Defence in depth: org-core has already checked the role and the domain
      // proof; the actor must still be an owner or admin of the organisation.
      if (!(await isOrganizationAdmin(organizationId, actorUserId))) {
        throw new UnauthorizedException('actor is not an administrator');
      }
      const [clash] = await db
        .select({ id: ssoProvider.id })
        .from(ssoProvider)
        .where(
          sql`${ssoProvider.providerId} = ${providerId} OR lower(${ssoProvider.domain}) = ${domain}`,
        )
        .limit(1);
      if (clash) {
        throw new ConflictException({
          code: 'provider_exists',
          message: 'A provider with this id or domain already exists.',
        });
      }
      const oidcConfig = {
        issuer,
        pkce: true,
        clientId,
        clientSecret,
        discoveryEndpoint: `${issuer.replace(/\/$/, '')}/.well-known/openid-configuration`,
        scopes: ['openid', 'email', 'profile'],
        mapping: {
          id: 'sub',
          email: 'email',
          emailVerified: 'email_verified',
          name: 'name',
        },
      };
      await db.insert(ssoProvider).values({
        id: randomUUID(),
        issuer,
        domain,
        providerId,
        oidcConfig: JSON.stringify(oidcConfig),
        userId: actorUserId,
        organizationId,
      });
    } catch (error) {
      if (
        error instanceof UnauthorizedException ||
        error instanceof ConflictException
      ) {
        throw error;
      }
      this.unavailable('register', error);
    }
    this.logger.log(`sso provider registered for org=${organizationId}`);
    return { provider: { providerId, domain, issuer, type: 'oidc' } };
  }

  @Delete('providers/:providerId')
  @HttpCode(HttpStatus.OK)
  async remove(
    @Headers('x-sso-admin-token') token: string | undefined,
    @Param('providerId') providerIdParam: string,
    @Query('organizationId') organizationId: string | undefined,
    @Query('actorUserId') actorUserId: string | undefined,
  ) {
    this.authenticate(token);
    const providerId = text(providerIdParam, 64).toLowerCase();
    const orgId = text(organizationId, 128);
    const actor = text(actorUserId, 128);
    if (!PROVIDER_ID.test(providerId) || !orgId || !actor) {
      throw new BadRequestException('providerId, organizationId and actorUserId are required');
    }
    try {
      if (!(await isOrganizationAdmin(orgId, actor))) {
        throw new UnauthorizedException('actor is not an administrator');
      }
      const deleted = await db
        .delete(ssoProvider)
        .where(
          and(
            eq(ssoProvider.providerId, providerId),
            eq(ssoProvider.organizationId, orgId),
          ),
        )
        .returning({ id: ssoProvider.id });
      if (deleted.length === 0) {
        throw new NotFoundException({
          code: 'provider_not_found',
          message: 'Provider not found.',
        });
      }
    } catch (error) {
      if (
        error instanceof UnauthorizedException ||
        error instanceof NotFoundException
      ) {
        throw error;
      }
      this.unavailable('remove', error);
    }
    return { status: 'removed' };
  }

  /** Has this user signed in through one of the organisation's providers recently? */
  @Get('evidence')
  async evidence(
    @Headers('x-sso-admin-token') token: string | undefined,
    @Query('organizationId') organizationId: string | undefined,
    @Query('userId') userId: string | undefined,
    @Query('withinSeconds') withinSeconds: string | undefined,
  ) {
    this.authenticate(token);
    const orgId = text(organizationId, 128);
    const uid = text(userId, 128);
    const window = Math.min(
      Math.max(Number.parseInt(withinSeconds ?? '', 10) || 86_400, 60),
      MAX_EVIDENCE_WINDOW_SECONDS,
    );
    if (!orgId || !uid) {
      throw new BadRequestException('organizationId and userId are required');
    }
    try {
      const rows = await db
        .select({ id: ssoSigninEvidence.id })
        .from(ssoSigninEvidence)
        .where(
          and(
            eq(ssoSigninEvidence.organizationId, orgId),
            eq(ssoSigninEvidence.userId, uid),
            sql`${ssoSigninEvidence.signedInAt} > now() - (${window}::int * interval '1 second')`,
          ),
        )
        .limit(1);
      return { recent: rows.length > 0 };
    } catch (error) {
      this.unavailable('evidence', error);
    }
  }

  /** Role (from the member table) and two-factor flag of the named users. */
  @Post('member-facts')
  @HttpCode(HttpStatus.OK)
  async memberFacts(
    @Headers('x-sso-admin-token') token: string | undefined,
    @Body() body: unknown,
  ) {
    this.authenticate(token);
    if (!isRecord(body)) throw new BadRequestException('an object is required');
    const orgId = text(body.organizationId, 128);
    const userIds = stringList(body.userIds, MAX_USER_IDS);
    if (!orgId) throw new BadRequestException('organizationId is required');
    if (userIds.length === 0) return { members: [] };
    try {
      const rows = await db
        .select({
          userId: member.userId,
          role: member.role,
          twoFactorEnabled: user.twoFactorEnabled,
        })
        .from(member)
        .innerJoin(user, eq(user.id, member.userId))
        .where(
          and(
            eq(member.organizationId, orgId),
            inArray(member.userId, userIds),
          ),
        );
      return {
        members: rows.map((row) => ({
          userId: row.userId,
          role: row.role,
          twoFactorEnabled: row.twoFactorEnabled === true,
        })),
      };
    } catch (error) {
      this.unavailable('member-facts', error);
    }
  }

  /**
   * End the sessions of the organisation's members whose e-mail is on one of
   * the covered domains, in resumable batches (cursor = last user id). Run when
   * a policy becomes `enforce`, so a covered user does not keep a password
   * session until it expires.
   */
  @Post('revoke-sessions')
  @HttpCode(HttpStatus.OK)
  async revokeSessions(
    @Headers('x-sso-admin-token') token: string | undefined,
    @Body() body: unknown,
  ) {
    this.authenticate(token);
    if (!isRecord(body)) throw new BadRequestException('an object is required');
    const orgId = text(body.organizationId, 128);
    const actor = text(body.actorUserId, 128);
    const domains = new Set(
      stringList(body.domains, 10).map((d) => d.toLowerCase()),
    );
    const exempt = new Set(stringList(body.exemptUserIds ?? [], 20));
    const after = text(body.afterUserId, 128);
    const limit = Math.min(
      Math.max(Number(body.limit) || 100, 1),
      MAX_REVOKE_BATCH,
    );
    if (!orgId || !actor || domains.size === 0) {
      throw new BadRequestException('organizationId, actorUserId and domains are required');
    }
    try {
      if (!(await isOrganizationAdmin(orgId, actor))) {
        throw new UnauthorizedException('actor is not an administrator');
      }
      const rows = await db
        .select({ userId: member.userId, email: user.email })
        .from(member)
        .innerJoin(user, eq(user.id, member.userId))
        .where(
          after
            ? and(eq(member.organizationId, orgId), gt(member.userId, after))
            : eq(member.organizationId, orgId),
        )
        .orderBy(asc(member.userId))
        .limit(limit);
      const context = await sessionAuth.$context;
      let revoked = 0;
      for (const row of rows) {
        const domain = emailDomain(row.email);
        if (!domain || !domains.has(domain) || exempt.has(row.userId)) continue;
        await context.internalAdapter.deleteSessions(row.userId);
        revoked += 1;
      }
      this.logger.log(`sso enforcement sweep for org=${orgId}: ${revoked} user(s)`);
      return {
        revoked,
        nextCursor: rows.length === limit ? rows[rows.length - 1].userId : '',
      };
    } catch (error) {
      if (error instanceof UnauthorizedException) throw error;
      this.unavailable('revoke-sessions', error);
    }
  }
}
