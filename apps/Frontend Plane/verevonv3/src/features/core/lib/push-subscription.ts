import { requestJson } from '@/shared/api/http'

/** What the server offers: whether it accepts Web Push and the application server key to subscribe with. */
export type PushConfig = { enabled: boolean; publicKey: string | null }

/** Whether this browser can take a Web Push subscription at all. */
export type PushSupport = 'supported' | 'insecure' | 'unsupported'

/** The body the server stores for one browser subscription. */
export type PushSubscriptionBody = { endpoint: string; keys: { p256dh: string; auth: string } }

const SERVICE_WORKER_PATH = '/sw.js'

export function pushSupport(): PushSupport {
  if (typeof window === 'undefined' || typeof navigator === 'undefined') return 'unsupported'
  if (!window.isSecureContext) return 'insecure'
  const capable = 'serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window
  return capable ? 'supported' : 'unsupported'
}

/** Decodes the base64url key the server publishes into the bytes PushManager expects. */
export function base64UrlToBytes(value: string): Uint8Array<ArrayBuffer> {
  const base64 = value.replace(/-/g, '+').replace(/_/g, '/')
  const binary = atob(base64.padEnd(Math.ceil(base64.length / 4) * 4, '='))
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}

/** Encodes one of the browser's subscription keys the way the server stores it: base64url, no padding. */
export function bytesToBase64Url(buffer: ArrayBufferLike | null): string {
  if (!buffer) return ''
  let binary = ''
  for (const byte of new Uint8Array(buffer)) {
    binary += String.fromCharCode(byte)
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

export function subscriptionBody(subscription: Pick<PushSubscription, 'endpoint' | 'getKey'>): PushSubscriptionBody {
  return {
    endpoint: subscription.endpoint,
    keys: {
      p256dh: bytesToBase64Url(subscription.getKey('p256dh')),
      auth: bytesToBase64Url(subscription.getKey('auth')),
    },
  }
}

export async function fetchPushConfig(): Promise<PushConfig> {
  const body = await requestJson<{ enabled: boolean; public_key?: string }>('/api/v1/push/config')
  return { enabled: body.enabled === true, publicKey: body.public_key ?? null }
}

/** Registers the worker that receives pushes. Its scope is the origin root; it caches nothing. */
export async function registerAttentionWorker(): Promise<ServiceWorkerRegistration> {
  await navigator.serviceWorker.register(SERVICE_WORKER_PATH, { scope: '/' })
  return navigator.serviceWorker.ready
}

/** Whether this browser currently holds a push subscription. Read-only: it never asks for permission. */
export async function deviceHasPushSubscription(): Promise<boolean> {
  if (pushSupport() !== 'supported') return false
  const registration = await navigator.serviceWorker.getRegistration('/')
  return Boolean(await registration?.pushManager.getSubscription())
}

/**
 * Turns push on for this browser. It asks for notification permission, which only a user gesture may do, so call it
 * from a click. Returns the server's id for the stored subscription.
 */
export async function enablePushOnThisDevice(publicKey: string): Promise<string> {
  const permission = await Notification.requestPermission()
  if (permission !== 'granted') {
    throw new Error('notification permission was not granted')
  }
  const registration = await registerAttentionWorker()
  const subscription =
    (await registration.pushManager.getSubscription()) ??
    (await registration.pushManager.subscribe({
      userVisibleOnly: true,
      applicationServerKey: base64UrlToBytes(publicKey),
    }))
  const stored = await requestJson<{ id: string }>('/api/v1/push/subscriptions', {
    method: 'POST',
    body: JSON.stringify(subscriptionBody(subscription)),
  })
  return stored.id
}

/**
 * Turns push off for this browser. The server id is removed when this session still knows it. Otherwise the browser
 * unsubscribes alone, and the push service's next 410 revokes the stored endpoint, so no push reaches this device.
 */
export async function disablePushOnThisDevice(serverId: string | null): Promise<void> {
  if (serverId) {
    await requestJson<void>(`/api/v1/push/subscriptions/${encodeURIComponent(serverId)}`, { method: 'DELETE' })
  }
  const registration = await navigator.serviceWorker.getRegistration('/')
  const subscription = await registration?.pushManager.getSubscription()
  await subscription?.unsubscribe()
}
