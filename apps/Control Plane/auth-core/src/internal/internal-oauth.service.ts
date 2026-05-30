import { Injectable, Logger } from '@nestjs/common';
import { sqlClient } from '../db';
import { createCipheriv, createDecipheriv, randomBytes } from 'crypto';

export interface InternalTokenResult {
  tokenRef: string;
  provider: string;
  providerAccountId: string;
  userId: string;
  accessToken: string | null;
  refreshToken: string | null;
  expiresAt: Date | null;
  scope: string | null;
}

export interface InternalRefreshResult {
  ok: true;
  tokenRef: string;
  provider: string;
  providerAccountId: string;
  userId: string;
  accessToken: string;
  refreshToken: string | null;
  expiresAt: Date;
  scope: string | null;
}

export interface InternalRefreshError {
  ok: false;
  code:
    | 'token_not_found'
    | 'no_refresh_token'
    | 'unsupported_provider'
    | 'provider_not_configured'
    | 'provider_rejected'
    | 'provider_unreachable'
    | 'persist_failed';
  detail?: string;
}

interface ProviderTokenResponse {
  access_token?: unknown;
  refresh_token?: unknown;
  expires_in?: unknown;
  scope?: unknown;
  token_type?: unknown;
  error?: unknown;
  error_description?: unknown;
}

@Injectable()
export class InternalOAuthService {
  private readonly logger = new Logger(InternalOAuthService.name);

  async getTokenByRef(tokenRef: string): Promise<InternalTokenResult | null> {
    const ref = tokenRef.trim();
    if (!ref) {
      return null;
    }

    const rows = await sqlClient<
      {
        id: string;
        provider_id: string;
        account_id: string;
        user_id: string;
        access_token: string | null;
        refresh_token: string | null;
        access_token_expires_at: Date | null;
        scope: string | null;
        updated_at: Date;
      }[]
    >`
      SELECT
        id,
        provider_id,
        account_id,
        user_id,
        access_token,
        refresh_token,
        access_token_expires_at,
        scope,
        updated_at
      FROM account
      WHERE id = ${ref} OR account_id = ${ref}
      ORDER BY updated_at DESC
      LIMIT 1
    `;

    if (rows.length === 0) {
      return null;
    }

    const row = rows[0];
    const accessToken =
      typeof row.access_token === 'string' ? row.access_token : null;
    const refreshToken =
      typeof row.refresh_token === 'string' ? row.refresh_token : null;
    return {
      tokenRef: row.id,
      provider: row.provider_id,
      providerAccountId: row.account_id,
      userId: row.user_id,
      accessToken: this.decryptMaybe(accessToken),
      refreshToken: this.decryptMaybe(refreshToken),
      expiresAt: row.access_token_expires_at,
      scope: row.scope,
    };
  }

  async getTokenByProviderAccount(
    provider: string,
    providerAccountId: string,
  ): Promise<InternalTokenResult | null> {
    const p = provider.trim();
    const a = providerAccountId.trim();
    const rows = await sqlClient<
      {
        id: string;
        provider_id: string;
        account_id: string;
        user_id: string;
        access_token: string | null;
        refresh_token: string | null;
        access_token_expires_at: Date | null;
        scope: string | null;
        updated_at: Date;
      }[]
    >`
      SELECT
        id,
        provider_id,
        account_id,
        user_id,
        access_token,
        refresh_token,
        access_token_expires_at,
        scope,
        updated_at
      FROM account
      WHERE provider_id = ${p} AND account_id = ${a}
      ORDER BY updated_at DESC
      LIMIT 1
    `;

    if (rows.length === 0) {
      return null;
    }

    const row = rows[0];
    const accessToken =
      typeof row.access_token === 'string' ? row.access_token : null;
    const refreshToken =
      typeof row.refresh_token === 'string' ? row.refresh_token : null;
    return {
      tokenRef: row.id,
      provider: row.provider_id,
      providerAccountId: row.account_id,
      userId: row.user_id,
      accessToken: this.decryptMaybe(accessToken),
      refreshToken: this.decryptMaybe(refreshToken),
      expiresAt: row.access_token_expires_at,
      scope: row.scope,
    };
  }

