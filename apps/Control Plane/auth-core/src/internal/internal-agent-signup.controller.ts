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
 * Protected by `x-internal-api-key` header — only reachable from internal
 * services (Quarry) that share the same INTERNAL_API_KEY env var.
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
} from '@nestjs/common';
import { randomBytes } from 'crypto';
import { auth } from '../auth/auth';

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

@Controller('internal')
export class InternalAgentSignupController {
  private readonly logger = new Logger(InternalAgentSignupController.name);

  // ── POST /internal/agent-signup ──────────────────────────────────────

  @Post('agent-signup')
  @HttpCode(HttpStatus.CREATED)
  async agentSignup(
    @Headers('x-internal-api-key') internalKey: string,
    @Body() body: AgentSignupRequest,
  ): Promise<AgentSignupResponse> {
    this.assertInternalKey(internalKey);

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
      const user = (signupResult as any)?.user ?? signupResult;
      userId = user?.id;

      if (!userId) {
        this.logger.error(`❌ [agent-signup] signUpEmail returned no userId`);
        throw new Error('user creation returned no id');
      }

      this.logger.log(`✅ [agent-signup] User created: ${userId}`);
    } catch (err: any) {
      // If user already exists, try to look them up
      if (
        err?.message?.includes('already exists') ||
        err?.status === 409 ||
        err?.body?.code === 'USER_ALREADY_EXISTS'
      ) {
        this.logger.warn(
          `⚠️ [agent-signup] User ${email} already exists — looking up`,
        );
        const authApi = auth.api as any;
        const existingUsers = await authApi
          .listUsers?.({
            query: { searchField: 'email', searchValue: email, limit: 1 },
          })
          .catch(() => null);

        const existing = existingUsers?.users?.[0];
        if (!existing?.id) {
          throw new ForbiddenException(
            'A user with this email already exists but could not be resolved',
          );
        }
        userId = existing.id;
        this.logger.log(`✅ [agent-signup] Resolved existing user: ${userId}`);
      } else {
        this.logger.error(
          `❌ [agent-signup] User creation failed: ${err.message}`,
        );
        throw err;
      }
    }

    // 2. Create organisation ───────────────────────────────────────────
    let orgId: string;

    try {
      const slug = `sandbox-${email.replace(/[^a-z0-9]/g, '-').slice(0, 40)}-${Date.now()}`;
      const authApi = auth.api as any;
      const orgResult = await authApi.createOrganization({
        body: {
          name: `${email}'s workspace`,
          slug,
        },
        // Server-side call — pass userId so org is created for that user
        query: { userId },
      } as any);

      orgId = orgResult?.id;

      if (!orgId) {
        this.logger.error(
          `❌ [agent-signup] createOrganization returned no id`,
        );
        throw new Error('organization creation returned no id');
      }

      this.logger.log(`✅ [agent-signup] Organization created: ${orgId}`);
    } catch (err: any) {
      this.logger.error(
        `❌ [agent-signup] Org creation failed: ${err.message}`,
      );
      throw err;
    }

    // 3. Create API key ────────────────────────────────────────────────
    let apiKeyValue: string;

    try {
      const authApi = auth.api as any;
      const keyResult = await authApi.createApiKey({
        body: {
          name: 'sandbox-key',
          userId,
          metadata: {
            orgId,
            source: 'agent-signup',
          },
        },
      });

      apiKeyValue = keyResult?.key;

      if (!apiKeyValue) {
        this.logger.error(`❌ [agent-signup] createApiKey returned no key`);
        throw new Error('API key creation returned no key');
      }

      this.logger.log(`✅ [agent-signup] API key created for user ${userId}`);
    } catch (err: any) {
      this.logger.error(
        `❌ [agent-signup] API key creation failed: ${err.message}`,
      );
      throw err;
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

  private assertInternalKey(internalKey?: string): void {
    const expected =
      process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET;

    if (!expected) {
      throw new ForbiddenException(
        'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET not configured — refusing request',
      );
    }

    if (!internalKey || internalKey !== expected) {
      throw new ForbiddenException('invalid internal API key');
    }
  }

  private isValidEmail(email: string): boolean {
    return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email);
  }
}
