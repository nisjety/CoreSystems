import { createEffect, createSignal, createUniqueId, onCleanup, Show } from 'solid-js'
import { Portal } from '@solidjs/web'
import {
  acknowledgeConnectionZdr,
  getConnectionZdr,
  isZdrWaiverUiEnabled,
  type ConnectionZdrAcknowledgeResult,
  type ConnectionZdrAssessment,
} from '@/shared/api/connection-zdr-client'
import type {
  ConnectionZdrAcknowledgedVia,
  ConnectionZdrLocale,
  ConnectionZdrVariant,
} from '@/shared/actions/connection-zdr'
import { ApiError } from '@/shared/api/http'
import { translateApiError, useI18n } from '@/shared/i18n'
import { en } from '@/shared/i18n/en'
import { no } from '@/shared/i18n/no'
import { Button } from '@/shared/ui/Button'
import './ConnectionZdrAcknowledgement.css'

// The acknowledgement dialog for a model connection that cannot give zero data retention (ZDR Z-20). Integration
// Core composes the text, stores its hash and owns the ledger. This dialog shows the title and body exactly as served
// and sends back the digest and text version it was shown. It writes no retention wording of its own into the dialog,
// and it keeps nothing in the browser: no acknowledgement and no text is stored. Behind VITE_ZDR_WAIVER_UI, read only
// through isZdrWaiverUiEnabled(); with the switch off it renders nothing and calls nothing.

export type ConnectionZdrAcknowledgementProps = {
  /** The Integration Core connection the dialog is about. */
  connectionId: string
  /**
   * The text to show: `org_zdr_on` when the organisation has ZDR on, `first_use` when a turn was refused for want of
   * an acknowledgement. `org_zdr_off` is a read-only review (nothing to acknowledge), so it offers no continue.
   */
  variant: ConnectionZdrVariant
  /** Where the member acknowledges: while connecting, on first use, or from settings. */
  acknowledgedVia: ConnectionZdrAcknowledgedVia
  /** The member acknowledged; the record Integration Core wrote (or replayed). */
  onAcknowledged?: (result: ConnectionZdrAcknowledgeResult) => void
  /** The dialog is done: cancelled, closed, acknowledged, or there was nothing to acknowledge. */
  onClose: () => void
}

/** The text the member is shown. The digest and version go back with the acknowledgement, unchanged. */
type ShownAssessment = {
  assessment: ConnectionZdrAssessment
  locale: ConnectionZdrLocale
  variant: ConnectionZdrVariant
}

type LoadState = 'loading' | 'ready' | 'failed'

/** An acknowledgement can be recorded only against the text the member was actually shown. */
function acknowledgeableVariant(variant: ConnectionZdrVariant): variant is 'org_zdr_on' | 'first_use' {
  return variant === 'org_zdr_on' || variant === 'first_use'
}

