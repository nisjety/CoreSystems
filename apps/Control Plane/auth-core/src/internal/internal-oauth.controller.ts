import {
  Body,
  Controller,
  ForbiddenException,
  Headers,
  HttpCode,
  HttpStatus,
  Post,
  UnauthorizedException,
} from '@nestjs/common';
import { status } from '@grpc/grpc-js';
import {
  InternalOAuthService,
  InternalRefreshError,
  InternalRefreshResult,
  InternalTokenResult,
} from './internal-oauth.service';
import {
  AuthInternalServiceAuthorizationError,
  authorizeAuthInternalService,
  loadAuthInternalServiceCredentials,
} from './internal-service-auth';

interface TokenRequest {
  tokenRef?: string;
  provider?: string;
  providerAccountId?: string;
}

@Controller('internal/oauth')
export class InternalOAuthController {
  private readonly serviceCredentials = loadAuthInternalServiceCredentials();

  constructor(private readonly internalOAuthService: InternalOAuthService) {}

  @Post('token')
  @HttpCode(HttpStatus.OK)
  async getToken(
    @Headers('x-service-credential-id') credentialId: string | undefined,
    @Headers('x-service-principal') principal: string | undefined,
    @Headers('x-service-auth') serviceToken: string | undefined,
    @Body() body: TokenRequest,
  ) {
    this.authorize(credentialId, principal, serviceToken, 'oauth:token:read');

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
  async refreshToken(
    @Headers('x-service-credential-id') credentialId: string | undefined,
    @Headers('x-service-principal') principal: string | undefined,
    @Headers('x-service-auth') serviceToken: string | undefined,
    @Body() body: TokenRequest,
  ) {
    this.authorize(
      credentialId,
      principal,
      serviceToken,
      'oauth:token:refresh',
    );

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

  private authorize(
    credentialId: string | undefined,
    principal: string | undefined,
    token: string | undefined,
    requiredScope: 'oauth:token:read' | 'oauth:token:refresh',
  ): void {
    try {
      authorizeAuthInternalService(
        { credentialId, principal, token },
        this.serviceCredentials,
        requiredScope,
      );
    } catch (error) {
      if (
        error instanceof AuthInternalServiceAuthorizationError &&
        error.code === status.PERMISSION_DENIED
      ) {
        throw new ForbiddenException(
          'Service principal lacks OAuth token authority',
        );
      }
      throw new UnauthorizedException(
        'Valid scoped service credential required',
      );
    }
  }
}
