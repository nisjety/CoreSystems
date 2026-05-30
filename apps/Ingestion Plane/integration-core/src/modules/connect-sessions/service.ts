import { AppConfig } from '../../common/config/app-config';
import { HttpError } from '../../common/http/http-error';
import { ConnectSessionResult, NangoRuntimeClient } from '../../common/runtime/nango-runtime-client';
import { getProviderDefinition } from '../providers/provider-catalog';

export interface CreateConnectSessionRequest {
  providerKey: string;
  organizationId: string;
  selectedSources?: string[];
  workspaceId: string;
  userId: string;
  userEmail: string;
}

export interface CreateConnectSessionResponse {
  sessionToken: string;
  connectUrl: string;
  expiresAt: string;
  provider: {
    key: string;
    label: string;
  };
}

export class ConnectSessionService {
  constructor(
    private readonly config: AppConfig,
    private readonly runtimeClient: NangoRuntimeClient
  ) {}

  async createSession(request: CreateConnectSessionRequest): Promise<CreateConnectSessionResponse> {
    const provider = getProviderDefinition(this.config, request.providerKey);

    if (!provider) {
      throw new HttpError(404, 'provider_not_found', `Unsupported provider: ${request.providerKey}`);
    }

    const session = await this.runtimeClient.createConnectSession({
      allowedIntegrations: [provider.nangoIntegrationId],
      tags: {
        end_user_email: request.userEmail.trim(),
        end_user_id: request.userId.trim(),
        organization_id: request.organizationId.trim(),
        provider_key: provider.key,
        selected_sources: (request.selectedSources ?? []).join(','),
        workspace_id: request.workspaceId.trim()
      }
    });

    return this.toResponse(provider.key, provider.label, session);
  }

  private toResponse(providerKey: string, providerLabel: string, session: ConnectSessionResult): CreateConnectSessionResponse {
    return {
      sessionToken: session.token,
      connectUrl: session.connectLink,
      expiresAt: session.expiresAt,
      provider: {
        key: providerKey,
        label: providerLabel
      }
    };
  }
}
