import { BookOpen, Loader2, Sparkles, Trash2 } from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show } from 'solid-js'
import {
  createSkill,
  deleteSkill,
  listSkills,
  updateSkill,
  type Skill,
} from '@/shared/api/skills-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Ferdigheter (skills) — org-dekkende ferdigheter agenten kan bruke i chat.
 *
 * En ferdighet er en markdown-instruks med utløser-nøkkelord. Når en samtale
 * treffer nøkkelordene, injiserer model-gateway ferdighetens innhold i modellens
 * systemkontekst (`sse.rs::fetch_skill_context`), så den styrer svaret. Bare
 * administratorer kan opprette, redigere eller slette ferdigheter — de gjelder
 * hele organisasjonen.
 */

function parseKeywords(raw: string): string[] {
  return raw
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0)
}

export function SkillsSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [skills, { refetch }] = createResource(
    () => orgId() || undefined,
    (id) => listSkills(id).then((result) => result.skills),
  )

  const [name, setName] = createSignal('')
  const [description, setDescription] = createSignal('')
  const [content, setContent] = createSignal('')
  const [keywords, setKeywords] = createSignal('')

  const [submitting, setSubmitting] = createSignal(false)
  const [busySkillId, setBusySkillId] = createSignal<string | null>(null)
  const [formError, setFormError] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)

  const list = createMemo(() => skills() ?? [])

  const resetForm = () => {
    setName('')
    setDescription('')
    setContent('')
    setKeywords('')
  }

  const handleCreate = async (event: Event) => {
    event.preventDefault()
    const id = orgId()
    if (!id || submitting()) return

    const trimmedName = name().trim()
    const trimmedContent = content().trim()
    if (!trimmedName || !trimmedContent) {
      setFormError('Navn og innhold er påkrevd.')
      return
    }

    setSubmitting(true)
    setFormError(null)
    try {
      await createSkill(id, {
        name: trimmedName,
        description: description().trim() || undefined,
        content: trimmedContent,
        trigger_keywords: parseKeywords(keywords()),
        enabled: true,
      })
      resetForm()
      await refetch()
    } catch (err) {
      setFormError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke opprette ferdigheten.', en: 'Could not create the skill.' }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  const handleToggle = async (skill: Skill) => {
    const id = orgId()
    if (!id || busySkillId()) return
    setBusySkillId(skill.id)
    setActionError(null)
    try {
      await updateSkill(id, skill.id, { enabled: !skill.enabled })
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke oppdatere ferdigheten.', en: 'Could not update the skill.' }),
      )
    } finally {
      setBusySkillId(null)
    }
  }

  const handleDelete = async (skill: Skill) => {
    const id = orgId()
    if (!id || busySkillId()) return
    if (
      typeof window !== 'undefined' &&
      !window.confirm(`Slette ferdigheten «${skill.name}»?`)
    ) {
      return
    }
    setBusySkillId(skill.id)
    setActionError(null)
    try {
      await deleteSkill(id, skill.id)
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke slette ferdigheten.', en: 'Could not delete the skill.' }),
      )
    } finally {
      setBusySkillId(null)
    }
  }

  return (
    <>
      <SectionHeader
        title="Ferdigheter"
        description="Org-dekkende ferdigheter agenten kan bruke. En ferdighet er en instruks med utløser-nøkkelord; når en samtale treffer nøkkelordene, injiseres innholdet i modellens kontekst og styrer svaret."
      />

      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={skills.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          Kunne ikke laste ferdigheter.
        </p>
      </Show>

      <Show
        when={!skills.loading}
        fallback={
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            Laster ferdigheter…
          </p>
        }
      >
        <Show
          when={list().length > 0}
          fallback={
            <div class="verevon-settings-list-card">
              <p class="verevon-settings-empty-row">
                Ingen ferdigheter er opprettet ennå. Legg til en nedenfor.
              </p>
            </div>
          }
        >
          <div class="verevon-settings-list-card">
            <For each={list()}>
              {(skill) => (
                <div class="verevon-settings-integration-row">
                  <div>
                    <p>
                      {skill.name}{' '}
                      <span class="verevon-trust-chip" aria-label={`Status: ${skill.enabled ? 'aktiv' : 'deaktivert'}`}>
                        {skill.enabled ? 'aktiv' : 'deaktivert'}
                      </span>
                    </p>
                    <Show when={skill.description.trim().length > 0}>
                      <span>{skill.description}</span>
                    </Show>
                    <Show when={skill.trigger_keywords.length > 0}>
                      <span class="verevon-trust-chips">
                        <For each={skill.trigger_keywords}>
                          {(keyword) => <span class="verevon-trust-chip">{keyword}</span>}
                        </For>
                      </span>
                    </Show>
                  </div>
                  <Show when={isAdmin()}>
                    <div>
                      <SettingsButton
                        settingsSize="sm"
                        disabled={busySkillId() === skill.id}
                        onClick={() => void handleToggle(skill)}
                        aria-label={`${skill.enabled ? 'Deaktiver' : 'Aktiver'} ${skill.name}`}
                      >
                        <Show
                          when={busySkillId() === skill.id}
                          fallback={<>{skill.enabled ? 'Deaktiver' : 'Aktiver'}</>}
                        >
                          <Loader2 size={14} aria-hidden="true" /> Lagrer…
                        </Show>
                      </SettingsButton>{' '}
                      <SettingsButton
                        settingsSize="sm"
                        danger
                        disabled={busySkillId() === skill.id}
                        onClick={() => void handleDelete(skill)}
                        aria-label={`Slett ${skill.name}`}
                      >
                        <Trash2 size={14} aria-hidden="true" /> Slett
                      </SettingsButton>
                    </div>
                  </Show>
                </div>
              )}
            </For>
          </div>
        </Show>
      </Show>

      <Show
        when={isAdmin()}
        fallback={
          <p class="verevon-settings-subnote">
            <BookOpen size={14} aria-hidden="true" /> Bare administratorer kan opprette
            eller endre ferdigheter — de gjelder hele organisasjonen.
          </p>
        }
      >
        <form class="verevon-settings-field-grid verevon-settings-field-grid--spaced" onSubmit={handleCreate}>
          <label for="skill-name" class="verevon-settings-field">
            <span class="verevon-settings-label">Navn</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="skill-name"
                value={name()}
                required
                placeholder="f.eks. Utrullingsrutine"
                onInput={(event) => setName(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="skill-description" class="verevon-settings-field">
            <span class="verevon-settings-label">Beskrivelse (valgfritt)</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="skill-description"
                value={description()}
                placeholder="Kort hva ferdigheten gjør"
                onInput={(event) => setDescription(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="skill-keywords" class="verevon-settings-field">
            <span class="verevon-settings-label">Utløser-nøkkelord</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="skill-keywords"
                value={keywords()}
                placeholder="kommaseparert, f.eks. utrulling, deploy, release"
                onInput={(event) => setKeywords(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
            <span class="verevon-settings-help">
              Ferdigheten injiseres i chat når en samtale treffer disse ordene.
            </span>
          </label>

          <label for="skill-content" class="verevon-settings-field">
            <span class="verevon-settings-label">Innhold (markdown)</span>
            <span class="verevon-settings-input-wrap">
              <textarea
                id="skill-content"
                value={content()}
                required
                rows={5}
                placeholder={'Instruksjoner modellen skal følge, f.eks.\n1. Kjør testene\n2. Bygg\n3. Rull ut'}
                onInput={(event) => setContent(event.currentTarget.value)}
                class="verevon-settings-input verevon-settings-textarea"
              />
            </span>
          </label>

          <Show when={formError()}>
            {(message) => (
              <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <div>
            <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={submitting()}>
              <Show
                when={submitting()}
                fallback={<><Sparkles size={14} aria-hidden="true" /> Opprett ferdighet</>}
              >
                <Loader2 size={14} aria-hidden="true" /> Oppretter…
              </Show>
            </SettingsButton>
          </div>
        </form>
      </Show>

      <p class="verevon-settings-subnote">
        <BookOpen size={14} aria-hidden="true" /> Org-tilhørighet utledes fra den
        verifiserte økten. Ferdigheter gjelder alle i organisasjonen.
      </p>
    </>
  )
}
