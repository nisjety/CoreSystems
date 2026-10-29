import { Bot, X } from 'lucide-solid'
import { createSignal, For, Show } from 'solid-js'

import { createSpaceAgent } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'

/**
 * The in-room create-agent flow, Grok Bot's ergonomics on our governed rails
 * (`docs/space-defenition.md` "Creating agents", scope plan §UI-3b): pick a
 * color, name it, optionally start from a template — one step for the human.
 * Server-side it is still the two-step flow; this dialog holds no authority
 * and simply reports the outcome, including the truthful "created but still
 * pending" state when Control has not confirmed the roster yet.
 *
 * Deliberately small: no model picker, no tools, no knowledge bindings. Room
 * agents are the simple kind — the dividing rule sends everything more
 * advanced to Agent Studio.
 */

export const AGENT_AVATAR_COLORS = [
  '#8f5b3c',
  '#dc2626',
  '#ea7317',
  '#f0a202',
  '#16a34a',
  '#0d9488',
  '#2563eb',
  '#7c3aed',
  '#db2777',
  '#64748b',
] as const

type AgentTemplate = {
  readonly id: string
  readonly nameNo: string
  readonly nameEn: string
  readonly descriptionNo: string
  readonly descriptionEn: string
  readonly instructionsNo: string
  readonly instructionsEn: string
}

const AGENT_TEMPLATES: readonly AgentTemplate[] = [
  {
    id: 'referent',
    nameNo: 'Møtereferent',
    nameEn: 'Meeting Scribe',
    descriptionNo: 'Oppsummerer samtaler til referat med beslutninger og oppgaver.',
    descriptionEn: 'Summarizes conversations into minutes with decisions and tasks.',
    instructionsNo:
      'Du er rommets møtereferent. Når du blir nevnt, oppsummer samtalen så langt som et kort referat: beslutninger først, deretter åpne spørsmål, deretter oppgaver med ansvarlig person når det fremgår. Vær nøktern og ikke dikt opp noe som ikke står i samtalen.',
    instructionsEn:
      'You are the room\'s meeting scribe. When mentioned, summarize the conversation so far as brief minutes: decisions first, then open questions, then tasks with owners when stated. Be factual and never invent anything not present in the conversation.',
  },
  {
    id: 'status',
    nameNo: 'Statusagent',
    nameEn: 'Status Agent',
    descriptionNo: 'Svarer på drifts- og statusspørsmål for teamet.',
    descriptionEn: 'Answers operations and status questions for the team.',
    instructionsNo:
      'Du er rommets statusagent. Svar kort og strukturert på spørsmål om status og drift, med punktlister der det passer. Skill tydelig mellom det du vet fra tilgjengelig kontekst og det som må sjekkes manuelt.',
    instructionsEn:
      'You are the room\'s status agent. Answer status and operations questions briefly and in structure, using bullet lists where fitting. Separate clearly what you know from available context and what needs a manual check.',
  },
  {
    id: 'kunnskap',
    nameNo: 'Kunnskapshjelper',
    nameEn: 'Knowledge Helper',
    descriptionNo: 'Svarer med grunnlag i organisasjonens egen kunnskap.',
    descriptionEn: 'Answers grounded in the organization\'s own knowledge.',
    instructionsNo:
      'Du er rommets kunnskapshjelper. Svar på spørsmål med grunnlag i organisasjonens tilgjengelige kunnskap, og si tydelig fra når grunnlaget mangler i stedet for å gjette.',
    instructionsEn:
      'You are the room\'s knowledge helper. Answer questions grounded in the organization\'s available knowledge, and say plainly when grounding is missing instead of guessing.',
  },
]

export interface SpaceCreateAgentDialogProps {
  readonly spaceRef: string
  readonly open: () => boolean
  readonly onClose: () => void
  /** Called after Control has confirmed the new agent's room membership. */
  readonly onCreated?: () => void
}

