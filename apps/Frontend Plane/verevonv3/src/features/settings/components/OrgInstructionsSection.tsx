import { Loader2, Sparkles } from '@/shared/icons'
import { createMemo, createSignal, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  getOrganizationInstructions,
  updateOrganizationInstructions,
} from '@/shared/api/organization-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

const MAX_INSTRUCTIONS_LENGTH = 4000

/**
 * ADR-0003's org layer — org-admin-authored instructions composed into every
 * chat turn's system message alongside the platform and Space layers
 * (`apps/AUTHORED_INSTRUCTIONS_ADR_2026-08-19.md`). Mirrors `SkillsSection`'s
 * admin gate: everyone can see what is authored, only an admin can change it.
 */
export function OrgInstructionsSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [saved, { refetch }] = createResource(
    () => orgId() || undefined,
    (id) => getOrganizationInstructions(id),
  )

  const [draft, setDraft] = createSignal('')
  const [dirty, setDirty] = createSignal(false)
  const [submitting, setSubmitting] = createSignal(false)
  const [formError, setFormError] = createSignal<string | null>(null)

  const value = createMemo(() => (dirty() ? draft() : saved() ?? ''))

  const handleInput = (event: InputEvent & { currentTarget: HTMLTextAreaElement }) => {
    setDraft(event.currentTarget.value)
    setDirty(true)
  }

  const handleSubmit = async (event: Event) => {
    event.preventDefault()
    const id = orgId()
    if (!id || submitting()) return
    setSubmitting(true)
    setFormError(null)
    try {
      await updateOrganizationInstructions(id, value())
      setDirty(false)
      await refetch()
    } catch (err) {
      setFormError(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke lagre organisasjonens instrukser.',
          en: 'Could not save the organization instructions.',
        }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <>
      <SectionHeader
        title="Organisasjonsinstrukser"
        description="Instrukser som gjelder for hele organisasjonen, og legges til i hver samtale sammen med eventuelle rom- og agentinstrukser. De kan utvide, men ikke overstyre, plattformens egne instrukser."
      />

      <Show when={formError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={saved.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          Kunne ikke laste organisasjonens instrukser.
        </p>
      </Show>

      <Show
        when={isAdmin()}
        fallback={
          <div class="verevon-settings-list-card">
            <Show
              when={!saved.loading}
              fallback={<p class="verevon-settings-subnote" role="status" aria-busy="true">Laster …</p>}
            >
              <Show
                when={value().trim().length > 0}
                fallback={<p class="verevon-settings-empty-row">Ingen organisasjonsinstrukser er lagt til ennå.</p>}
              >
                <p style={{ "white-space": "pre-wrap" }}>{value()}</p>
              </Show>
            </Show>
            <p class="verevon-settings-subnote">
              Bare administratorer kan endre organisasjonsinstrukser.
            </p>
          </div>
        }
      >
        <form class="verevon-settings-field-grid verevon-settings-field-grid--spaced" onSubmit={handleSubmit}>
          <label for="org-instructions" class="verevon-settings-field">
            <span class="verevon-settings-label">Instrukser</span>
            <span class="verevon-settings-input-wrap">
              <Show
                when={!saved.loading}
                fallback={<p class="verevon-settings-subnote" role="status" aria-busy="true">Laster …</p>}
              >
                <textarea
                  id="org-instructions"
                  value={value()}
                  rows={8}
                  maxlength={MAX_INSTRUCTIONS_LENGTH}
                  placeholder={'F.eks.\nSvar alltid på norsk med mindre kunden skriver på et annet språk.\nOppgi alltid saksnummer når du refererer til en sak.'}
                  onInput={handleInput}
                  class="verevon-settings-input verevon-settings-textarea"
                />
              </Show>
            </span>
            <span class="verevon-settings-help">
              {value().length} / {MAX_INSTRUCTIONS_LENGTH} tegn
            </span>
          </label>

          <div>
            <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={submitting() || !dirty()}>
              <Show when={submitting()} fallback={<><Sparkles size={14} aria-hidden="true" /> Lagre instrukser</>}>
                <Loader2 size={14} aria-hidden="true" /> Lagrer…
              </Show>
            </SettingsButton>
          </div>
        </form>
      </Show>
    </>
  )
}
