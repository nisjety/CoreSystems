import { createEffect, createMemo, createSignal, Show } from 'solid-js'
import type { BrowserAttachSharedJarEvent } from '@/shared/api/run-console-client'
import { useI18n } from '@/shared/i18n'
import './assistant-live-browser.css'

/** Owner-private, receipt-pinned live view. Gateway and Quarry authorize the frame stream. */
export function AssistantLiveBrowser(props: {
  sessionId: string
  modelRunId: string
  receipt: BrowserAttachSharedJarEvent | null
  action?: string | null
  /** After a confirmed terminal, the last frame is a labelled snapshot. */
  finished?: boolean
  /** The main Nett workspace gives the browser more room than the assistant dock. */
  surface?: 'workspace' | 'dock'
  onStop?: () => void
  stopLabel?: string
  stopping?: boolean
}) {
  const i18n = useI18n()
  const [frame, setFrame] = createSignal<string | null>(null)
  const [cursor, setCursor] = createSignal<{ left: number; top: number } | null>(null)
  const [cursorLastSeen, setCursorLastSeen] = createSignal(false)
  const [cursorMenu, setCursorMenu] = createSignal<{ left: number; top: number } | null>(null)
  const [state, setState] = createSignal<'waiting' | 'live' | 'disconnected'>('waiting')
  // Annotation mode (F-t2 slice 4, behind VITE_BROWSER_ANNOTATE=true): the draft
  // lives only in this component's state, is never persisted, and is copied to
  // the clipboard for pasting into the composer. No page content is stored.
  const annotateEnabled = () => import.meta.env.VITE_BROWSER_ANNOTATE === 'true'
  const [annotating, setAnnotating] = createSignal(false)
  const [annotationPoint, setAnnotationPoint] = createSignal<{ left: number; top: number } | null>(null)
  const [annotationText, setAnnotationText] = createSignal('')
  const [annotationNote, setAnnotationNote] = createSignal<string | null>(null)
  const pickAnnotationPoint = (event: { clientX: number; clientY: number; currentTarget: { getBoundingClientRect(): { left: number; top: number; width: number; height: number } } }) => {
    const rect = event.currentTarget.getBoundingClientRect()
    if (!rect.width || !rect.height) return
    setAnnotationPoint({
      left: Math.min(100, Math.max(0, (event.clientX - rect.left) / rect.width * 100)),
      top: Math.min(100, Math.max(0, (event.clientY - rect.top) / rect.height * 100)),
    })
    setAnnotationNote(null)
  }
  const copyAnnotation = async () => {
    const point = annotationPoint()
    const text = annotationText().trim()
    if (!point || !text) return
    const payload = `@${point.left.toFixed(1)}%,${point.top.toFixed(1)}%: ${text}`
    try {
      await navigator.clipboard.writeText(payload)
      setAnnotationNote(i18n.tr('Kopiert. Lim inn i skrivefeltet.', 'Copied. Paste into the composer.'))
    } catch {
      setAnnotationNote(null)
    }
  }
  const validReceipt = createMemo(() => {
    const r = props.receipt
    return r && r.runId === props.modelRunId && r.sessionRunId === props.sessionId && r.tabId && r.attachId ? r : null
  })

  createEffect(validReceipt, (receipt) => {
    setFrame(null); setCursor(null); setCursorLastSeen(false); setCursorMenu(null); setState('waiting')
    if (!receipt) return
    const base = `/api/v1/browser/sessions/${encodeURIComponent(receipt.sessionRunId)}`
    const frames = new EventSource(`${base}/frames/stream?${new URLSearchParams({ tabId: receipt.tabId })}`)
    let expiry: ReturnType<typeof setTimeout> | undefined
    let snapshotExpiry: ReturnType<typeof setTimeout> | undefined
    // The stream can close before the terminal event arrives. Keep only the
    // last real frame briefly so that the later terminal state can label it.
    const disconnected = () => {
      if (expiry !== undefined) clearTimeout(expiry)
      expiry = undefined
      setCursorMenu(null)
      setState('disconnected')
    }
    frames.addEventListener('frame', event => {
      try {
        const payload: unknown = JSON.parse((event as MessageEvent<string>).data)
        if (!payload || typeof payload !== 'object') return
        const data = payload as Record<string, unknown>
        if (typeof data.dataBase64 !== 'string' || !data.dataBase64.length || data.dataBase64.length > 8_000_000 || !/^[A-Za-z0-9+/]*={0,2}$/.test(data.dataBase64)) return
        if (data.mimeType !== 'image/jpeg' && data.mimeType !== 'image/png') return
        // Only accept a position Quarry supplied with an actual frame and
        // bound to this attachment. Subsequent frames retain that position as
        // explicitly last-seen, rather than suggesting a new pointer move.
        const marker = data.agentCursor
        let position: { left: number; top: number } | null = null
        if (marker && typeof marker === 'object' && !Array.isArray(marker)) {
          const c = marker as Record<string, unknown>
          if ((c.modelRunId === undefined || c.modelRunId === receipt.runId) && c.sessionRunId === receipt.sessionRunId && c.attachId === receipt.attachId && c.tabId === receipt.tabId &&
            typeof c.x === 'number' && Number.isFinite(c.x) && typeof c.y === 'number' && Number.isFinite(c.y) &&
            typeof c.viewportWidth === 'number' && Number.isFinite(c.viewportWidth) && c.viewportWidth > 0 &&
            typeof c.viewportHeight === 'number' && Number.isFinite(c.viewportHeight) && c.viewportHeight > 0 &&
            c.x >= 0 && c.x < c.viewportWidth && c.y >= 0 && c.y < c.viewportHeight) {
            position = { left: c.x / c.viewportWidth * 100, top: c.y / c.viewportHeight * 100 }
          }
        }
        if (position) {
          setCursor(position)
          setCursorLastSeen(false)
        } else if (marker == null) {
          if (cursor()) setCursorLastSeen(true)
        } else {
          // Reject malformed or mismatched telemetry, including its previous
          // position, rather than attributing it to this run.
          setCursor(null); setCursorLastSeen(false); setCursorMenu(null)
        }
        setFrame(`data:${data.mimeType};base64,${data.dataBase64}`); setState('live')
        if (snapshotExpiry !== undefined) clearTimeout(snapshotExpiry)
        snapshotExpiry = setTimeout(() => {
          snapshotExpiry = undefined
          setFrame(null)
          setCursor(null)
          setCursorLastSeen(false)
          setCursorMenu(null)
        }, 120_000)
        if (expiry !== undefined) clearTimeout(expiry)
        expiry = setTimeout(disconnected, 5000)
      } catch { /* Ignore malformed frames. */ }
    })
    frames.addEventListener('error', disconnected)
    frames.addEventListener('done', () => { disconnected(); frames.close() })
    return () => { frames.close(); if (expiry !== undefined) clearTimeout(expiry); if (snapshotExpiry !== undefined) clearTimeout(snapshotExpiry); setFrame(null); setCursor(null); setCursorLastSeen(false); setCursorMenu(null) }
  })

  return <section class={['assistant-live-browser', { 'assistant-live-browser--workspace': props.surface === 'workspace' }]} aria-label={i18n.tr('Agentens nettleser', 'Agent browser')}>
    <div class="assistant-live-browser__title">
      <div class="assistant-live-browser__tab"><span class="assistant-live-browser__tab-dot" aria-hidden="true" /><strong>{i18n.tr('Verevons nettleser', 'Verevon’s browser')}</strong></div>
      <span class="assistant-live-browser__privacy">{props.finished ? i18n.tr('Siste visning · opptil 2 minutter', 'Last view · up to 2 minutes') : i18n.tr('Privat visning', 'Private view')}</span>
    </div>
    <div class="assistant-live-browser__toolbar">
      <span class="assistant-live-browser__connection" role="status">
        <span class="assistant-live-browser__connection-dot" data-state={props.finished ? 'finished' : state()} aria-hidden="true" />
        {props.finished ? i18n.tr('Avsluttet', 'Finished') : state() === 'live' ? i18n.tr('Direkte', 'Live') : state() === 'disconnected' ? i18n.tr('Frakoblet', 'Disconnected') : i18n.tr('Kobler til …', 'Connecting…')}
      </span>
      <Show when={props.action}><span class="assistant-live-browser__action" role="status">{props.action}</span></Show>
      <Show when={!props.finished && props.onStop}>
        <button type="button" class="assistant-live-browser__stop" disabled={props.stopping} onClick={() => props.onStop?.()}>{props.stopping ? i18n.tr('Stopper …', 'Stopping…') : props.stopLabel ?? i18n.tr('Stopp', 'Stop')}</button>
      </Show>
      <Show when={annotateEnabled() && state() === 'live' && !props.finished}>
        <button type="button" class="assistant-live-browser__annotate" aria-pressed={annotating() ? 'true' : 'false'} onClick={() => setAnnotating((value) => !value)}>{i18n.tr('Kommenter', 'Annotate')}</button>
      </Show>
    </div>
    <Show when={validReceipt()} fallback={<p role="status">{i18n.tr('Venter på bekreftet nettlesertilkobling.', 'Waiting for a confirmed browser attachment.')}</p>}>
      <Show when={frame()} fallback={<p role="status">{state() === 'disconnected' ? i18n.tr('Nettleserforbindelsen ble brutt.', 'Browser connection lost.') : i18n.tr('Venter på nettleserbildet …', 'Waiting for the browser image…')}</p>}>
        {src => <div class="assistant-live-browser__frame" onClick={(event) => { if (annotateEnabled() && annotating()) pickAnnotationPoint(event) }}><img src={src()} alt={i18n.tr('Agentens nettleserside', 'Agent browser page')} />
          <Show when={!props.finished && state() === 'live' && cursor()}>
            {position => <button type="button" class="assistant-live-browser__cursor" aria-label={i18n.tr('Verevons markør', 'Verevon cursor')} aria-expanded={cursorMenu() ? 'true' : 'false'} disabled={!props.onStop} onClick={() => setCursorMenu(cursorMenu() ? null : position())} style={{ left: `${position().left}%`, top: `${position().top}%` }}>
              <svg viewBox="0 0 24 26" aria-hidden="true"><path d="M2 2l18 12-8 1-4 8z" /></svg><span>{cursorLastSeen() ? i18n.tr('Verevon · sist sett', 'Verevon · last seen') : 'Verevon'}</span>
            </button>}
          </Show>
          <Show when={props.finished && cursor()}>
            {position => <span class="assistant-live-browser__cursor assistant-live-browser__cursor--snapshot" role="img" aria-label={i18n.tr('Verevons siste markørposisjon', 'Verevon’s last cursor position')} style={{ left: `${position().left}%`, top: `${position().top}%` }}>
              <svg viewBox="0 0 24 26" aria-hidden="true"><path d="M2 2l18 12-8 1-4 8z" /></svg><span>{i18n.tr('Verevon · siste posisjon', 'Verevon · last position')}</span>
            </span>}
          </Show>
          <Show when={!props.finished && state() === 'live' && cursorMenu()}>{position => <div class="assistant-live-browser__cursor-menu" role="group" aria-label={i18n.tr('Agentkontroller', 'Agent controls')} style={{ left: `min(${position().left}%, calc(100% - 132px))`, top: `min(${position().top}%, calc(100% - 72px))` }}>
            <button type="button" disabled={props.stopping} onClick={() => { props.onStop?.(); setCursorMenu(null) }}>{props.stopLabel ?? i18n.tr('Stopp agenten', 'Stop agent')}</button>
            <button type="button" onClick={() => setCursorMenu(null)}>{i18n.tr('Lukk', 'Close')}</button>
          </div>}</Show>
          <Show when={annotateEnabled() && annotating()}>
            <div class="assistant-live-browser__annotate-panel" onClick={(event) => event.stopPropagation()}>
              <p>{annotationPoint() ? i18n.tr('Klikk på bildet for å velge punktet.', 'Click the image to pick the point.') : i18n.tr('Klikk på bildet for å velge punktet.', 'Click the image to pick the point.')}</p>
              <textarea
                aria-label={i18n.tr('Kommentar til bildepunkt', 'Comment for image point')}
                value={annotationText()}
                onInput={(event) => setAnnotationText(event.currentTarget.value)}
                maxlength={500}
                rows={2}
              />
              <div>
                <button type="button" disabled={!annotationPoint() || !annotationText().trim()} onClick={() => void copyAnnotation()}>{i18n.tr('Kopier', 'Copy')}</button>
                <button type="button" onClick={() => { setAnnotating(false); setAnnotationPoint(null); setAnnotationText(''); setAnnotationNote(null) }}>{i18n.tr('Avbryt', 'Cancel')}</button>
              </div>
              <Show when={annotationNote()}><p role="status">{annotationNote()}</p></Show>
            </div>
          </Show>
          <Show when={annotateEnabled() && annotating() ? annotationPoint() : null}>
            {point => <span class="assistant-live-browser__annotate-mark" style={{ left: `${point().left}%`, top: `${point().top}%` }} aria-hidden="true" />}
          </Show>
        </div>}
      </Show>
    </Show>
  </section>
}