export function SpaceCreateAgentDialog(props: SpaceCreateAgentDialogProps) {
  const i18n = useI18n()
  const [name, setName] = createSignal('')
  const [instructions, setInstructions] = createSignal('')
  const [color, setColor] = createSignal<string>(AGENT_AVATAR_COLORS[6])
  const [submitting, setSubmitting] = createSignal(false)
  const [errorMessage, setErrorMessage] = createSignal<string | undefined>(undefined)

  function applyTemplate(template: AgentTemplate): void {
    setName(i18n.tr(template.nameNo, template.nameEn))
    setInstructions(i18n.tr(template.instructionsNo, template.instructionsEn))
  }

  function close(): void {
    if (submitting()) return
    setErrorMessage(undefined)
    props.onClose()
  }

  async function submit(): Promise<void> {
    const trimmed = name().trim()
    if (trimmed.length < 2 || submitting()) return
    setSubmitting(true)
    setErrorMessage(undefined)
    try {
      await createSpaceAgent(props.spaceRef, {
        name: trimmed,
        instructions: instructions(),
        avatarColor: color(),
      })
      setName('')
      setInstructions('')
      props.onCreated?.()
      props.onClose()
    } catch {
      // The gateway's own vocabulary for the honest partial state: the agent
      // may exist as `pending` even when this request errors. The Agent tab
      // shows the truth either way; here we only report that it is not active.
      setErrorMessage(i18n.tr(
        'Agenten kunne ikke bekreftes som medlem av rommet. Sjekk Agent-fanen — den kan stå som ventende.',
        'The agent could not be confirmed as a room member. Check the Agent tab — it may be listed as pending.',
      ))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Show when={props.open()}>
      <div class="verevon-space-create-agent" role="presentation" onClick={(event) => { if (event.target === event.currentTarget) close() }}>
        <section
          class="verevon-space-create-agent__dialog"
          role="dialog"
          aria-modal="true"
          aria-labelledby="space-create-agent-title"
        >
          <header>
            <h3 id="space-create-agent-title">{i18n.tr('Opprett en agent', 'Create an agent')}</h3>
            <button type="button" onClick={close} aria-label={i18n.tr('Lukk', 'Close')}>
              <X size={16} />
            </button>
          </header>

          <div class="verevon-space-create-agent__preview" aria-hidden="true">
            <span class="verevon-space-create-agent__avatar" style={{ background: color() }}>
              <Bot size={26} />
            </span>
          </div>

          <div class="verevon-space-create-agent__colors" role="radiogroup" aria-label={i18n.tr('Agentfarge', 'Agent color')}>
            <For each={AGENT_AVATAR_COLORS}>
              {(candidate) => (
                <button
                  type="button"
                  role="radio"
                  aria-checked={color() === candidate}
                  aria-label={candidate}
                  classList={{ 'verevon-space-create-agent__color': true, 'verevon-space-create-agent__color--selected': color() === candidate }}
                  style={{ background: candidate }}
                  onClick={() => setColor(candidate)}
                />
              )}
            </For>
          </div>

          <label class="verevon-space-create-agent__field">
            <span>{i18n.tr('Navn', 'Name')}</span>
            <input
              value={name()}
              maxLength={60}
              placeholder={i18n.tr('Hva skal agenten hete?', 'What should the agent be called?')}
              onInput={(event) => setName(event.currentTarget.value)}
              disabled={submitting()}
            />
          </label>

          <label class="verevon-space-create-agent__field">
            <span>{i18n.tr('Instruksjoner', 'Instructions')}</span>
            <textarea
              value={instructions()}
              rows={4}
              maxLength={4000}
              placeholder={i18n.tr(
                'Hva skal agenten gjøre når den nevnes i rommet?',
                'What should the agent do when mentioned in the room?',
              )}
              onInput={(event) => setInstructions(event.currentTarget.value)}
              disabled={submitting()}
            />
          </label>

          <div class="verevon-space-create-agent__templates" aria-label={i18n.tr('Forslag', 'Suggestions')}>
            <p>{i18n.tr('Forslag', 'Suggestions')}</p>
            <div>
              <For each={AGENT_TEMPLATES}>
                {(template) => (
                  <button type="button" onClick={() => applyTemplate(template)} disabled={submitting()}>
                    <strong>{i18n.tr(template.nameNo, template.nameEn)}</strong>
                    <small>{i18n.tr(template.descriptionNo, template.descriptionEn)}</small>
                  </button>
                )}
              </For>
            </div>
          </div>

          <Show when={errorMessage()}>
            {(message) => <p class="verevon-space-projection-error" role="alert">{message()}</p>}
          </Show>

          <footer>
            <p class="verevon-space-create-agent__note">
              {i18n.tr(
                'Agenten blir medlem av dette rommet og svarer når den nevnes med @.',
                'The agent becomes a member of this room and answers when mentioned with @.',
              )}
            </p>
            <button
              type="button"
              class="verevon-space-create-agent__submit"
              disabled={name().trim().length < 2 || submitting()}
              onClick={() => void submit()}
            >
              {submitting() ? i18n.tr('Oppretter …', 'Creating…') : i18n.tr('Opprett agent', 'Create agent')}
            </button>
          </footer>
        </section>
      </div>
    </Show>
  )
}
