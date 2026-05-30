import { randomUUID } from 'node:crypto';

import { Pool } from 'pg';

export interface ConnectionMapping {
  id: string;
  organizationId: string;
  workspaceId: string;
  userId: string;
  userEmail: string | null;
  providerKey: string;
  providerLabel: string;
  nangoConnectionId: string;
  nangoIntegrationId: string;
  status: string;
  lastSyncStatus: string | null;
  lastSyncSummary: Record<string, unknown> | null;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
}

export interface ConnectionMappingFilters {
  organizationId?: string;
  providerKey?: string;
  userId?: string;
  workspaceId?: string;
}

export interface UpsertConnectionMappingInput {
  organizationId: string;
  workspaceId: string;
  userId: string;
  userEmail: string | null;
  providerKey: string;
  providerLabel: string;
  nangoConnectionId: string;
  nangoIntegrationId: string;
  status: string;
  lastSyncStatus?: string | null;
  lastSyncSummary?: Record<string, unknown> | null;
}

export interface RecordedWebhookEvent {
  eventType: string;
  id: string;
  nangoConnectionId?: string;
  operation?: string | null;
  payload: Record<string, unknown>;
  source: string;
}

export interface ConnectionMappingRepository {
  getById(id: string): Promise<ConnectionMapping | null>;
  list(filters: ConnectionMappingFilters): Promise<ConnectionMapping[]>;
  markDeleted(id: string): Promise<ConnectionMapping | null>;
  recordWebhookEvent(event: RecordedWebhookEvent): Promise<void>;
  upsertFromAuthWebhook(input: UpsertConnectionMappingInput): Promise<ConnectionMapping>;
}

type ConnectionRow = {
  id: string;
  organization_id: string;
  workspace_id: string;
  user_id: string;
  user_email: string | null;
  provider_key: string;
  provider_label: string;
  nango_connection_id: string;
  nango_integration_id: string;
  status: string;
  last_sync_status: string | null;
  last_sync_summary: Record<string, unknown> | null;
  created_at: Date;
  updated_at: Date;
  deleted_at: Date | null;
};

export class PgConnectionMappingRepository implements ConnectionMappingRepository {
  constructor(private readonly pool: Pool) {}

  async getById(id: string): Promise<ConnectionMapping | null> {
    const result = await this.pool.query<ConnectionRow>(
      `
        SELECT *
        FROM integration_connection_mappings
        WHERE id = $1
        LIMIT 1
      `,
      [id]
    );

    if (result.rowCount === 0) {
      return null;
    }

    return mapConnection(result.rows[0]);
  }

  async list(filters: ConnectionMappingFilters): Promise<ConnectionMapping[]> {
    const where: string[] = ['deleted_at IS NULL'];
    const values: string[] = [];

    if (filters.organizationId) {
      values.push(filters.organizationId);
      where.push(`organization_id = $${values.length}`);
    }

    if (filters.workspaceId) {
      values.push(filters.workspaceId);
      where.push(`workspace_id = $${values.length}`);
    }

    if (filters.userId) {
      values.push(filters.userId);
      where.push(`user_id = $${values.length}`);
    }

    if (filters.providerKey) {
      values.push(filters.providerKey);
      where.push(`provider_key = $${values.length}`);
    }

    const result = await this.pool.query<ConnectionRow>(
      `
        SELECT *
        FROM integration_connection_mappings
        WHERE ${where.join(' AND ')}
        ORDER BY created_at DESC
      `,
      values
    );

    return result.rows.map(mapConnection);
  }

  async markDeleted(id: string): Promise<ConnectionMapping | null> {
    const result = await this.pool.query<ConnectionRow>(
      `
        UPDATE integration_connection_mappings
        SET deleted_at = NOW(),
            status = 'deleted',
            updated_at = NOW()
        WHERE id = $1
        RETURNING *
      `,
      [id]
    );

    if (result.rowCount === 0) {
      return null;
    }

    return mapConnection(result.rows[0]);
  }

  async recordWebhookEvent(event: RecordedWebhookEvent): Promise<void> {
    await this.pool.query(
      `
        INSERT INTO integration_webhook_events (
          id,
          source,
          event_type,
          operation,
          nango_connection_id,
          payload
        ) VALUES ($1, $2, $3, $4, $5, $6::jsonb)
      `,
      [
        event.id,
        event.source,
        event.eventType,
        event.operation ?? null,
        event.nangoConnectionId ?? null,
        JSON.stringify(event.payload)
      ]
    );
  }

  async upsertFromAuthWebhook(input: UpsertConnectionMappingInput): Promise<ConnectionMapping> {
    const result = await this.pool.query<ConnectionRow>(
      `
        INSERT INTO integration_connection_mappings (
          id,
          organization_id,
          workspace_id,
          user_id,
          user_email,
          provider_key,
          provider_label,
          nango_connection_id,
          nango_integration_id,
          status,
          last_sync_status,
          last_sync_summary
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::jsonb)
        ON CONFLICT (nango_connection_id)
        DO UPDATE SET
          organization_id = EXCLUDED.organization_id,
          workspace_id = EXCLUDED.workspace_id,
          user_id = EXCLUDED.user_id,
          user_email = EXCLUDED.user_email,
          provider_key = EXCLUDED.provider_key,
          provider_label = EXCLUDED.provider_label,
          nango_integration_id = EXCLUDED.nango_integration_id,
          status = EXCLUDED.status,
          last_sync_status = EXCLUDED.last_sync_status,
          last_sync_summary = EXCLUDED.last_sync_summary,
          deleted_at = NULL,
          updated_at = NOW()
        RETURNING *
      `,
      [
        randomUUID(),
        input.organizationId,
        input.workspaceId,
        input.userId,
        input.userEmail,
        input.providerKey,
        input.providerLabel,
        input.nangoConnectionId,
        input.nangoIntegrationId,
        input.status,
        input.lastSyncStatus ?? null,
        JSON.stringify(input.lastSyncSummary ?? null)
      ]
    );

    return mapConnection(result.rows[0]);
  }
}

function mapConnection(row: ConnectionRow): ConnectionMapping {
  return {
    id: row.id,
    organizationId: row.organization_id,
    workspaceId: row.workspace_id,
    userId: row.user_id,
    userEmail: row.user_email,
    providerKey: row.provider_key,
    providerLabel: row.provider_label,
    nangoConnectionId: row.nango_connection_id,
    nangoIntegrationId: row.nango_integration_id,
    status: row.status,
    lastSyncStatus: row.last_sync_status,
    lastSyncSummary: row.last_sync_summary,
    createdAt: row.created_at.toISOString(),
    updatedAt: row.updated_at.toISOString(),
    deletedAt: row.deleted_at ? row.deleted_at.toISOString() : null
  };
}