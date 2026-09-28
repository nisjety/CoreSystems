import { ApiError, requestJson } from './http'

/**
 * The personal inbound-mail alias (chat-wave2 mail-task spec, owner ruling
 * 2026-09-28: Postmark inbound, self-forward only). Proxied by the gateway's
 * `src/domains/settings/mail_task.rs` straight through to user-core's
 * `/api/v1/users/me/mail-task-address` — the org and user are resolved
 * server-side from the session, never sent by the client.
 *
 * GET/POST both idempotently get-or-create the address (user-core's
 * `GenerateAlias` is itself idempotent per org/user); DELETE revokes the
 * current alias and mints its replacement in the same call, so
 * "Generer på nytt / Regenerate" is DELETE-then-GET, never a window with no
 * address at all.
 */
export interface MailTaskAddress {
  address: string
}

/** 503 means the feature is not configured on this deployment yet (the owner
 * has not finished the Postmark account + DNS setup) — distinct from a
 * transport failure so the section can render "not available yet" instead of
 * a generic error. */
export class MailTaskNotConfiguredError extends Error {
  constructor() {
    super('mail-task is not configured on this deployment')
    this.name = 'MailTaskNotConfiguredError'
  }
}

async function requestAddress(method: 'GET' | 'POST' | 'DELETE'): Promise<MailTaskAddress> {
  try {
    return await requestJson<MailTaskAddress>('/api/v1/users/me/mail-task-address', { method })
  } catch (error) {
    if (error instanceof ApiError && error.status === 503) {
      throw new MailTaskNotConfiguredError()
    }
    throw error
  }
}

export function getMailTaskAddress(): Promise<MailTaskAddress> {
  return requestAddress('GET')
}

export function createMailTaskAddress(): Promise<MailTaskAddress> {
  return requestAddress('POST')
}

/** Revokes the current alias and mints its replacement, then reads it back. */
export async function regenerateMailTaskAddress(): Promise<MailTaskAddress> {
  try {
    await requestJson<void>('/api/v1/users/me/mail-task-address', { method: 'DELETE' })
  } catch (error) {
    if (error instanceof ApiError && error.status === 503) {
      throw new MailTaskNotConfiguredError()
    }
    throw error
  }
  return getMailTaskAddress()
}
