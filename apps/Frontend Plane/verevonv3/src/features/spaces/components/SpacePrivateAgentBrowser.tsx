import { createEffect, createMemo, createSignal, Show } from 'solid-js'
import type { BrowserAttachSharedJarEvent } from '@/shared/api/run-console-client'
import { useI18n } from '@/shared/i18n'

/**
 * An owner-private, read-only view of an attached agent's pinned tab.
 * The caller must obtain `receipt` from the authorized Model run stream;
 * matching identifiers here is presentation correlation, not authorization.
 * Quarry and the gateway must independently authorize every frame request.
 */
export function SpacePrivateAgentBrowser(props: {
  sessionId: string
  modelRunId: string
  receipt: BrowserAttachSharedJarEvent | null
}) {
  const i18n = useI18n()
  const [frame, setFrame] = createSignal<string | null>(null)
  const [state, setState] = createSignal<'waiting' | 'live' | 'disconnected'>('waiting')
  const sourceUrl = createMemo(() => {
    const receipt = props.receipt
    if (!receipt || !props.sessionId || !props.modelRunId || receipt.runId !== props.modelRunId ||
      receipt.sessionRunId !== props.sessionId || !receipt.tabId) return null
    const query = new URLSearchParams({ tabId: receipt.tabId })
    return `/api/v1/browser/sessions/${encodeURIComponent(props.sessionId)}/frames/stream?${query}`
  })

  createEffect(sourceUrl, (url) => {
    setFrame(null)
    setState('waiting')
    if (!url) return
    const source = new EventSource(url)
    let expiry: ReturnType<typeof setTimeout> | undefined
    const disconnected = () => {
      if (expiry !== undefined) clearTimeout(expiry)
      expiry = undefined
      setFrame(null)
      setState('disconnected')
    }
    source.addEventListener('frame', (event) => {
      try {
        const data: unknown = JSON.parse((event as MessageEvent<string>).data)
        if (!data || typeof data !== 'object') return
        const payload = data as Record<string, unknown>
        if (typeof payload.dataBase64 !== 'string' || payload.dataBase64.length === 0 ||
          payload.dataBase64.length > 8_000_000 || !/^[A-Za-z0-9+/]*={0,2}$/.test(payload.dataBase64)) return
        if (payload.mimeType !== 'image/jpeg' && payload.mimeType !== 'image/png') return
        setFrame(`data:${payload.mimeType};base64,${payload.dataBase64}`)
        setState('live')
        if (expiry !== undefined) clearTimeout(expiry)
        expiry = setTimeout(disconnected, 5000)
      } catch { /* A malformed frame is not evidence of a new image. */ }
    })
    source.addEventListener('error', disconnected)
    source.addEventListener('done', () => { disconnected(); source.close() })
    return () => { source.close(); if (expiry !== undefined) clearTimeout(expiry); setFrame(null) }
  })

  return (
    <section aria-label={i18n.tr('Privat agentnettleser', 'Private agent browser')}>
      <p>{i18n.tr('Privat visning – ikke delt i rommet.', 'Private view – not shared in the Space.')}</p>
      <Show when={sourceUrl()} fallback={<p role="status">{i18n.tr('Venter på en bekreftet nettlesertilkobling.', 'Waiting for a confirmed browser attachment.')}</p>}>
        <Show when={frame()} fallback={<p role="status">{state() === 'disconnected'
          ? i18n.tr('Forbindelsen ble brutt.', 'The connection was lost.')
          : i18n.tr('Venter på nettleserbildet …', 'Waiting for the browser image…')}</p>}>
          {(src) => <img src={src()} alt={i18n.tr('Agentens nettleserside', 'Agent browser page')} style={{ width: '100%', display: 'block' }} />}
        </Show>
      </Show>
    </section>
  )
}
