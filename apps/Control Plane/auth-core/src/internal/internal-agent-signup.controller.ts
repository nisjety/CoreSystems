/**
 * Internal Agent-Signup Controller
 *
 * Provides the `POST /internal/agent-signup` endpoint that Quarry (Ingestion
 * Plane) calls after a successful OTP verification to provision a sandbox
 * account.  The flow:
 *
 *   1. Create a user via Better Auth `signUpEmail` (server-side, no session).
 *   2. Create a personal organisation via Better Auth `createOrganization`.
 *   3. Create an API key via Better Auth `createApiKey`.
 *   4. Return `{ data: { orgId, userId, apiKey } }`.
 *
 * Protected by an audience/scope-bound service principal. Quarry receives a
 * dedicated agent:provision credential that cannot cross into other Auth
 * internal contracts.
 */

import {
  Body,
  Controller,
  ForbiddenException,
  Headers,
  HttpCode,
  HttpStatus,
  Logger,
  Post,
  UnauthorizedException,
} from '@nestjs/common';
import { status } from '@grpc/grpc-js';
import { randomBytes } from 'crypto';
import { auth } from '../auth/auth';
import {
  AuthInternalServiceAuthorizationError,
  authorizeAuthInternalService,
  loadAuthInternalServiceCredentials,
} from './internal-service-auth';

interface AgentSignupRequest {
  email: string;
}

interface AgentSignupResponse {
  data: {
    orgId: string;
    userId: string;
    apiKey: string;
  };
}

type AgentAuthApi = Readonly<{
  listUsers?: (input: {
    query: { searchField: string; searchValue: string; limit: number };
  }) => Promise<unknown>;
  createOrganization: (input: {
    body: { name: string; slug: string };
    query: { userId: string };
  }) => Promise<unknown>;
  createApiKey: (input: {
    body: {
      name: string;
      userId: string;
      metadata: { orgId: string; source: string };
    };
  }) => Promise<unknown>;
}>;

const agentAuthApi = auth.api as unknown as AgentAuthApi;

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function stringField(value: unknown, name: string): string {
  const candidate = record(value)?.[name];
  return typeof candidate === 'string' ? candidate : '';
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : 'unknown error';
}

function userAlreadyExists(error: unknown): boolean {
  const details = record(error);
  return Boolean(
    errorMessage(error).includes('already exists') ||
      details?.status === 409 ||
      record(details?.body)?.code === 'USER_ALREADY_EXISTS',
  );
}

@Controller('internal')
export class InternalAgentSignupController {
  private readonly logger = new Logger(InternalAgentSignupController.name);
  private readonly serviceCredentials = loadAuthInternalServiceCredentials();

  // ── POST /internal/agent-signup ──────────────────────────────────────

