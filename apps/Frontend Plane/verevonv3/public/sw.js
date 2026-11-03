// Verevon service worker (WS16-S6). It handles only Web Push: a push asks the user to look at the attention rail,
// and a click opens it. It has no fetch handler and never uses the Cache API, so the worker stores no page, no API
// response and no conversation. A push carries a constant wake-up and nothing else; the copy below is fixed.

const ATTENTION_URL = '/chat?filter=attention'
const ATTENTION_TAG = 'verevon-attention'

self.addEventListener('install', () => {
  self.skipWaiting()
})

self.addEventListener('activate', (event) => {
  event.waitUntil(self.clients.claim())
})

self.addEventListener('push', (event) => {
  event.waitUntil(showAttentionNotice())
})

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  event.waitUntil(openAttention())
})

self.addEventListener('pushsubscriptionchange', (event) => {
  event.waitUntil(renewSubscription())
})

// A visible or focused window already shows the attention rail, so no notice is shown for that push.
async function showAttentionNotice() {
  const windows = await self.clients.matchAll({ type: 'window', includeUncontrolled: true })
  if (windows.some((client) => client.visibilityState === 'visible' || client.focused)) {
    return
  }
  await self.registration.showNotification('Verevon', {
    body: 'A chat needs your attention.',
    tag: ATTENTION_TAG,
    icon: '/icons/verevon-mark.svg',
    data: { url: ATTENTION_URL },
  })
}

async function openAttention() {
  const windows = await self.clients.matchAll({ type: 'window', includeUncontrolled: true })
  const client = windows.find((candidate) => 'focus' in candidate)
  if (client) {
    try {
      await client.navigate(ATTENTION_URL)
      return client.focus()
    } catch {
      // A client this worker cannot navigate is left alone; a new window opens the rail instead.
    }
  }
  return self.clients.openWindow(ATTENTION_URL)
}

// The browser replaced its subscription. The new endpoint is registered for the same signed-in owner with the
// session cookie. The old endpoint is revoked by the push service's own 404 or 410 on the next wake-up.
async function renewSubscription() {
  const configResponse = await fetch('/api/v1/push/config', { credentials: 'same-origin' })
  if (!configResponse.ok) return
  const config = await configResponse.json()
  const publicKey = config?.public_key
  if (!config?.enabled || !publicKey) return

  const subscription = await self.registration.pushManager.subscribe({
    userVisibleOnly: true,
    applicationServerKey: base64UrlToBytes(publicKey),
  })
  const json = subscription.toJSON()
  await fetch('/api/v1/push/subscriptions', {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ endpoint: json.endpoint, keys: { p256dh: json.keys.p256dh, auth: json.keys.auth } }),
  })
}

function base64UrlToBytes(value) {
  const base64 = value.replace(/-/g, '+').replace(/_/g, '/')
  const binary = atob(base64.padEnd(Math.ceil(base64.length / 4) * 4, '='))
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}
