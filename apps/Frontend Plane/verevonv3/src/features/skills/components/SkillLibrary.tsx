import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { createSkill, listSkills, type Skill } from '@/shared/api/skills-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * The skill library (EP-11 S9): every skill this person can see, by where it comes from. Mine and
 * Shared with me are people's own skills, Space holds skills that steer only one Space's threads,
 * Organisation the org's own, and Verevon the first-party skills built into the platform.
 *
 * The library reads; it decides nothing. What a turn may use is still settled on the server (the
 * skill's scope and owner, and the organisation's `ext.skills` policy), so a tab here can never
 * widen it. An admin may publish a skill someone shared with them to the organisation: the
 * existing promotion, which copies it as a new, disabled org skill.
 */

export type LibraryTier = 'mine' | 'shared' | 'space' | 'org' | 'verevon'

export const LIBRARY_TIERS: readonly LibraryTier[] = ['mine', 'shared', 'space', 'org', 'verevon']

/**
 * Where a listed skill belongs for this viewer. The registry stores no `verevon` row (first-party
 * skills live in the gateway), so a row is one of the other four. A scope this build does not know
 * is shown as nobody's own: it can only land under Shared with me, never under Mine or Organisation.
 */
export function libraryTier(skill: Skill, viewerId: string): Exclude<LibraryTier, 'verevon'> {
  // The registry's type names the two scopes it had before Space skills; a Space row says so.
  const scope: string = skill.scope ?? 'org'
  if (scope === 'org') return 'org'
  if (scope === 'space') return 'space'
  if (scope === 'user' && viewerId !== '' && skill.owner_user_id === viewerId) return 'mine'
  return 'shared'
}

/** The rows a library tab lists. Proposals and declined learned skills belong to the Læring inbox. */
export function skillsInTier(skills: readonly Skill[], tier: LibraryTier, viewerId: string): Skill[] {
  if (tier === 'verevon') return []
  return skills.filter(
    (skill) =>
      skill.review_state !== 'proposed' &&
      skill.review_state !== 'rejected' &&
      libraryTier(skill, viewerId) === tier,
  )
}

/**
 * Whether an admin may publish this skill to the organisation: an accepted skill a member shared,
 * as the promotion route requires. The server checks it again.
 */
export function canPublishToOrg(skill: Skill, viewerId: string, isAdmin: boolean): boolean {
  return (
    isAdmin &&
    skill.scope === 'user' &&
    skill.review_state === 'accepted' &&
    skill.owner_user_id !== viewerId &&
    (skill.shared_with?.length ?? 0) > 0
  )
}