export function ConnectionZdrAcknowledgement(props: ConnectionZdrAcknowledgementProps) {
  const i18n = useI18n()
  const titleId = createUniqueId()
  const copy = () => (i18n.locale() === 'no' ? no : en).zdr.waiver.dialog
  const [state, setState] = createSignal<LoadState>('loading')
  const [shown, setShown] = createSignal<ShownAssessment>()
  const [notice, setNotice] = createSignal<string | null>(null)
  const [saving, setSaving] = createSignal(false)
  let loadAttempt = 0
  let disposed = false

  onCleanup(() => {
    disposed = true
    loadAttempt++
  })

  /**
   * The words for a failed call. A 503 (Integration Core cannot read the text or the organisation's state right now)
   * reads from the error catalogue's service-unavailable copy, which says to try again; any other failure reads by its
   * own code, with the dialog's sentence standing in when the catalogue has none.
   */
  const failureText = (error: unknown, fallback: { no: string; en: string }): string =>
    error instanceof ApiError && error.status === 503
      ? translateApiError({ code: 'upstream_unavailable', status: 503, retryAfterSeconds: error.retryAfterSeconds }, i18n.tr, fallback)
      : translateApiError(error, i18n.tr, fallback)

  /** Reads the assessment and the served text for a locale and variant. `changed` says why it is read again. */
  const load = async (connectionId: string, variant: ConnectionZdrVariant, locale: ConnectionZdrLocale, changed: boolean) => {
    const attempt = ++loadAttempt
    setState('loading')
    setNotice(null)
    try {
      const assessment = await getConnectionZdr(connectionId, locale, variant)
      if (disposed || attempt !== loadAttempt) return
      // Nothing to acknowledge: the dialog shows nothing and is done.
      if (!assessment.requiresAcknowledgement) {
        props.onClose()
        return
      }
      setShown({ assessment, locale, variant })
      setState('ready')
      if (changed) setNotice(copy().changed)
    } catch (error) {
      if (disposed || attempt !== loadAttempt) return
      setState('failed')
      setNotice(failureText(error, { no: no.zdr.waiver.dialog.loadFailed, en: en.zdr.waiver.dialog.loadFailed }))
    }
  }

  // Loads when the dialog opens, and again when the connection, the variant or the app language changes.
  createEffect(
    () => ({ connectionId: props.connectionId, variant: props.variant, locale: i18n.locale() as ConnectionZdrLocale }),
    ({ connectionId, variant, locale }) => {
      if (!isZdrWaiverUiEnabled()) return
      void load(connectionId, variant, locale, false)
    },
  )

  // Escape closes the dialog while it is open.
  createEffect(
    () => isZdrWaiverUiEnabled(),
    (enabled) => {
      if (!enabled) return
      const onKey = (event: KeyboardEvent) => {
        if (event.key === 'Escape' && !saving()) props.onClose()
      }
      document.addEventListener('keydown', onKey)
      return () => document.removeEventListener('keydown', onKey)
    },
  )

  const retry = () => {
    const current = shown()
    void load(props.connectionId, current?.variant ?? props.variant, current?.locale ?? (i18n.locale() as ConnectionZdrLocale), false)
  }

  const acknowledge = async () => {
    const current = shown()
    if (!current || saving() || !acknowledgeableVariant(current.variant)) return
    setSaving(true)
    setNotice(null)
    try {
      // The digest and the text version are the ones read with the text the member sees: nothing else is sent as the
      // text. Integration Core refuses a stale pair, and then the new text is shown.
      const result = await acknowledgeConnectionZdr(props.connectionId, {
        digest: current.assessment.digest,
        textVersion: current.assessment.wordingVersion,
        locale: current.locale,
        variant: current.variant,
        acknowledgedVia: props.acknowledgedVia,
      })
      if (disposed) return
      props.onAcknowledged?.(result)
      props.onClose()
    } catch (error) {
      if (disposed) return
      if (error instanceof ApiError && error.status === 409 && error.code === 'zdr_assessment_changed') {
        await load(props.connectionId, current.variant, current.locale, true)
        return
      }
      setNotice(failureText(error, { no: no.zdr.waiver.dialog.saveFailed, en: en.zdr.waiver.dialog.saveFailed }))
    } finally {
      if (!disposed) setSaving(false)
    }
  }

  return (
    <Show when={isZdrWaiverUiEnabled()}>
      <Portal>
        <div class="verevon-zdr-ack-backdrop">
          <section class="verevon-zdr-ack" role="dialog" aria-modal="true" aria-labelledby={titleId} tabindex={-1}>
            <Show
              when={state() === 'ready' ? shown() : undefined}
              fallback={
                <>
                  <h2 id={titleId} class="verevon-zdr-ack__title">
                    {state() === 'loading' ? copy().loading : copy().loadFailed}
                  </h2>
                  <Show when={state() === 'failed'}>
                    <Show when={notice()}>{(message) => <p class="verevon-zdr-ack__notice" role="alert">{message()}</p>}</Show>
                    <div class="verevon-zdr-ack__actions">
                      <Button size="sm" onClick={() => retry()}>{copy().retry}</Button>
                      <Button size="sm" onClick={() => props.onClose()}>{copy().close}</Button>
                    </div>
                  </Show>
                </>
              }
            >
              {(current) => (
                <Show
                  when={current().assessment.text}
                  fallback={
                    <>
                      <h2 id={titleId} class="verevon-zdr-ack__title">{copy().unavailable}</h2>
                      <div class="verevon-zdr-ack__actions">
                        <Button size="sm" onClick={() => props.onClose()}>{copy().close}</Button>
                      </div>
                    </>
                  }
                >
                  {(text) => (
                    <>
                      {/* The title and the body are the server's text, as served. */}
                      <h2 id={titleId} class="verevon-zdr-ack__title">{text().title}</h2>
                      <p class="verevon-zdr-ack__body">{text().body}</p>
                      <Show when={notice()}>
                        {(message) => <p class="verevon-zdr-ack__notice" role="alert">{message()}</p>}
                      </Show>
                      <div class="verevon-zdr-ack__actions">
                        <Show when={acknowledgeableVariant(current().variant)}>
                          <Button variant="primary" size="sm" disabled={saving()} onClick={() => void acknowledge()}>
                            {saving() ? copy().saving : text().continueLabel}
                          </Button>
                        </Show>
                        <Button size="sm" disabled={saving()} onClick={() => props.onClose()}>
                          {text().cancelLabel}
                        </Button>
                      </div>
                    </>
                  )}
                </Show>
              )}
            </Show>
          </section>
        </div>
      </Portal>
    </Show>
  )
}
