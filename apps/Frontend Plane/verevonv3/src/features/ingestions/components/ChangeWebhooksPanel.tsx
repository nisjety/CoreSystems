import { createSignal, For, Show } from 'solid-js'
import { ApiError } from '@/shared/api/http'
import { getOrganizationZdr } from '@/shared/api/organization-client'
import {
  createWebhook,
  deleteWebhook,
  listWebhookDeliveries,
  listWebhooks,
  retryWebhookDelivery,
  rotateWebhookSecret,
  updateWebhook,
  type WebhookDelivery,
  type WebhookSubscription,
} from '@/shared/api/webhooks-client'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { Eye, KeyRound, Loader, Pause, Play, Plus, RefreshCw, Send, Trash2, Webhook } from '@/shared/icons'
import { webhookErrorMessage } from '@/features/ingestions/lib/errors'
import { CopyButton, EmptyState, relativeTime, type Tr } from '@/features/ingestions/components/ingestions-ui'

/** The secret of a create or a rotation. Held in memory only, until the person dismisses it. */
interface ShownSecret {
  url: string
  secret: string
  created: boolean
}

/**
 * Webhook subscriptions for change alerts. When a monitored page changes, Quarry POSTs a
 * signed body to the address here. The signing secret is shown once, kept only in this
 * component's memory and never written to storage. A Zero Data Retention organization cannot
 * subscribe, so the form is hidden for it; the list, pause, rotate and delete stay open.
 */
