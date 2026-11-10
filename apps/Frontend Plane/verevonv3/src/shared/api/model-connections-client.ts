import { requestJson } from './http'

/**
 * The organisation's customer-gateway model connections (W4-BYO-6). Integration Core seals each key and
 * never returns it, so no shape here carries one. A customer gateway is never zero data retention and is the
 * lowest privacy tier; the settings section and the model picker say so.
 */
export type CustomerGatewayConnection = Readonly<{
  id: string
  name: string
  baseUrl: string
  status: string
  modelIds: readonly string[]
}>

export type CustomerGatewayInput = Readonly<{
  name: string
  baseUrl: string
  /** Sent once and never kept: the caller clears its field after the request. */
  apiKey: string
  modelIds: readonly string[]
}>

const CUSTOMER_GATEWAY_PATH = '/api/v1/model-connections/customer-gateway'

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

function textOf(value: unknown): string {
  return typeof value === 'string' ? value.trim() : ''
}

/** Model ids from a list or a comma-separated string, trimmed, non-empty and unique. */
function modelIdsOf(value: unknown): string[] {
  const parts: unknown[] = Array.isArray(value) ? value : typeof value === 'string' ? value.split(',') : []
  const ids = parts.map(textOf).filter((id) => id !== '')
  return ids.filter((id, index) => ids.indexOf(id) === index)
}

/** One connection as Integration Core reports it. A row without an id or a name is dropped, not guessed at. */
function connectionOf(raw: unknown): CustomerGatewayConnection | undefined {
  const row = asRecord(raw)
  if (!row) return undefined
  const context: Record<string, unknown> = asRecord(row.providerContext) ?? asRecord(row.provider_context) ?? {}
  const id = textOf(row.id)
  const name = textOf(row.name) || textOf(row.displayName)
  if (!id || !name) return undefined
  return {
    id,
    name,
    baseUrl: textOf(row.baseUrl) || textOf(row.base_url) || textOf(context.base_url),
    status: textOf(row.status),
    modelIds: modelIdsOf(row.modelIds ?? row.model_ids ?? context.model_ids),
  }
}

/** The organisation's customer gateways (`GET`). Each one lists the model ids it serves. */
export async function listCustomerGateways(): Promise<CustomerGatewayConnection[]> {
  const raw = await requestJson<unknown>(CUSTOMER_GATEWAY_PATH)
  const rows: unknown[] = Array.isArray(raw)
    ? raw
    : Array.isArray(asRecord(raw)?.connections)
      ? (asRecord(raw)?.connections as unknown[])
      : []
  return rows
    .map(connectionOf)
    .filter((connection): connection is CustomerGatewayConnection => connection !== undefined)
}

/** Registers a customer gateway (`POST`, owners and admins only). Returns the new connection, without its key. */
export async function createCustomerGateway(input: CustomerGatewayInput): Promise<CustomerGatewayConnection | undefined> {
  const raw = await requestJson<unknown>(CUSTOMER_GATEWAY_PATH, {
    method: 'POST',
    body: JSON.stringify({
      name: input.name.trim(),
      base_url: input.baseUrl.trim(),
      api_key: input.apiKey,
      model_ids: [...input.modelIds],
    }),
  })
  return connectionOf(asRecord(raw)?.connection)
}

/** Removes one customer gateway (`DELETE`, owners and admins only). Its key is discarded upstream. */
export async function deleteCustomerGateway(connectionId: string): Promise<void> {
  await requestJson<unknown>(CUSTOMER_GATEWAY_PATH, {
    method: 'DELETE',
    body: JSON.stringify({ connection_id: connectionId.trim() }),
  })
}
