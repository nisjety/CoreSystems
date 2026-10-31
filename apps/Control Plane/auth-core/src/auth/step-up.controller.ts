/**
 * Step-up re-authentication for high-risk Model Plane approvals (build plan WS13 S5).
 *
 * `POST /api/v2/auth/step-up` re-checks the signed-in user's password against their credential
 * account and records a proof for that session, valid for 120 seconds. The Model Plane token
 * minted while the proof is fresh carries it as a `step_up` claim, and the gateway refuses to
 * grant an approval whose cause demands one without it. A denial never needs the proof.
 *
 * The path sits under `api/v2/auth` because Better Auth owns `/api/auth/*`
 * (see nats-auth.controller.ts).
 */

import { createHash } from 'node:crypto';

import {
  BadRequestException,
  Body,
  Controller,
  ForbiddenException,
  HttpException,
  HttpStatus,
  Logger,
  Post,
  Req,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { ApiOperation, ApiResponse, ApiTags } from '@nestjs/swagger';
import { and, eq } from 'drizzle-orm';
import type { Request } from 'express';

import { db } from '../db';
import { redisSecondaryStorage } from '../db/redis';
import { account } from '../db/schema';
import { auth } from './auth';

export const STEP_UP_VALIDITY_SECONDS = 120;

const STEP_UP_METHOD = 'password';
const ATTEMPT_WINDOW_SECONDS = 900;
const ATTEMPT_LIMIT = 5;
const MAX_PASSWORD_LENGTH = 1024;

const logger = new Logger('StepUpController');

export interface StepUpClaim {
  /** Unix seconds at which the password was re-verified. */
  at: number;
  method: typeof STEP_UP_METHOD;
}

interface StepUpBody {
  password?: unknown;
}

interface StepUpSessionSnapshot {
  user?: { id?: string };
  session?: { id?: string };
}

const typedSessionAuth = auth as unknown as {
  api: {
    getSession(input: {
      headers: Headers;
    }): Promise<StepUpSessionSnapshot | null>;
  };
  // The password check Better Auth itself uses at sign-in, so the two cannot disagree.
  $context: Promise<{
    password: {
      verify(input: { hash: string; password: string }): Promise<boolean>;
    };
  }>;
};

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

function stepUpKey(sessionId: string): string {
  return `model-plane:step-up:${sessionId}`;
}

// Attempts are counted per user, keyed by a digest so the raw user id is not stored.
function attemptKey(userId: string): string {
  const digest = createHash('sha256').update(userId).digest('hex');
  return `model-plane:step-up-attempts:${digest}`;
}

function isStepUpClaim(value: unknown): value is StepUpClaim {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { at?: unknown }).at === 'number' &&
    (value as { method?: unknown }).method === STEP_UP_METHOD
  );
}

/**
 * The step-up proof recorded for this session, if it is still within its validity. A store that
 * cannot be read yields no proof: the token is still minted, and the gateway refuses only the
 * grants that need one.
 */
export async function readFreshStepUp(
  sessionId: string | undefined,
  nowSeconds: number = Math.floor(Date.now() / 1000),
): Promise<StepUpClaim | undefined> {
  if (!sessionId) return undefined;
  let raw: string | null;
  try {
    raw = await redisSecondaryStorage.get(stepUpKey(sessionId));
  } catch {
    logger.warn(
      'Step-up proof could not be read; the token is minted without it',
    );
    return undefined;
  }
  if (!raw) return undefined;
  let claim: unknown;
  try {
    claim = JSON.parse(raw);
  } catch {
    return undefined;
  }
  if (!isStepUpClaim(claim)) return undefined;
  return nowSeconds - claim.at <= STEP_UP_VALIDITY_SECONDS ? claim : undefined;
}

@ApiTags('Model Plane Auth')
@Controller('api/v2/auth')
export class StepUpController {
  @Post('step-up')
  @ApiOperation({
    summary:
      'Re-verify the signed-in password before a high-risk Model Plane approval',
  })
  @ApiResponse({
    status: 200,
    description: 'A step-up proof, valid for STEP_UP_VALIDITY_SECONDS',
  })
  async stepUp(@Req() request: Request, @Body() body: StepUpBody) {
    const session = await typedSessionAuth.api.getSession({
      headers: toWebHeaders(request.headers),
    });
    const userId = session?.user?.id;
    const sessionId = session?.session?.id;
    if (!userId || !sessionId) {
      throw new UnauthorizedException('Authentication required');
    }

    const password = typeof body?.password === 'string' ? body.password : '';
    if (password.length === 0 || password.length > MAX_PASSWORD_LENGTH) {
      throw new BadRequestException({
        code: 'VALIDATION_ERROR',
        message: 'A password is required to confirm this action.',
      });
    }

    const attempts = await this.countAttempt(userId);
    if (attempts > ATTEMPT_LIMIT) {
      throw new HttpException(
        {
          code: 'RATE_LIMITED',
          message: 'Too many password attempts. Try again later.',
        },
        HttpStatus.TOO_MANY_REQUESTS,
      );
    }

    const hash = await this.credentialPasswordHash(userId);
    if (!hash) {
      throw new ForbiddenException({
        code: 'STEP_UP_METHOD_UNAVAILABLE',
        message: 'This account has no password to confirm with.',
        details: { method: STEP_UP_METHOD },
      });
    }
    const context = await typedSessionAuth.$context;
    if (!(await context.password.verify({ hash, password }))) {
      throw new UnauthorizedException({
        code: 'INVALID_PASSWORD',
        message: 'The password is not correct.',
      });
    }

    const nowSeconds = Math.floor(Date.now() / 1000);
    await this.recordStepUp(sessionId, nowSeconds);
    return {
      data: {
        method: STEP_UP_METHOD,
        stepUpAt: new Date(nowSeconds * 1000).toISOString(),
        expiresAt: new Date(
          (nowSeconds + STEP_UP_VALIDITY_SECONDS) * 1000,
        ).toISOString(),
        expiresInSeconds: STEP_UP_VALIDITY_SECONDS,
      },
    };
  }

  private async countAttempt(userId: string): Promise<number> {
    try {
      return await redisSecondaryStorage.increment(
        attemptKey(userId),
        ATTEMPT_WINDOW_SECONDS,
      );
    } catch {
      throw this.unavailable();
    }
  }

  private async credentialPasswordHash(
    userId: string,
  ): Promise<string | undefined> {
    const rows = await db
      .select({ password: account.password })
      .from(account)
      .where(
        and(eq(account.userId, userId), eq(account.providerId, 'credential')),
      )
      .limit(1);
    return rows[0]?.password ?? undefined;
  }

  private async recordStepUp(sessionId: string, nowSeconds: number) {
    const claim: StepUpClaim = { at: nowSeconds, method: STEP_UP_METHOD };
    try {
      await redisSecondaryStorage.set(
        stepUpKey(sessionId),
        JSON.stringify(claim),
        STEP_UP_VALIDITY_SECONDS,
      );
    } catch {
      throw this.unavailable();
    }
  }

  private unavailable(): ServiceUnavailableException {
    return new ServiceUnavailableException({
      code: 'STEP_UP_UNAVAILABLE',
      message: 'Confirmation is unavailable. Try again shortly.',
    });
  }
}
