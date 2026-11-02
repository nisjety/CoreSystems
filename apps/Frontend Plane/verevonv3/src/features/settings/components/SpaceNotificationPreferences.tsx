import { createSignal, For, onSettled, Show } from 'solid-js'
import { SectionHeader, ToggleRow } from '@/features/settings/components/settings-ui'
import { getNotificationPreferences, updateNotificationPreference } from '@/shared/api/notifications-client'
import { useI18n } from '@/shared/i18n'

/**
 * Email preferences for the Space attention signals (WS11-S9). The in-app copy of
 * these signals is the Space inbox, so only the email channel is listed here. The
 * defaults mirror notification-core migration 015 (mention, invite, approval on;
 * the rest off); an explicit saved row overrides them. Email goes out only once
 * the operator has turned delivery on, so this is a preference, not a promise.
 */
const SPACE_EMAIL_SIGNALS = [
  { type: 'space.mention', defaultOn: true },
  { type: 'space.reply', defaultOn: false },
  { type: 'space.invite', defaultOn: true },
  { type: 'space.agent_done', defaultOn: false },
  { type: 'space.agent_failed', defaultOn: false },
  { type: 'space.approval_needed', defaultOn: true },
  { type: 'space.agent_awaiting', defaultOn: false },
  { type: 'space.reminder', defaultOn: false },
] as const

type SpaceSignalType = (typeof SPACE_EMAIL_SIGNALS)[number]['type']

/** The list route may answer a bare array or a wrapper, in snake or camel case. */
function explicitEmailPreferences(raw: unknown): Map<string, boolean> {
  const result = new Map<string, boolean>()
  const rows: unknown[] = Array.isArray(raw)
    ? raw
    : Array.isArray((raw as { preferences?: unknown })?.preferences)
      ? ((raw as { preferences: unknown[] }).preferences)
      : Array.isArray((raw as { data?: unknown })?.data)
        ? ((raw as { data: unknown[] }).data)
        : []
  for (const row of rows) {
    const record = row as Record<string, unknown>
    const type = (record.event_type ?? record.eventType) as unknown
    if (typeof type === 'string' && record.channel === 'email' && typeof record.enabled === 'boolean') {
      result.set(type, record.enabled)
    }
  }
  return result
}

export function SpaceNotificationPreferences() {
  const i18n = useI18n()
  const [explicit, setExplicit] = createSignal<Map<string, boolean>>(new Map())
  const [error, setError] = createSignal<string | null>(null)

  onSettled(() => {
    getNotificationPreferences()
      .then((rows) => setExplicit(explicitEmailPreferences(rows)))
      .catch(() => {
        // Defaults stay shown; saving still works.
      })
  })

  const labels = (): Record<SpaceSignalType, { title: string; description: string }> => ({
    'space.mention': {
      title: i18n.tr('Nevnt i et Space', 'Mentioned in a Space'),
      description: i18n.tr('E-post når noen nevner deg.', 'Email when someone mentions you.'),
    },
    'space.reply': {
      title: i18n.tr('Svar i et Space', 'Reply in a Space'),
      description: i18n.tr('E-post når noen svarer deg.', 'Email when someone replies to you.'),
    },
    'space.invite': {
      title: i18n.tr('Lagt til i et Space', 'Added to a Space'),
      description: i18n.tr('E-post når du blir lagt til i et Space.', 'Email when you are added to a Space.'),
    },
    'space.agent_done': {
      title: i18n.tr('Agent ferdig', 'Agent finished'),
      description: i18n.tr('E-post når en agent du startet er ferdig.', 'Email when an agent you started finishes.'),
    },
    'space.agent_failed': {
      title: i18n.tr('Agent feilet', 'Agent failed'),
      description: i18n.tr('E-post når en agent du startet feiler.', 'Email when an agent you started fails.'),
    },
    'space.approval_needed': {
      title: i18n.tr('Godkjenning trengs', 'Approval needed'),
      description: i18n.tr('E-post når en agent venter på godkjenning fra deg.', 'Email when an agent is waiting for your approval.'),
    },
    'space.agent_awaiting': {
      title: i18n.tr('Agent venter', 'Agent waiting'),
      description: i18n.tr('E-post når en agent venter på deg.', 'Email when an agent is waiting for you.'),
    },
    'space.reminder': {
      title: i18n.tr('Påminnelse', 'Reminder'),
      description: i18n.tr('E-post når en påminnelse du satte forfaller.', 'Email when a reminder you set comes due.'),
    },
  })

  const isOn = (type: SpaceSignalType, defaultOn: boolean) => explicit().get(type) ?? defaultOn

  const change = async (type: SpaceSignalType, enabled: boolean) => {
    const previous = explicit()
    setExplicit(new Map(previous).set(type, enabled))
    setError(null)
    try {
      await updateNotificationPreference(type, 'email', enabled)
    } catch {
      setExplicit(previous)
      setError(i18n.tr('Kunne ikke lagre varselvalget.', 'Could not save the notification choice.'))
    }
  }

  return (
    <section id="space-notifications" class="verevon-settings-section">
      <SectionHeader
        title={i18n.tr('Space-varsler på e-post', 'Space email notifications')}
        description={i18n.tr(
          'Velg hvilke Space-hendelser som sendes på e-post. Selve innboksen i appen styres i Space.',
          'Choose which Space events are sent by email. The in-app inbox is controlled in Space.',
        )}
      />
      <Show when={error()}>
        <p role="alert">{error()}</p>
      </Show>
      <div class="verevon-settings-divided-list">
        <For each={SPACE_EMAIL_SIGNALS}>
          {(signal) => (
            <ToggleRow
              title={labels()[signal.type].title}
              description={labels()[signal.type].description}
              enabled={isOn(signal.type, signal.defaultOn)}
              onChange={(checked: boolean) => void change(signal.type, checked)}
            />
          )}
        </For>
      </div>
    </section>
  )
}
