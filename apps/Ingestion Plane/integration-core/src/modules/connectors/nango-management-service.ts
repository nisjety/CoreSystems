import { AppConfig } from '../../common/config/app-config';
import { HttpError } from '../../common/http/http-error';

/**
 * Nango integration management service
 * Handles OAuth flows, connection status, and connector configuration
 */
export interface NangoIntegrationInfo {
  id: string;
  key: string;
  name: string;
  category: string;
  docs: string;
  enabled: boolean;
}

export interface NangoConnectionStatus {
  connectionId: string;
  integrationId: string;
  status: 'active' | 'inactive' | 'error';
  lastSyncTime?: string;
  errorMessage?: string;
}

export interface InitiateNangoAuthRequest {
  providerKey: string;
  organizationId: string;
  workspaceId: string;
  userId: string;
  userEmail?: string;
  redirectUrl: string;
}

export interface InitiateNangoAuthResponse {
  authUrl: string;
  state: string;
  expiresIn: number;
}

export class NangoManagementService {
  constructor(private readonly config: AppConfig) {}

  private get baseUrl(): string {
    return this.config.connectorRuntimeBaseUrl.replace(/\/+$/, '');
  }

  private get authHeader(): string {
    return `Bearer ${this.config.connectorRuntimeSecret}`;
  }

  private async nangoGet<T>(path: string): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      method: 'GET',
      headers: {
        Authorization: this.authHeader,
        Accept: 'application/json',
      },
      signal: AbortSignal.timeout(this.config.connectorRuntimeTimeoutMs),
    });

    if (!response.ok) {
      throw new HttpError(
        response.status === 401 ? 502 : response.status,
        'nango_request_failed',
        `Nango runtime returned ${response.status} for ${path}`,
      );
    }

    return (await response.json()) as T;
  }

  /**
   * List integrations configured in the self-hosted Nango runtime.
   */
  async listIntegrations(): Promise<NangoIntegrationInfo[]> {
    try {
      const data = await this.nangoGet<{
        data?: Array<Record<string, unknown>>;
        integrations?: Array<Record<string, unknown>>;
      }>('/integrations');
      const integrations = data.data ?? data.integrations ?? [];
      return integrations.map((integration) => ({
        id: String(integration.unique_key ?? integration.id ?? integration.key ?? ''),
        key: String(integration.unique_key ?? integration.key ?? ''),
        name: String(integration.display_name ?? integration.name ?? integration.provider ?? ''),
        category: String(integration.provider ?? integration.category ?? 'other'),
        docs: String(integration.docs ?? ''),
        enabled: true,
      })).filter((integration) => integration.key.length > 0);
    } catch (err) {
      if (err instanceof HttpError) throw err;
      throw new HttpError(500, 'nango_service_error', 'Failed to list Nango integrations');
    }
  }

  /**
   * Get integration details by key
   */
  async getIntegration(integrationKey: string): Promise<NangoIntegrationInfo | null> {
    const integrations = await this.listIntegrations();
    return integrations.find((i) => i.key === integrationKey) || null;
  }

  /**
   * Check connection status
   */
  async checkConnectionStatus(connectionId: string): Promise<NangoConnectionStatus> {
    try {
      const data = await this.nangoGet<{
        connection_id?: string;
        connectionId?: string;
        provider_config_key?: string;
        integrationId?: string;
        status: string;
        lastSyncTime?: string;
        errorMessage?: string;
      }>(`/connection/${encodeURIComponent(connectionId)}`);

      return {
        connectionId: data.connectionId ?? data.connection_id ?? connectionId,
        integrationId: data.integrationId ?? data.provider_config_key ?? '',
        status: (data.status as 'active' | 'inactive' | 'error') || 'inactive',
        lastSyncTime: data.lastSyncTime,
        errorMessage: data.errorMessage,
      };
    } catch (err) {
      if (err instanceof HttpError) throw err;
      throw new HttpError(500, 'nango_service_error', 'Failed to check connection status');
    }
  }
}