export function SkillLibrary() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))
  const viewerId = session.user?.id ?? ''
  const [tier, setTier] = createSignal<LibraryTier>('mine')
  const [busySkillId, setBusySkillId] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)
  const [published, setPublished] = createSignal<string | null>(null)

  const [skills, { refetch }] = createResource(
    () => orgId() || undefined,
    (id) => listSkills(id).then((result) => result.skills),
  )
  const visible = createMemo(() => skillsInTier(skills() ?? [], tier(), viewerId))

  const tierLabel = (value: LibraryTier): string => {
    switch (value) {
      case 'mine':
        return i18n.tr('Mine', 'Mine')
      case 'shared':
        return i18n.tr('Delt med meg', 'Shared with me')
      case 'space':
        return i18n.tr('Rom', 'Space')
      case 'org':
        return i18n.tr('Organisasjon', 'Organisation')
      case 'verevon':
        return 'Verevon'
    }
  }

  const emptyText = (value: LibraryTier): string => {
    switch (value) {
      case 'mine':
        return i18n.tr('Du har ingen egne ferdigheter ennå.', 'You have no skills of your own yet.')
      case 'shared':
        return i18n.tr('Ingen har delt en ferdighet med deg.', 'No one has shared a skill with you.')
      case 'space':
        return i18n.tr(
          'Ferdigheter for et rom gjelder bare samtaler i det rommet. Ingen er tilgjengelige her ennå.',
          'Space skills apply only to conversations in that Space. None are available here yet.',
        )
      case 'org':
        return i18n.tr('Organisasjonen har ingen ferdigheter ennå.', 'The organisation has no skills yet.')
      case 'verevon':
        return i18n.tr(
          'Verevons egne ferdigheter er innebygd og brukes når de passer. De kan ikke endres her.',
          "Verevon's own skills are built in and used when they fit. They cannot be changed here.",
        )
    }
  }

  const publish = async (skill: Skill) => {
    const id = orgId()
    if (!id || busySkillId()) return
    setBusySkillId(skill.id)
    setActionError(null)
    setPublished(null)
    try {
      // The copy starts disabled: an admin turns it on once they have read it.
      await createSkill(id, {
        name: skill.name,
        description: skill.description,
        content: skill.content,
        trigger_keywords: skill.trigger_keywords,
        enabled: false,
        scope: 'org',
        promoted_from: skill.id,
      })
      setPublished(skill.id)
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke publisere ferdigheten til organisasjonen.',
          en: 'Could not publish the skill to the organisation.',
        }),
      )
    } finally {
      setBusySkillId(null)
    }
  }

  return (
    <section class="verevon-settings-section" aria-label={i18n.tr('Ferdighetsbibliotek', 'Skill library')}>
      <SectionHeader
        title={i18n.tr('Ferdighetsbibliotek', 'Skill library')}
        description={i18n.tr(
          'Alle ferdigheter du kan se, etter hvor de kommer fra.',
          'Every skill you can see, by where it comes from.',
        )}
      />
      <div class="verevon-run-history__tabs" role="tablist" aria-label={i18n.tr('Bibliotek', 'Library')}>
        <For each={LIBRARY_TIERS}>
          {(value) => (
            <button
              type="button"
              role="tab"
              aria-selected={tier() === value ? 'true' : 'false'}
              data-tier={value}
              onClick={() => setTier(value)}
            >
              {tierLabel(value)}
            </button>
          )}
        </For>
      </div>
      <div role="tabpanel" aria-label={tierLabel(tier())}>
        <Show when={skills.loading}>
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            {i18n.tr('Laster…', 'Loading…')}
          </p>
        </Show>
        <Show when={skills.error}>
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {i18n.tr('Kunne ikke laste ferdighetene.', 'Could not load the skills.')}
          </p>
        </Show>
        <Show when={actionError()}>
          {(message) => (
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {message()}
            </p>
          )}
        </Show>
        <Show when={!skills.loading && !skills.error}>
          <Show
            when={visible().length > 0}
            fallback={<p class="verevon-settings-subnote">{emptyText(tier())}</p>}
          >
            <ul class="verevon-settings-list">
              <For each={visible()}>
                {(skill) => (
                  <li data-skill-id={skill.id}>
                    <strong>{skill.name}</strong>
                    <Show when={skill.description}>
                      <span class="verevon-settings-help"> {skill.description}</span>
                    </Show>
                    <Show when={!skill.enabled}>
                      <span class="verevon-trust-chip">{i18n.tr('Av', 'Off')}</span>
                    </Show>
                    <Show when={canPublishToOrg(skill, viewerId, isAdmin())}>
                      <SettingsButton
                        type="button"
                        disabled={busySkillId() !== null}
                        onClick={() => void publish(skill)}
                      >
                        {i18n.tr('Publiser til organisasjonen', 'Publish to organisation')}
                      </SettingsButton>
                    </Show>
                    <Show when={published() === skill.id}>
                      <span class="verevon-settings-help" role="status">
                        {i18n.tr(
                          'Kopiert til organisasjonen, slått av til den er lest.',
                          'Copied to the organisation, off until it is read.',
                        )}
                      </span>
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
      </div>
    </section>
  )
}