export function ChangeWebhooksPanel(props: { orgId: string }) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [url, setUrl] = createSignal('')
  const [description, setDescription] = createSignal('')
  const [saving, setSaving] = createSignal(false)
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [retryingId, setRetryingId] = createSignal<string | null>(null)
  const [confirming, setConfirming] = createSignal<string | null>(null)
  const [openId, setOpenId] = createSignal<string | null>(null)
  const [zdrRefused, setZdrRefused] = createSignal(false)
  const [shown, setShown] = createSignal<ShownSecret | null>(null)
  const [error, setError] = createSignal<string | null>(null)

  const [subscriptions, { refetch: refetchSubscriptions }] = createResource(
    () => 'webhook-subscriptions',
    () => listWebhooks(),
  )
  const [zdr] = createResource(
    () => props.orgId.trim() || false,
    (id) => getOrganizationZdr(id).catch(() => false),
  )
  const [deliveries, { refetch: refetchDeliveries }] = createResource(openId, (id) => listWebhookDeliveries(id))

  const canCreate = () => zdr() !== true && !zdrRefused()

  function failed(caught: unknown) {
    setError(webhookErrorMessage(caught, tr, { no: 'Webhooken kunne ikke oppdateres.', en: 'The webhook could not be updated.' }))
  }

  /** A destructive step takes two clicks: the first asks, the second runs `action` on `arg`. */
  function askOnce<T>(key: string, action: (arg: T) => void, arg: T) {
    if (confirming() === key) {
      setConfirming(null)
      void action(arg)
      return
    }
    setConfirming(key)
  }

  async function create() {
    const address = url().trim()
    if (!address) {
      setError(tr('Angi adressen som skal få varsler.', 'Enter the address that should receive alerts.'))
      return
    }
    setError(null)
    setSaving(true)
    try {
      const created = await createWebhook({ url: address, description: description().trim() || undefined })
      setShown({ url: created.url, secret: created.secret, created: true })
      setUrl('')
      setDescription('')
      await refetchSubscriptions()
    } catch (caught) {
      if (caught instanceof ApiError && caught.details?.reason === 'webhook_off_process') setZdrRefused(true)
      failed(caught)
    } finally {
      setSaving(false)
    }
  }

  async function setActive(sub: WebhookSubscription, active: boolean) {
    setError(null)
    setBusyId(sub.id)
    try {
      await updateWebhook(sub.id, { active })
      await refetchSubscriptions()
    } catch (caught) {
      failed(caught)
    } finally {
      setBusyId(null)
    }
  }

  async function rotate(sub: WebhookSubscription) {
    setError(null)
    setBusyId(sub.id)
    try {
      const next = await rotateWebhookSecret(sub.id)
      setShown({ url: sub.url, secret: next.secret, created: false })
      await refetchSubscriptions()
    } catch (caught) {
      failed(caught)
    } finally {
      setBusyId(null)
    }
  }

  async function remove(sub: WebhookSubscription) {
    setError(null)
    setBusyId(sub.id)
    try {
      await deleteWebhook(sub.id)
      if (openId() === sub.id) setOpenId(null)
      if (shown()?.url === sub.url) setShown(null)
      await refetchSubscriptions()
    } catch (caught) {
      failed(caught)
    } finally {
      setBusyId(null)
    }
  }

  async function retry(row: WebhookDelivery) {
    const webhookId = openId()
    if (!webhookId) return
    setError(null)
    setRetryingId(row.id)
    try {
      await retryWebhookDelivery(webhookId, row.id)
      await refetchDeliveries()
    } catch (caught) {
      failed(caught)
    } finally {
      setRetryingId(null)
    }
  }

  return (
    <section class="verevon-panel ingestions-card ingestions-webhooks">
      <div class="ingestions-card__header">
        <div>
          <h2>{tr('Webhooks', 'Webhooks')}</h2>
          <p>
            {tr(
              'Quarry sender en signert POST til adressen din når en overvåket side endrer seg.',
              'Quarry sends a signed POST to your address when a monitored page changes.',
            )}
          </p>
        </div>
        <Button size="xs" variant="ghost" onClick={() => void refetchSubscriptions()}>
          <RefreshCw class="size-4" strokeWidth={1.9} />
          {tr('Oppdater', 'Refresh')}
        </Button>
      </div>

      <Show when={!canCreate()}>
        <p class="ingestions-muted">
          {tr(
            'Webhooks er ikke tilgjengelig under dataskjerming (ZDR). Eksisterende kan du fortsatt se, pause, rotere og slette.',
            'Webhooks are not available under zero data retention (ZDR). You can still view, pause, rotate and delete existing ones.',
          )}
        </p>
      </Show>

      <Show when={canCreate()}>
        <form
          class="ingestions-webhook-form"
          onSubmit={(event) => {
            event.preventDefault()
            void create()
          }}
        >
          <label class="ingestions-field">
            {tr('Adresse som skal få varsler (https)', 'Address that receives alerts (https)')}
            <VerevonInput
              value={url()}
              onInput={(event) => setUrl(event.currentTarget.value)}
              placeholder="https://hooks.example.com/verevon"
            />
          </label>
          <label class="ingestions-field">
            {tr('Beskrivelse (valgfritt)', 'Description (optional)')}
            <VerevonInput
              value={description()}
              onInput={(event) => setDescription(event.currentTarget.value)}
              placeholder={tr('For eksempel prodsystemet', 'For example the production system')}
            />
          </label>
          <small class="ingestions-field__help">
            {tr(
              'Hendelse: change_detected. Adressen må være offentlig og bruke https.',
              'Event: change_detected. The address must be public and use https.',
            )}
          </small>
          <Button variant="secondary" fullWidth type="submit" disabled={saving()}>
            <Show when={saving()} fallback={<Plus class="size-4" strokeWidth={1.9} />}>
              <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
            </Show>
            {tr('Opprett webhook', 'Create webhook')}
          </Button>
        </form>
      </Show>

      <Show when={error()}>
        {(message) => (
          <div class="ingestions-alert" role="alert">
            {message()}
          </div>
        )}
      </Show>

      <Show when={shown()}>
        {(secret) => (
          <div class="ingestions-secret-once" role="status">
            <strong>
              {secret().created ? tr('Hemmelighet for signering', 'Signing secret') : tr('Ny hemmelighet for signering', 'New signing secret')}
            </strong>
            <p class="ingestions-muted">
              {tr(
                'Verevon viser den bare én gang og kan ikke hente den igjen. Lagre den nå. Hvis du mister den, roter hemmeligheten.',
                'Verevon shows it once and cannot fetch it again. Save it now. If you lose it, rotate the secret.',
              )}
              {secret().created ? '' : ` ${tr('Den gamle hemmeligheten slutter å virke med en gang.', 'The old secret stops verifying at once.')}`}
            </p>
            <code class="ingestions-mono ingestions-secret-once__value">{secret().secret}</code>
            <div class="ingestions-actions-row">
              <CopyButton text={() => secret().secret} label={tr('Kopier hemmelighet', 'Copy secret')} />
              <Button size="xs" variant="ghost" onClick={() => setShown(null)}>
                {tr('Jeg har lagret den', "I've saved it")}
              </Button>
            </div>
          </div>
        )}
      </Show>

      <Show
        when={!subscriptions.error}
        fallback={
          <div class="ingestions-alert" role="alert">
            {webhookErrorMessage(subscriptions.error, tr, { no: 'Webhooks kunne ikke lastes.', en: 'Webhooks could not be loaded.' })}
          </div>
        }
      >
        <Show when={subscriptions()} fallback={<div class="ingestions-empty-box">{tr('Laster webhooks …', 'Loading webhooks…')}</div>}>
          {(items) => (
            <div class="ingestions-card-list">
              <For
                each={items()}
                fallback={
                  <EmptyState icon={<Webhook class="size-5" strokeWidth={1.8} />} title={tr('Ingen webhooks ennå.', 'No webhooks yet.')}>
                    {tr(
                      'Legg inn en https-adresse over. Den får en signert POST når en overvåket side endrer seg.',
                      'Add an https address above. It gets a signed POST when a monitored page changes.',
                    )}
                  </EmptyState>
                }
              >
                {(sub) => (
                  <article class={cn('ingestions-list-card', !sub.active && 'ingestions-list-card--paused')}>
                    <div class="ingestions-title-row ingestions-title-row--spread">
                      <div class="ingestions-min-width">
                        <h3 class="ingestions-truncate">{sub.url}</h3>
                        <p>
                          {sub.description ? `${sub.description} · ` : ''}
                          <span class="ingestions-mono">{sub.events.join(', ')}</span>
                          {' · '}
                          <span class="ingestions-mono">{sub.secretHint}</span>
                        </p>
                        <small class="ingestions-muted">
                          {sub.lastDelivery
                            ? `${tr('Sist', 'Last')}: ${deliveryStatusLabel(sub.lastDelivery.status, tr)} · ${relativeTime(sub.lastDelivery.at, i18n)}`
                            : tr('Ingen leveringer ennå.', 'No deliveries yet.')}
                        </small>
                      </div>
                      <span class={cn('ingestions-status', sub.active ? 'ingestions-status--success' : 'ingestions-status--neutral')}>
                        {sub.active ? tr('aktiv', 'active') : tr('pauset', 'paused')}
                      </span>
                    </div>
                    <div class="ingestions-actions-row">
                      <Button size="xs" variant="ghost" disabled={busyId() === sub.id} onClick={() => void setActive(sub, !sub.active)}>
                        <Show when={sub.active} fallback={<Play class="size-4" strokeWidth={1.9} />}>
                          <Pause class="size-4" strokeWidth={1.9} />
                        </Show>
                        {sub.active ? tr('Pause', 'Pause') : tr('Aktiver', 'Resume')}
                      </Button>
                      <Button size="xs" variant="ghost" onClick={() => setOpenId(openId() === sub.id ? null : sub.id)}>
                        <Eye class="size-4" strokeWidth={1.9} />
                        {openId() === sub.id ? tr('Skjul leveringer', 'Hide deliveries') : tr('Leveringer', 'Deliveries')}
                      </Button>
                      <Button
                        size="xs"
                        variant="ghost"
                        disabled={busyId() === sub.id}
                        onClick={() => askOnce(`rotate:${sub.id}`, rotate, sub)}
                      >
                        <KeyRound class="size-4" strokeWidth={1.9} />
                        {confirming() === `rotate:${sub.id}` ? tr('Bekreft ny hemmelighet', 'Confirm new secret') : tr('Roter hemmelighet', 'Rotate secret')}
                      </Button>
                      <Button
                        size="xs"
                        variant="ghost"
                        disabled={busyId() === sub.id}
                        onClick={() => askOnce(`delete:${sub.id}`, remove, sub)}
                      >
                        <Trash2 class="size-4" strokeWidth={1.9} />
                        {confirming() === `delete:${sub.id}` ? tr('Bekreft sletting', 'Confirm delete') : tr('Slett', 'Delete')}
                      </Button>
                    </div>

                    <Show when={openId() === sub.id}>
                      <div class="ingestions-webhook-deliveries">
                        <Show
                          when={!deliveries.error}
                          fallback={<p class="ingestions-muted">{tr('Leveringene kunne ikke hentes.', 'The deliveries could not be loaded.')}</p>}
                        >
                          <Show
                            when={!deliveries.loading && deliveries()}
                            fallback={<p class="ingestions-muted">{tr('Laster leveringer …', 'Loading deliveries…')}</p>}
                          >
                            {(rows) => (
                              <ol class="ingestions-webhook-deliveries__list">
                                <For each={rows()} fallback={<li class="ingestions-muted">{tr('Ingen leveringer ennå.', 'No deliveries yet.')}</li>}>
                                  {(row) => (
                                    <li class="ingestions-webhook-delivery">
                                      <span class={cn('ingestions-status', deliveryTone(row.status))}>
                                        {deliveryStatusLabel(row.status, tr)}
                                      </span>
                                      <div class="ingestions-min-width">
                                        <span class="ingestions-mono">{row.eventType}</span>
                                        <small class="ingestions-muted">
                                          {tr('forsøk', 'attempt')} {row.attempt} · {relativeTime(row.createdAt, i18n)}
                                          {row.lastError ? ` · ${row.lastError}` : ''}
                                        </small>
                                      </div>
                                      <Show when={row.status === 'failed' || row.status === 'dlq'}>
                                        <Button size="xs" variant="secondary" disabled={retryingId() === row.id} onClick={() => void retry(row)}>
                                          <Send class="size-4" strokeWidth={1.9} />
                                          {tr('Send på nytt', 'Send again')}
                                        </Button>
                                      </Show>
                                    </li>
                                  )}
                                </For>
                              </ol>
                            )}
                          </Show>
                        </Show>
                      </div>
                    </Show>
                  </article>
                )}
              </For>
            </div>
          )}
        </Show>
      </Show>

      <details class="ingestions-webhook-docs">
        <summary>{tr('Slik verifiserer du signaturen', 'How to verify the signature')}</summary>
        <p>
          {tr(
            'Hver POST har Content-Type application/json og headeren X-Quarry-Signature: t=<unix-tid>,v1=<hex>.',
            'Each POST has Content-Type application/json and the header X-Quarry-Signature: t=<unix time>,v1=<hex>.',
          )}
        </p>
        <ol>
          <li>{tr('Del headeren på komma og les t og v1.', 'Split the header on commas and read t and v1.')}</li>
          <li>
            {tr(
              'Regn ut HMAC-SHA256 med hele whsec_-strengen som nøkkel, over «t» + «.» + kroppen slik den kom. Ikke bruk en ny JSON-serialisering.',
              'Compute HMAC-SHA256 with the whole whsec_ string as the key, over "t" + "." + the body exactly as it arrived. Do not re-serialise the JSON.',
            )}
          </li>
          <li>
            {tr(
              'Sammenlign med v1 i konstant tid, og avvis t som er mer enn 5 minutter fra klokken din.',
              'Compare with v1 in constant time, and reject a t more than 5 minutes from your clock.',
            )}
          </li>
        </ol>
        <p>
          {tr(
            'Hendelser kan komme mer enn én gang og i annen rekkefølge. Bruk event_id til å hindre dobbeltbehandling.',
            'Events can arrive more than once and out of order. Use event_id to avoid processing one twice.',
          )}
        </p>
      </details>
    </section>
  )
}

function deliveryStatusLabel(status: string, tr: Tr): string {
  switch (status) {
    case 'success':
    case 'delivered':
      return tr('levert', 'delivered')
    case 'pending':
      return tr('venter', 'pending')
    case 'in_flight':
      return tr('sendes', 'sending')
    case 'failed':
      return tr('feilet', 'failed')
    case 'dlq':
      return tr('gitt opp', 'given up')
    default:
      return status.replace(/_/g, ' ')
  }
}

function deliveryTone(status: string): string {
  if (status === 'success' || status === 'delivered') return 'ingestions-status--success'
  if (status === 'failed' || status === 'dlq') return 'ingestions-status--warning'
  return 'ingestions-status--info'
}