  /**
   * G24: Provider-aware OAuth token refresh.
   *
   * Loads the stored refresh token for `tokenRef`, exchanges it with the
   * upstream provider for a fresh access token (and possibly a rotated
   * refresh token), persists the new tokens (encrypted), and returns the
   * short-lived access token to the caller.
   *
   * Supported providers: microsoft (Entra ID), google. Others return
   * `unsupported_provider` so the caller can surface a clear error to the
   * operator.
   *
   * Errors are returned as a tagged union (`InternalRefreshError`) instead
   * of thrown so the controller can produce stable JSON responses without
   * leaking provider error text into stack traces.
   */
  async refreshTokenByRef(
    tokenRef: string,
  ): Promise<InternalRefreshResult | InternalRefreshError> {
    const existing = await this.getTokenByRef(tokenRef);
    if (!existing) {
      return { ok: false, code: 'token_not_found' };
    }

    const refreshToken = existing.refreshToken?.trim();
    if (!refreshToken) {
      return {
        ok: false,
        code: 'no_refresh_token',
        detail:
          'no refresh_token stored for this account; original sign-in did not request offline_access',
      };
    }

    const providerConfig = this.providerConfig(existing.provider);
    if (!providerConfig) {
      return { ok: false, code: 'unsupported_provider' };
    }

    if (!providerConfig.clientId || !providerConfig.clientSecret) {
      return {
        ok: false,
        code: 'provider_not_configured',
        detail: `${existing.provider}: clientId or clientSecret env var is missing`,
      };
    }

    const providerResp = await this.exchangeRefreshToken(
      providerConfig.tokenEndpoint,
      providerConfig.clientId,
      providerConfig.clientSecret,
      refreshToken,
      existing.scope,
    );
    // `'ok' in providerResp` is the discriminator — only the error variant
    // (InternalRefreshError) carries an `ok` property; the success variant is
    // an anonymous shape without it. Using just the `in` check lets TS narrow
    // providerResp to the success variant on the fall-through path; the
    // combined `&& providerResp.ok === false` form left both variants in the
    // union and caused TS2339 on access_token / refresh_token / expiresAt /
    // scope below.
    if ('ok' in providerResp) {
      return providerResp;
    }

    const newAccessToken = providerResp.access_token;
    const newRefreshToken = providerResp.refresh_token ?? null;
    const expiresAt = providerResp.expiresAt;
    const newScope = providerResp.scope ?? existing.scope;

    try {
      await this.persistRefreshedToken(
        existing.tokenRef,
        newAccessToken,
        newRefreshToken,
        expiresAt,
        newScope,
      );
    } catch (error) {
      this.logger.error(
        `Failed to persist refreshed token for ${existing.tokenRef}: ${error instanceof Error ? error.message : String(error)}`,
      );
      return {
        ok: false,
        code: 'persist_failed',
        detail: 'database write failed; client should retry',
      };
    }

    return {
      ok: true,
      tokenRef: existing.tokenRef,
      provider: existing.provider,
      providerAccountId: existing.providerAccountId,
      userId: existing.userId,
      accessToken: newAccessToken,
      refreshToken: newRefreshToken,
      expiresAt,
      scope: newScope,
    };
  }

  private providerConfig(provider: string): {
    tokenEndpoint: string;
    clientId: string | undefined;
    clientSecret: string | undefined;
  } | null {
    switch (provider) {
      case 'microsoft': {
        const tenantId = process.env.MICROSOFT_TENANT_ID || 'common';
        return {
          tokenEndpoint: `https://login.microsoftonline.com/${tenantId}/oauth2/v2.0/token`,
          clientId: process.env.MICROSOFT_CLIENT_ID,
          clientSecret: process.env.MICROSOFT_CLIENT_SECRET,
        };
      }
      case 'google':
        return {
          tokenEndpoint: 'https://oauth2.googleapis.com/token',
          clientId: process.env.GOOGLE_CLIENT_ID,
          clientSecret: process.env.GOOGLE_CLIENT_SECRET,
        };
      default:
        return null;
    }
  }

