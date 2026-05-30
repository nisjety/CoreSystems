import { FastifyInstance } from 'fastify';

import { AppConfig } from '../../common/config/app-config';
import { successResponse } from '../../common/http/http-error';
import { NangoManagementService } from '../connectors/nango-management-service';
import { listProviders } from './provider-catalog';

export function registerProviderRoutes(
  app: FastifyInstance,
  config: AppConfig,
  nangoMgmt?: NangoManagementService
): void {
  app.get('/api/v1/providers', async () => {
    const providers = listProviders(config);
    let configuredIntegrationKeys: Set<string> | null = null;

    if (nangoMgmt) {
      try {
        const integrations = await nangoMgmt.listIntegrations();
        configuredIntegrationKeys = new Set(integrations.map((integration) => integration.key));
      } catch {
        configuredIntegrationKeys = new Set();
      }
    }

    return successResponse({
      providers: providers.map((provider) => ({
        key: provider.key,
        label: provider.label,
        description: provider.description,
        sources: provider.sources,
        supported_sources: provider.sources,
        default_sources: provider.sources,
        auth_execution: 'connector_runtime',
        sync_execution: 'connector_runtime',
        configured: configuredIntegrationKeys
          ? configuredIntegrationKeys.has(provider.nangoIntegrationId)
          : true
      }))
    });
  });
}
