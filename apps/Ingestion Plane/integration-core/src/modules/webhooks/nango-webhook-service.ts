import { randomUUID } from 'node:crypto';

import { AppConfig } from '../../common/config/app-config';
import { HttpError } from '../../common/http/http-error';
import { IntegrationEventPublisher } from '../../common/nats/event-publisher';
import { IntegrationSubject, integrationSubjects } from '../../common/nats/subjects';
import { extractWebhookSignature, verifyWebhookSignature } from '../../common/security/webhook-signature';
import {
  ConnectionMapping,
  ConnectionMappingRepository
} from '../connections/connection-mapping-repository';
import {
  ConnectionIntroIngestService,
  ConnectionIntroSnapshot
} from '../data-plane/connection-intro-ingest-service';
import { getProviderDefinition } from '../providers/provider-catalog';

type NangoWebhookPayload = {
  type?: string;
  operation?: string;
  success?: boolean;
  connectionId?: string;
  provider_config_key?: string;
  last_sync_status?: string;
  tags?: Record<string, string>;
  [key: string]: unknown;
};

type RuntimeEnvironmentResponse = {
  environmentAndAccount?: {
    webhook_signing_key?: string;
  };
};

export interface NangoWebhookResult {
  accepted: boolean;
  connection?: ConnectionMapping;
  webhookEventId: string;
}

export class NangoWebhookService {
  private runtimeWebhookSigningKey: { value: string; expiresAt: number } | null = null;

  constructor(
    private readonly config: AppConfig,
    private readonly repository: ConnectionMappingRepository,
    private readonly eventPublisher: IntegrationEventPublisher,
    private readonly introIngestService?: ConnectionIntroIngestService
  ) {}

  async handle(rawBody: string, headers: Record<string, string | string[] | undefined>): Promise<NangoWebhookResult> {
    const signature = extractWebhookSignature(headers);

    if (!(await this.isValidSignature(rawBody, signature))) {
      throw new HttpError(401, 'invalid_nango_webhook_signature', 'Invalid Nango webhook signature');
    }

    let payload: NangoWebhookPayload;

    try {
      payload = JSON.parse(rawBody) as NangoWebhookPayload;
    } catch {
      throw new HttpError(400, 'invalid_json', 'Webhook payload must be valid JSON');
    }

    const webhookEventId = randomUUID();

    await this.repository.recordWebhookEvent({
      id: webhookEventId,
      source: 'nango',
      eventType: typeof payload.type === 'string' ? payload.type : 'unknown',
      operation: typeof payload.operation === 'string' ? payload.operation : null,
      nangoConnectionId: typeof payload.connectionId === 'string' ? payload.connectionId : undefined,
      payload: payload as Record<string, unknown>
    });

    if (payload.type !== 'auth') {
      return {
        accepted: true,
        webhookEventId
      };
    }

    return this.handleAuthWebhook(payload, webhookEventId);
  }

  private async handleAuthWebhook(payload: NangoWebhookPayload, webhookEventId: string): Promise<NangoWebhookResult> {
    const tags = payload.tags ?? {};
    const nangoConnectionId = requiredString(payload.connectionId, 'connectionId');
    const providerKey = requiredString(tags.provider_key, 'tags.provider_key');
    const provider = getProviderDefinition(this.config, providerKey);

    if (!provider) {
      throw new HttpError(422, 'provider_not_supported', 'Unsupported provider in webhook payload');
    }

    const organizationId = requiredString(tags.organization_id, 'tags.organization_id');
    const workspaceId = requiredString(tags.workspace_id, 'tags.workspace_id');
    const userId = requiredString(tags.end_user_id, 'tags.end_user_id');
    const userEmail = optionalString(tags.end_user_email);
    const operation = typeof payload.operation === 'string' ? payload.operation : 'creation';
    const selectedSources = optionalString(tags.selected_sources)
      ?.split(',')
      .map((source) => source.trim())
      .filter(Boolean) ?? [];

    if (!payload.success) {
      await this.publishBestEffort(integrationSubjects.connectionAuthFailed, {
        organizationId,
        providerKey: provider.key,
        workspaceId,
        userId,
        userEmail,
        nangoConnectionId,
        webhookEventId
      });

      return {
        accepted: true,
        webhookEventId
      };
    }

    const intro = await this.buildIntroBestEffort({
      nangoConnectionId,
      nangoIntegrationId: provider.nangoIntegrationId,
      providerKey: provider.key,
      selectedSources
    });

    const connection = await this.repository.upsertFromAuthWebhook({
      organizationId,
      workspaceId,
      userId,
      userEmail,
      providerKey: provider.key,
      providerLabel: provider.label,
      nangoConnectionId,
      nangoIntegrationId: provider.nangoIntegrationId,
      status: 'connected',
      lastSyncStatus: typeof payload.last_sync_status === 'string' ? payload.last_sync_status : null,
      lastSyncSummary: {
        introError: intro?.error,
        introFetchedAt: intro?.fetchedAt,
        introItems: intro?.items ?? [],
        operation,
        selectedSources,
        webhookEventId
      }
    });

    await this.ingestIntroBestEffort(connection, selectedSources, webhookEventId, intro);

    await this.publishBestEffort(
      operation === 'override'
        ? integrationSubjects.connectionUpdated
        : integrationSubjects.connectionCreated,
      {
        connectionId: connection.id,
        nangoConnectionId: connection.nangoConnectionId,
        nangoIntegrationId: connection.nangoIntegrationId,
        organizationId: connection.organizationId,
        providerKey: connection.providerKey,
        status: connection.status,
        userId: connection.userId,
        workspaceId: connection.workspaceId
      }
    );

    return {
      accepted: true,
      connection,
      webhookEventId
    };
  }