  private async exchangeRefreshToken(
    tokenEndpoint: string,
    clientId: string,
    clientSecret: string,
    refreshToken: string,
    scope: string | null,
  ): Promise<
    | {
        access_token: string;
        refresh_token: string | null;
        expiresAt: Date;
        scope: string | null;
      }
    | InternalRefreshError
  > {
    const body = new URLSearchParams({
      grant_type: 'refresh_token',
      client_id: clientId,
      client_secret: clientSecret,
      refresh_token: refreshToken,
    });
    if (scope) {
      // G47 (velion-gap.md §8.33): Better Auth stores granted scopes as a
      // comma-separated string in `account.scope` (e.g.
      // `email,openid,profile,User.Read`), but RFC 6749 §3.3 requires the
      // OAuth scope parameter to be space-separated. Sending the raw
      // comma-string made Microsoft treat the whole thing as one unknown
      // scope name and return AADSTS65001 because the "scope" didn't
      // match any consent record (what we hit in Wave 12 verification).
      const normalized = scope
        .split(/[\s,]+/)
        .map((s) => s.trim())
        .filter((s) => s.length > 0)
        .join(' ');
      if (normalized) body.set('scope', normalized);
    }

    const ctrl = new AbortController();
    const timer = setTimeout(() => ctrl.abort(), 5_000);
    let resp: Response;
    try {
      resp = await fetch(tokenEndpoint, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/x-www-form-urlencoded',
          Accept: 'application/json',
        },
        body: body.toString(),
        signal: ctrl.signal,
      });
    } catch (error) {
      this.logger.warn(
        `Provider token endpoint unreachable: ${error instanceof Error ? error.message : String(error)}`,
      );
      return { ok: false, code: 'provider_unreachable' };
    } finally {
      clearTimeout(timer);
    }

    let payload: ProviderTokenResponse;
    try {
      payload = (await resp.json()) as ProviderTokenResponse;
    } catch {
      return {
        ok: false,
        code: 'provider_rejected',
        detail: `non-JSON response (status ${resp.status})`,
      };
    }

    if (!resp.ok) {
      const errCode =
        typeof payload.error === 'string' ? payload.error : 'unknown_error';
      const errDesc =
        typeof payload.error_description === 'string'
          ? payload.error_description
          : `status ${resp.status}`;
      this.logger.warn(`Provider rejected refresh: ${errCode} — ${errDesc}`);
      return {
        ok: false,
        code: 'provider_rejected',
        detail: `${errCode}: ${errDesc}`,
      };
    }

    if (typeof payload.access_token !== 'string' || !payload.access_token) {
      return {
        ok: false,
        code: 'provider_rejected',
        detail: 'missing access_token in provider response',
      };
    }

    const expiresIn =
      typeof payload.expires_in === 'number' && payload.expires_in > 0
        ? payload.expires_in
        : 3600;

    return {
      access_token: payload.access_token,
      refresh_token:
        typeof payload.refresh_token === 'string' && payload.refresh_token
          ? payload.refresh_token
          : null,
      expiresAt: new Date(Date.now() + expiresIn * 1000),
      scope: typeof payload.scope === 'string' ? payload.scope : null,
    };
  }

  private async persistRefreshedToken(
    tokenRef: string,
    accessToken: string,
    refreshToken: string | null,
    expiresAt: Date,
    scope: string | null,
  ): Promise<void> {
    const encAccess = this.encryptMaybe(accessToken);
    const encRefresh = refreshToken ? this.encryptMaybe(refreshToken) : null;

    if (encRefresh !== null) {
      await sqlClient`
        UPDATE account
        SET access_token = ${encAccess},
            refresh_token = ${encRefresh},
            access_token_expires_at = ${expiresAt},
            scope = COALESCE(${scope}, scope),
            updated_at = NOW()
        WHERE id = ${tokenRef}
      `;
    } else {
      // Provider did not rotate the refresh token; keep the existing one.
      await sqlClient`
        UPDATE account
        SET access_token = ${encAccess},
            access_token_expires_at = ${expiresAt},
            scope = COALESCE(${scope}, scope),
            updated_at = NOW()
        WHERE id = ${tokenRef}
      `;
    }
  }

  /**
   * AES-256-GCM encryption mirroring the helper in `auth/auth.ts`.
   * Format: `iv.tag.ciphertext` — all base64.
   * Returns the plaintext unchanged when no key is configured (dev path).
   */
  private encryptMaybe(token: string): string {
    const keyB64 = process.env.TOKEN_ENCRYPTION_KEY;
    if (!keyB64) return token;

    const key = Buffer.from(keyB64, 'base64');
    if (key.length !== 32) return token;

    const iv = randomBytes(12);
    const cipher = createCipheriv('aes-256-gcm', key, iv);
    const enc = Buffer.concat([cipher.update(token, 'utf8'), cipher.final()]);
    const tag = cipher.getAuthTag();
    return `${iv.toString('base64')}.${tag.toString('base64')}.${enc.toString('base64')}`;
  }

  private decryptMaybe(token: string | null): string | null {
    if (!token) {
      return null;
    }

    const keyB64 = process.env.TOKEN_ENCRYPTION_KEY;
    if (!keyB64) {
      return token;
    }

    const segments = token.split('.');
    if (segments.length !== 3) {
      return token;
    }

    try {
      const key = Buffer.from(keyB64, 'base64');
      if (key.length !== 32) {
        return token;
      }

      const iv = Buffer.from(segments[0], 'base64');
      const tag = Buffer.from(segments[1], 'base64');
      const ciphertext = Buffer.from(segments[2], 'base64');

      const decipher = createDecipheriv('aes-256-gcm', key, iv);
      decipher.setAuthTag(tag);
      const dec = Buffer.concat([
        decipher.update(ciphertext),
        decipher.final(),
      ]);
      return dec.toString('utf8');
    } catch (error) {
      this.logger.warn(
        `Failed to decrypt provider token: ${error instanceof Error ? error.message : String(error)}`,
      );
      return token;
    }
  }
}
