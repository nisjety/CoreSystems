import { requestJson } from './http'

export type ComposerSettingsItem = {
  connected?: boolean
  description?: string
  id: string
  name: string
}

export async function loadComposerSettingsItems(
  endpoint: string,
  itemKey: string,
  signal?: AbortSignal,
): Promise<ComposerSettingsItem[]> {
  const payload = await requestJson<unknown>(endpoint, { signal })
  return extractComposerSettingsItems(payload, itemKey)
}

function extractComposerSettingsItems(payload: unknown, itemKey: string): ComposerSettingsItem[] {
  const source = arrayFromPayload(payload, itemKey)

  return source
    .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object')
    .map((item, index) => {
      const fallbackId = `${itemKey}-${index}`
      return {
        id: stringFrom(item.id) ?? stringFrom(item.key) ?? stringFrom(item.providerKey) ?? fallbackId,
        name:
          stringFrom(item.name) ??
          stringFrom(item.title) ??
          stringFrom(item.label) ??
          stringFrom(item.key) ??
          'Untitled',
        description:
          stringFrom(item.description) ??
          stringFrom(item.sub) ??
          stringFrom(item.summary) ??
          stringFrom(item.status),
        connected:
          booleanFrom(item.connected) ??
          booleanFrom(item.configured) ??
          booleanFrom(item.ready),
      }
    })
}

function arrayFromPayload(payload: unknown, itemKey: string): unknown[] {
  if (Array.isArray(payload)) return payload
  if (!payload || typeof payload !== 'object') return []

  const record = payload as Record<string, unknown>
  if (Array.isArray(record[itemKey])) return record[itemKey]

  const nestedData = record.data
  if (nestedData && typeof nestedData === 'object') {
    const nested = nestedData as Record<string, unknown>
    if (Array.isArray(nested[itemKey])) return nested[itemKey]
    if (Array.isArray(nested.items)) return nested.items
  }

  if (Array.isArray(record.items)) return record.items
  if (Array.isArray(record.results)) return record.results
  return []
}

function stringFrom(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim().length > 0 ? value : undefined
}

function booleanFrom(value: unknown): boolean | undefined {
  return typeof value === 'boolean' ? value : undefined
}
