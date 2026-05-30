// NangoSdkClient wraps the official @nangohq/node SDK.
//
// Stub mode (secretKey empty):
//   Returns a synthetic session token and logs a warning at startup.
//   Safe for local development — no real Nango calls are made.
//
// Production mode (secretKey set):
//   Delegates to nango.createConnectSession() with:
//     input.allowedIntegrations → allowed_integrations
//     input.tags                → tags  (end_user_id, org_id, etc.)
//
// Self-hosted Nango: set baseUrl to your instance URL.
// Nango Cloud (default): leave baseUrl empty (https://api.nango.dev).
import { randomUUID } from 'node:crypto';

import { Nango } from '@nangohq/node';

import { HttpError } from '../http/http-error';

export interface CreateConnectSessionInput {
  tags: Record<string, string>;
  allowedIntegrations: string[];
}

export interface ConnectSessionResult {
  token: string;
  connectLink: string;
  expiresAt: string;
}

export interface NangoRuntimeClient {
  createConnectSession(input: CreateConnectSessionInput): Promise<ConnectSessionResult>;
  getConnectionToken(providerConfigKey: string, connectionId: string): Promise<string>;
}

export interface NangoSdkClientConfig {
  secretKey: string;  // CONNECTOR_RUNTIME_SECRET — leave empty for stub mode
  baseUrl?: string;   // CONNECTOR_RUNTIME_BASE_URL — optional, self-hosted Nango
  publicBaseUrl?: string; // Browser-reachable URL for the self-hosted Connect UI API.
}

// NangoSdkClient is the production client backed by @nangohq/node.
export class NangoSdkClient implements NangoRuntimeClient {
  private readonly client: Nango | null;
  private readonly publicBaseUrl?: string;

  constructor(cfg: NangoSdkClientConfig) {
    this.publicBaseUrl = normaliseBaseUrl(cfg.publicBaseUrl);

    if (!cfg.secretKey.trim()) {
      console.warn('[integration-core/runtime] WARNING: CONNECTOR_RUNTIME_SECRET is not set — stub mode active, no real Nango sessions will be created');
      this.client = null;
      return;
    }

    this.client = new Nango({
      secretKey: cfg.secretKey,
      ...(cfg.baseUrl?.trim() ? { host: cfg.baseUrl.trim() } : {})
    });
  }

  async createConnectSession(input: CreateConnectSessionInput): Promise<ConnectSessionResult> {
    // Stub mode — return synthetic values so local dev works without API keys.
    if (this.client === null) {
      const stubToken = `stub_${randomUUID()}`;
      return {
        token: stubToken,
        connectLink: withConnectApiUrl(`http://localhost:3009/connect?session_token=${stubToken}`, this.publicBaseUrl),
        expiresAt: new Date(Date.now() + 30 * 60 * 1000).toISOString()
      };
    }

    let data: { token: string; connect_link: string; expires_at: string };

    try {
      const response = await this.client.createConnectSession({
        allowed_integrations: input.allowedIntegrations,
        tags: input.tags
      });
      data = response.data as typeof data;
    } catch (err: unknown) {
      const message = nangoErrorMessage(err, 'Nango SDK createConnectSession failed');
      throw new HttpError(502, 'nango_connect_session_failed', message);
    }

    const token = typeof data?.token === 'string' ? data.token : '';
    const connectLink = typeof data?.connect_link === 'string' ? data.connect_link : '';
    const expiresAt = typeof data?.expires_at === 'string' ? data.expires_at : '';

    if (!token || !connectLink || !expiresAt) {
      throw new HttpError(502, 'nango_connect_session_invalid', 'Nango SDK returned an incomplete connect session payload');
    }

    return {
      token,
      connectLink: withConnectApiUrl(connectLink, this.publicBaseUrl),
      expiresAt
    };
  }

  async getConnectionToken(providerConfigKey: string, connectionId: string): Promise<string> {
    if (this.client === null) {
      return `stub_token_${randomUUID()}`;
    }
    let connection: Awaited<ReturnType<Nango['getConnection']>>;
    try {
      connection = await this.client.getConnection(providerConfigKey, connectionId);
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : 'Nango getConnection failed';
      throw new HttpError(502, 'nango_get_connection_failed', message);
    }
    const token = (connection.credentials as { access_token?: string }).access_token;
    if (!token) {
      throw new HttpError(502, 'nango_missing_access_token',
        `No access token for connection ${connectionId}`);
    }
    return token;
  }
}

export function withConnectApiUrl(connectLink: string, publicBaseUrl?: string): string {
  const apiUrl = normaliseBaseUrl(publicBaseUrl);
  if (!apiUrl) return connectLink;

  try {
    const url = new URL(connectLink);
    if (!url.searchParams.has('apiURL')) {
      url.searchParams.set('apiURL', apiUrl);
    }
    return url.toString();
  } catch {
    const separator = connectLink.includes('?') ? '&' : '?';
    return `${connectLink}${separator}apiURL=${encodeURIComponent(apiUrl)}`;
  }
}

function normaliseBaseUrl(baseUrl?: string): string | undefined {
  const trimmed = baseUrl?.trim();
  if (!trimmed) return undefined;
  return trimmed.replace(/\/+$/, '');
}

function nangoErrorMessage(error: unknown, fallback: string): string {
  if (!error || typeof error !== 'object') {
    return fallback;
  }

  const record = error as {
    message?: unknown;
    response?: {
      data?: unknown;
      status?: unknown;
    };
  };
  const responseData = record.response?.data;

  if (responseData && typeof responseData === 'object') {
    const responseRecord = responseData as Record<string, unknown>;
    const nestedError = responseRecord.error;

    if (nestedError && typeof nestedError === 'object') {
      const nestedRecord = nestedError as Record<string, unknown>;
      const errors = nestedRecord.errors;
      if (Array.isArray(errors) && errors.length > 0) {
        const first = errors[0] as Record<string, unknown>;
        if (typeof first.message === 'string') return first.message;
      }
      if (typeof nestedRecord.message === 'string') return nestedRecord.message;
      if (typeof nestedRecord.code === 'string') return nestedRecord.code;
    }

    if (typeof responseRecord.message === 'string') return responseRecord.message;
    if (typeof responseRecord.error === 'string') return responseRecord.error;
  }

  if (typeof record.message === 'string') return record.message;
  return fallback;
}
