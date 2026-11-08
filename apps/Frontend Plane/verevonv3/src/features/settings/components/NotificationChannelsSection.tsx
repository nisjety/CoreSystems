import { createEffect, createSignal, For, Show } from 'solid-js'
import {
  deviceHasPushSubscription,
  disablePushOnThisDevice,
  enablePushOnThisDevice,
  fetchPushConfig,
  pushSupport,
  type PushConfig,
  type PushSupport,
} from '@/features/core/lib/push-subscription'
import { SectionHeader, SettingsButton, ToggleRow } from '@/features/settings/components/settings-ui'
import { executeAction } from '@/shared/actions/action-client'
import { requestJson } from '@/shared/api/http'
import { useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'

type Channel = 'in_app' | 'push'

type PreferenceRow = { event_type: string; channel: string; enabled: boolean }

type AttentionType = {
  eventType: string
  title: { no: string; en: string }
  description: { no: string; en: string }
}

/**
 * The types a user can route to a channel. Each waiting state always notifies (DEC-38); the scheduled-run outcome
 * pushes only for the outcomes that wait on the owner, which the server decides.
 */
const ATTENTION_TYPES: readonly AttentionType[] = [
  {
    eventType: 'modelplane.needs_approval',
    title: { no: 'Godkjenning kreves', en: 'Approval needed' },
    description: { no: 'En kjøring i en av chatene dine venter på godkjenningen din.', en: 'A run in one of your chats is waiting for your approval.' },
  },
  {
    eventType: 'modelplane.needs_input',
    title: { no: 'Spørsmål til deg', en: 'Question for you' },
    description: { no: 'En kjøring i en av chatene dine har et spørsmål til deg.', en: 'A run in one of your chats has a question for you.' },
  },
  {
    eventType: 'modelplane.plan_ready',
    title: { no: 'Plan klar', en: 'Plan ready' },
    description: { no: 'En plan i en av chatene dine er klar for gjennomgang.', en: 'A plan in one of your chats is ready for your review.' },
  },
  {
    eventType: 'modelplane.scheduled_run_outcome',
    title: { no: 'Planlagt kjøring', en: 'Scheduled run' },
    description: { no: 'En planlagt kjøring i en av chatene dine er ferdig eller venter på deg.', en: 'A scheduled run in one of your chats has finished or is waiting for you.' },
  },
]

// Both channels are on by default. The channel seeds in notification-core migrations 015 and 017 enable in-app and
// push for these types, and a stored row only records a change from that default.
const DEFAULT_ENABLED = true

function preferenceKey(eventType: string, channel: Channel): string {
  return `${eventType}|${channel}`
}

async function loadPreferences(): Promise<Record<string, boolean>> {
  const body = await requestJson<{ preferences?: PreferenceRow[] }>('/api/v1/notifications/preferences')
  const values: Record<string, boolean> = {}
  for (const row of body.preferences ?? []) {
    values[preferenceKey(row.event_type, row.channel as Channel)] = row.enabled
  }
  return values
}

/**
 * Per-channel notification settings (WS16-S6). It replaces the dead push boolean on the account page: each row is a
 * real preference saved through notifications.update_preference, and push is available only when the server offers
 * Web Push and this browser can take a subscription.
 */
export function NotificationChannelsSection() {
  const i18n = useI18n()
  const [values, setValues] = createSignal<Record<string, boolean>>({})
  const [pushConfig, setPushConfig] = createSignal<PushConfig>({ enabled: false, publicKey: null })
  const [deviceOn, setDeviceOn] = createSignal(false)
  const [serverId, setServerId] = createSignal<string | null>(null)
  const [busy, setBusy] = createSignal(false)
  const [message, setMessage] = createSignal<string | null>(null)
  const [loaded, setLoaded] = createSignal(false)
  const support: PushSupport = pushSupport()

  const pushAvailable = () => support === 'supported' && pushConfig().enabled && pushConfig().publicKey !== null

  const valueOf = (eventType: string, channel: Channel) => values()[preferenceKey(eventType, channel)] ?? DEFAULT_ENABLED

  const actor = () => ({
    type: 'human' as const,
    userId: getSession().user?.id ?? '',
    orgId: getSession().activeOrg?.id ?? '',
  })

  // Loads once when the section mounts: the compute half reads nothing, so the effect never reruns.
  createEffect(
    () => undefined,
    () => {
      void (async () => {
        try {
          setValues(await loadPreferences())
        } catch {
          setMessage(i18n.tr('Kunne ikke laste varselinnstillingene.', 'Could not load the notification settings.'))
        }
        const [config, subscribed] = await Promise.all([
          fetchPushConfig().catch(() => ({ enabled: false, publicKey: null })),
          deviceHasPushSubscription().catch(() => false),
        ])
        setPushConfig(config)
        setDeviceOn(subscribed)
        setLoaded(true)
      })()
    },
  )

  const setChannel = async (eventType: string, channel: Channel, enabled: boolean) => {
    const key = preferenceKey(eventType, channel)
    setMessage(null)
    setValues((current) => ({ ...current, [key]: enabled }))
    try {
      await executeAction('notifications.update_preference', actor(), { eventType, channel, enabled })
    } catch {
      // A switch only flips, so a refused change returns to the opposite of the value it asked for.
      setValues((current) => ({ ...current, [key]: !enabled }))
      setMessage(i18n.tr('Kunne ikke lagre varselinnstillingen.', 'Could not save the notification setting.'))
    }
  }

  const turnOnHere = async () => {
    const publicKey = pushConfig().publicKey
    if (!publicKey || busy()) return
    setBusy(true)
    setMessage(null)
    try {
      setServerId(await enablePushOnThisDevice(publicKey))
      setDeviceOn(true)
    } catch {
      setMessage(i18n.tr(
        'Push ble ikke slått på for denne enheten. Sjekk varseltillatelsen i nettleseren.',
        'Push was not turned on for this device. Check the notification permission in your browser.',
      ))
    } finally {
      setBusy(false)
    }
  }

  const turnOffHere = async () => {
    if (busy()) return
    setBusy(true)
    setMessage(null)
    try {
      await disablePushOnThisDevice(serverId())
      setServerId(null)
      setDeviceOn(false)
    } catch {
      setMessage(i18n.tr('Kunne ikke slå av push for denne enheten.', 'Could not turn off push for this device.'))
    } finally {
      setBusy(false)
    }
  }

  const deviceStatus = () => {
    if (support === 'insecure') {
      return i18n.tr('Push krever en sikker tilkobling (HTTPS).', 'Push requires a secure connection (HTTPS).')
    }
    if (support === 'unsupported') {
      return i18n.tr('Denne nettleseren støtter ikke push-varsler.', 'This browser does not support push notifications.')
    }
    if (loaded() && !pushConfig().enabled) {
      return i18n.tr('Push er ikke slått på på serveren ennå.', 'Push is not turned on for the server yet.')
    }
    return deviceOn()
      ? i18n.tr('Slått på for denne enheten.', 'On for this device.')
      : i18n.tr('Slått av for denne enheten.', 'Off for this device.')
  }

  const pushDescription = () =>
    pushAvailable()
      ? i18n.tr('Push til nettleseren på enheter der du har slått det på.', 'Push to the browser on devices where you turned it on.')
      : i18n.tr('Krever at push er slått på for serveren og denne enheten.', 'Requires push to be turned on for the server and this device.')

  return (
    <section id="notifications" class="verevon-settings-section">
      <SectionHeader
        title={i18n.tr('Varsler', 'Notifications')}
        description={i18n.tr(
          'Velg hvordan Verevon varsler deg når en chat venter på deg.',
          'Choose how Verevon notifies you when a chat is waiting for you.',
        )}
      />
      <div class="verevon-settings-list-card">
        <div class="verevon-settings-list-row">
          <div>
            <p>{i18n.tr('Push på denne enheten', 'Push on this device')}</p>
            <span>{deviceStatus()}</span>
          </div>
          <Show
            when={pushAvailable()}
            fallback={<SettingsButton settingsSize="sm" disabled>{i18n.tr('Ikke tilgjengelig', 'Unavailable')}</SettingsButton>}
          >
            <SettingsButton
              settingsSize="sm"
              disabled={busy()}
              onClick={() => void (deviceOn() ? turnOffHere() : turnOnHere())}
            >
              {deviceOn() ? i18n.tr('Slå av her', 'Turn off here') : i18n.tr('Slå på her', 'Turn on here')}
            </SettingsButton>
          </Show>
        </div>
      </div>
      <div class="verevon-settings-divided-list">
        <For each={ATTENTION_TYPES}>
          {(type) => (
            <>
              <ToggleRow
                title={`${i18n.tr(type.title.no, type.title.en)} · ${i18n.tr('I Verevon', 'In Verevon')}`}
                description={i18n.tr(type.description.no, type.description.en)}
                enabled={valueOf(type.eventType, 'in_app')}
                onChange={(checked) => void setChannel(type.eventType, 'in_app', checked)}
              />
              <ToggleRow
                title={`${i18n.tr(type.title.no, type.title.en)} · Push`}
                description={pushDescription()}
                enabled={valueOf(type.eventType, 'push')}
                disabled={!pushAvailable()}
                onChange={(checked) => void setChannel(type.eventType, 'push', checked)}
              />
            </>
          )}
        </For>
      </div>
      <Show when={message()}>
        {(text) => (
          <p class="verevon-settings-status-message" role="status">
            {text()}
          </p>
        )}
      </Show>
    </section>
  )
}