  @Post('agent-signup')
  @HttpCode(HttpStatus.CREATED)
  async agentSignup(
    @Headers('x-service-credential-id') credentialId: string | undefined,
    @Headers('x-service-principal') principal: string | undefined,
    @Headers('x-service-auth') token: string | undefined,
    @Body() body: AgentSignupRequest,
  ): Promise<AgentSignupResponse> {
    this.authorize(credentialId, principal, token);

    const email = body.email?.trim().toLowerCase();
    if (!email || !this.isValidEmail(email)) {
      throw new ForbiddenException('invalid or missing email');
    }

    this.logger.log(
      `🚀 [agent-signup] Starting sandbox provisioning for ${email}`,
    );

    // 1. Create user ───────────────────────────────────────────────────
    const tempPassword = randomBytes(24).toString('base64url');
    let userId: string;

    try {
      const signupResult = await auth.api.signUpEmail({
        body: {
          email,
          password: tempPassword,
          name: email.split('@')[0],
        },
        // No headers → server-side call, no session is created for the caller
      });

      // signUpEmail returns { user, session? } — user always present on success
      const signup = record(signupResult);
      const user = record(signup?.user) ?? signup;
      userId = stringField(user, 'id');

      if (!userId) {
        this.logger.error(`❌ [agent-signup] signUpEmail returned no userId`);
        throw new Error('user creation returned no id');
      }

      this.logger.log(`✅ [agent-signup] User created: ${userId}`);
    } catch (error: unknown) {
      // If user already exists, try to look them up
      if (userAlreadyExists(error)) {
        this.logger.warn(
          `⚠️ [agent-signup] User ${email} already exists — looking up`,
        );
        const existingUsers = agentAuthApi.listUsers
          ? await agentAuthApi
              .listUsers({
                query: { searchField: 'email', searchValue: email, limit: 1 },
              })
              .catch(() => null)
          : null;
        const users = record(existingUsers)?.users;
        const existing = Array.isArray(users) ? record(users[0]) : null;
        const existingUserId = stringField(existing, 'id');
        if (!existingUserId) {
          throw new ForbiddenException(
            'A user with this email already exists but could not be resolved',
          );
        }
        userId = existingUserId;
        this.logger.log(`✅ [agent-signup] Resolved existing user: ${userId}`);
      } else {
        this.logger.error(
          `❌ [agent-signup] User creation failed: ${errorMessage(error)}`,
        );
        throw error;
      }
    }

    // 2. Create organisation ───────────────────────────────────────────
    let orgId: string;

    try {
      const slug = `sandbox-${email.replace(/[^a-z0-9]/g, '-').slice(0, 40)}-${Date.now()}`;
      const orgResult = await agentAuthApi.createOrganization({
        body: {
          name: `${email}'s workspace`,
          slug,
        },
        // Server-side call — pass userId so org is created for that user
        query: { userId },
      });

      orgId = stringField(orgResult, 'id');

      if (!orgId) {
        this.logger.error(
          `❌ [agent-signup] createOrganization returned no id`,
        );
        throw new Error('organization creation returned no id');
      }

      this.logger.log(`✅ [agent-signup] Organization created: ${orgId}`);
    } catch (error: unknown) {
      this.logger.error(
        `❌ [agent-signup] Org creation failed: ${errorMessage(error)}`,
      );
      throw error;
    }

    // 3. Create API key ────────────────────────────────────────────────
    let apiKeyValue: string;

    try {
      const keyResult = await agentAuthApi.createApiKey({
        body: {
          name: 'sandbox-key',
          userId,
          metadata: {
            orgId,
            source: 'agent-signup',
          },
        },
      });

      apiKeyValue = stringField(keyResult, 'key');

      if (!apiKeyValue) {
        this.logger.error(`❌ [agent-signup] createApiKey returned no key`);
        throw new Error('API key creation returned no key');
      }

      this.logger.log(`✅ [agent-signup] API key created for user ${userId}`);
    } catch (error: unknown) {
      this.logger.error(
        `❌ [agent-signup] API key creation failed: ${errorMessage(error)}`,
      );
      throw error;
    }

    this.logger.log(
      `🎉 [agent-signup] Sandbox provisioned: user=${userId} org=${orgId}`,
    );

    return {
      data: {
        orgId,
        userId,
        apiKey: apiKeyValue,
      },
    };
  }

  // ── Helpers ──────────────────────────────────────────────────────────

  private authorize(
    credentialId: string | undefined,
    principal: string | undefined,
    token: string | undefined,
  ): void {
    try {
      authorizeAuthInternalService(
        { credentialId, principal, token },
        this.serviceCredentials,
        'agent:provision',
      );
    } catch (error) {
      if (
        error instanceof AuthInternalServiceAuthorizationError &&
        error.code === status.PERMISSION_DENIED
      ) {
        throw new ForbiddenException(
          'Service principal lacks agent provisioning authority',
        );
      }
      throw new UnauthorizedException(
        'Valid scoped service credential required',
      );
    }
  }

  private isValidEmail(email: string): boolean {
    return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email);
  }
}