  private async ingestIntroBestEffort(
    connection: ConnectionMapping,
    selectedSources: string[],
    webhookEventId: string,
    intro?: ConnectionIntroSnapshot
  ): Promise<void> {
    if (!this.introIngestService) return;

    try {
      await this.introIngestService.ingestConnectionIntro({
        connection,
        intro,
        selectedSources,
        webhookEventId
      });
    } catch (error) {
      console.error('Failed to ingest integration intro document', {
        connectionId: connection.id,
        error,
        providerKey: connection.providerKey
      });
    }
  }

  private async buildIntroBestEffort(input: {
    nangoConnectionId: string;
    nangoIntegrationId: string;
    providerKey: string;
    selectedSources: string[];
  }): Promise<ConnectionIntroSnapshot | undefined> {
    if (!this.introIngestService) return undefined;

    try {
      return await this.introIngestService.buildIntroSnapshot(input);
    } catch (error) {
      console.error('Failed to build integration intro snapshot', {
        error,
        providerKey: input.providerKey
      });
      return undefined;
    }
  }

  private async publishBestEffort(
    subject: IntegrationSubject,
    payload: Record<string, unknown>
  ): Promise<void> {
    try {
      await this.eventPublisher.publish(subject, payload);
    } catch (error) {
      console.error('Failed to publish integration event', {
        error,
        payload,
        subject
      });
    }
  }

  private async isValidSignature(rawBody: string, signature: string | null): Promise<boolean> {
    if (verifyWebhookSignature(rawBody, signature, this.config.connectorRuntimeWebhookSecret)) {
      return true;
    }

    const runtimeSecret = await this.getRuntimeWebhookSigningKey();
    return verifyWebhookSignature(rawBody, signature, runtimeSecret);
  }

  private async getRuntimeWebhookSigningKey(): Promise<string | undefined> {
    const now = Date.now();
    if (this.runtimeWebhookSigningKey && this.runtimeWebhookSigningKey.expiresAt > now) {
      return this.runtimeWebhookSigningKey.value;
    }

    try {
      const response = await fetch(
        `${this.config.connectorRuntimeBaseUrl.replace(/\/+$/, '')}/api/v1/environments/current?env=dev`,
        {
          method: 'GET',
          headers: {
            Accept: 'application/json',
            Authorization: `Bearer ${this.config.connectorRuntimeSecret}`,
          },
          signal: AbortSignal.timeout(this.config.connectorRuntimeTimeoutMs),
        }
      );

      if (!response.ok) {
        return undefined;
      }

      const payload = (await response.json()) as RuntimeEnvironmentResponse;
      const value = payload.environmentAndAccount?.webhook_signing_key;
      if (typeof value !== 'string' || value.trim().length === 0) {
        return undefined;
      }

      this.runtimeWebhookSigningKey = {
        value: value.trim(),
        expiresAt: now + 5 * 60_000,
      };
      return this.runtimeWebhookSigningKey.value;
    } catch {
      return undefined;
    }
  }
}

function optionalString(value: unknown): string | null {
  return typeof value === 'string' && value.trim().length > 0 ? value.trim() : null;
}

function requiredString(value: unknown, field: string): string {
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new HttpError(422, 'invalid_webhook_payload', `Missing required field: ${field}`);
  }

  return value.trim();
}
