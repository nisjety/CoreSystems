import {
  Body,
  Controller,
  ForbiddenException,
  HttpCode,
  HttpStatus,
  Post,
} from '@nestjs/common';
import {
  InternalOAuthService,
  InternalRefreshError,
  InternalRefreshResult,
  InternalTokenResult,
} from './internal-oauth.service';

interface TokenRequest {
  tokenRef?: string;
  provider?: string;
  providerAccountId?: string;
  internalApiKey?: string;
}

@Controller('internal/oauth')
export class InternalOAuthController {
  constructor(private readonly internalOAuthService: InternalOAuthService) {}

  @Post('token')
  @HttpCode(HttpStatus.OK)
  async getToken(@Body() body: TokenRequest) {
    this.assertInternalKey(body.internalApiKey);

    const tokenRef = body.tokenRef?.trim();
    let token: InternalTokenResult | null = null;

    if (tokenRef) {
      token = await this.internalOAuthService.getTokenByRef(tokenRef);
    } else if (body.provider?.trim() && body.providerAccountId?.trim()) {
      token = await this.internalOAuthService.getTokenByProviderAccount(
        body.provider,
        body.providerAccountId,
      );
    }

    if (!token) {
      return {
        found: false,
        error: 'token reference not found',
      };
    }

    return {
      found: true,
      tokenRef: token.tokenRef,
      provider: token.provider,
      providerAccountId: token.providerAccountId,
      userId: token.userId,
      access_token: token.accessToken,
      refresh_token: token.refreshToken,
      scope: token.scope,
      // G41 (velion-gap.md §8.30): `expiresAt` is typed `Date | null` but the
      // underlying `sqlClient` driver returns ISO strings at runtime for
      // `TIMESTAMPTZ` columns, so `.toISOString()` blows up. Coerce to a
      // canonical ISO string defensively — accept either shape and skip on
      // null/undefined.
      expires_at: this.toIsoString(token.expiresAt),
    };
  }

  private toIsoString(value: unknown): string | null {
    if (value == null) {
      return null;
    }
    if (value instanceof Date) {
      return value.toISOString();
    }
    if (typeof value === 'string') {
      const trimmed = value.trim();
      if (trimmed === '') return null;
      // Already an ISO string; pass through. If it's a different shape,
      // try Date parsing; bail to null on invalid.
      const asDate = new Date(trimmed);
      return Number.isNaN(asDate.getTime()) ? null : asDate.toISOString();
    }
    return null;
  }

  /**
   * G24: provider-aware OAuth refresh.
   *
   * Exchanges the stored refresh token (encrypted in `account.refresh_token`)
   * for a fresh access token via the provider's token endpoint. Persists the
   * new access token (and rotated refresh token, if returned) and returns the
   * short-lived access token to the caller.
   *
   * Response shape on success:
   *   { refreshed: true, tokenRef, provider, providerAccountId, userId,
   *     access_token, refresh_token, scope, expires_at }
   *
   * Response shape on failure: { refreshed: false, code, error }
   * Codes: token_not_found, no_refresh_token, unsupported_provider,
   *        provider_not_configured, provider_rejected, provider_unreachable,
   *        persist_failed.
   */
  @Post('refresh')
  @HttpCode(HttpStatus.OK)
  async refreshToken(@Body() body: TokenRequest) {
    this.assertInternalKey(body.internalApiKey);

    const tokenRef = body.tokenRef?.trim();
    if (!tokenRef) {
      return {
        refreshed: false,
        code: 'invalid_request',
        error: 'tokenRef is required',
      };
    }

    const result = await this.internalOAuthService.refreshTokenByRef(tokenRef);
    if (!result.ok) {
      return this.formatRefreshError(result);
    }

    return this.formatRefreshSuccess(result);
  }

  private formatRefreshSuccess(result: InternalRefreshResult) {
    return {
      refreshed: true,
      tokenRef: result.tokenRef,
      provider: result.provider,
      providerAccountId: result.providerAccountId,
      userId: result.userId,
      access_token: result.accessToken,
      refresh_token: result.refreshToken,
      scope: result.scope,
      expires_at: result.expiresAt.toISOString(),
    };
  }

  private formatRefreshError(error: InternalRefreshError) {
    return {
      refreshed: false,
      code: error.code,
      error: error.detail ?? error.code,
    };
  }

  private assertInternalKey(internalApiKey?: string) {
    const expected =
      process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET;

    if (!expected) {
      throw new ForbiddenException(
        'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET not configured — refusing request',
      );
    }

    if (!internalApiKey || internalApiKey !== expected) {
      throw new ForbiddenException('invalid internal API key');
    }
  }
}
