/**
 * The review card under an artifact (F-t3-086): one read-only reviewer's verdict on the text on screen, the model's
 * located findings grouped by severity, and the gateway's own checks in a list of their own, so a reader can tell what
 * the model said from what the gateway verified. A finding can point at lines in the code view on screen. Nothing here
 * is stored: the card lives as long as the artifact view that asked for it.
 */
import { For, Match, Show, Switch, createMemo } from 'solid-js'
import type { ReviewCard, ReviewFinding, ReviewSourceCheck } from '@/shared/api/chat-client'
import { ApiError } from '@/shared/api/api-error'
import { ReviewRunContractError } from '@/shared/actions/chat-review'
import { AlertCircle, AlertTriangle, CircleCheck, CircleHelp, X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { formatLatency, formatUsd } from './chat-media-markdown'
import './review-run.css'

/**
 * `VITE_REVIEW_RUN=true` offers Review on a text artifact in a chat (F-t3-086, W5-086-b). Unset, or any value but `true`
 * (the default), no artifact offers it. Exact string, as the other chat switches.
 */
export const reviewRunEnabled = () => import.meta.env.VITE_REVIEW_RUN === 'true'

/** The lines a finding points at, 1-based and inclusive, as Model Gateway counts them. */
export type ReviewLineRange = { start: number; end: number }

/**
 * Why a review produced no card, as one line of copy. `refused_locally` is the contract refusing the text before anything
 * was sent; `off` is the gateway's review switch answering 404.
 */
export type ReviewRunFailureKind =
  | 'too_large'
  | 'no_text'
  | 'refused'
  | 'refused_locally'
  | 'unavailable'
  | 'off'
  | 'failed'

/** The severities, most severe first. */
const SEVERITIES = ['P0', 'P1', 'P2', 'P3'] as const

type Severity = ReviewFinding['severity']
type Verdict = ReviewCard['verdict']

/**
 * The verdict the card headlines. An unreadable reply is never headlined as clean, whatever its verdict field says
 * (F-t3-086).
 */
export function reviewHeadline(card: Pick<ReviewCard, 'status' | 'verdict'>): Verdict {
  return card.status === 'unreadable' || card.verdict === 'unreadable' ? 'unreadable' : card.verdict
}

/**
 * The reason a review call raised, for the copy the panel shows. It is typed by the status and the code the gateway sent,
 * and it never carries any part of the text.
 */
export function reviewRunFailureKind(error: unknown): ReviewRunFailureKind {
  if (error instanceof ReviewRunContractError) return 'refused_locally'
  if (error instanceof ApiError) {
    if (error.status === 413) return 'too_large'
    if (error.status === 400) return error.code === 'review_material_missing' ? 'no_text' : 'refused'
    if (error.status === 503) return 'unavailable'
  }
  return 'failed'
}

/**
 * The review card. `locatable` says the finding lines can be shown in the code view on screen, which is only the revision
 * this card read. `onLocate` is told which lines the person chose, and `onClose` removes the card.
 */
export function ReviewRunCard(props: {
  card: ReviewCard
  locatable: boolean
  onLocate?: (range: ReviewLineRange) => void
  onClose?: () => void
}) {
  const i18n = useI18n()
  const groups = createMemo(() => SEVERITIES.flatMap((severity) => {
    const findings = props.card.findings.filter((finding) => finding.severity === severity)
    return findings.length > 0 ? [{ severity, findings }] : []
  }))

  const verdict = createMemo(() => {
    const kind = reviewHeadline(props.card)
    const count = props.card.findings.length
    const copy: Record<Verdict, { title: string; note: string }> = {
      clean: {
        title: i18n.tr('Ingen funn', 'No findings'),
        note: i18n.tr('Ingen funn ble meldt, og gatewayen fjernet ingen.', 'Nothing was reported, and the gateway removed nothing.'),
      },
      findings: {
        title: count === 1 ? i18n.tr('1 funn', '1 finding') : i18n.tr(`${count} funn`, `${count} findings`),
        note: i18n.tr('Hvert funn er sjekket mot teksten.', 'Each finding was checked against the text.'),
      },
      unverified: {
        title: i18n.tr('Ikke verifisert', 'Not verified'),
        note: i18n.tr(
          'Modellen meldte funn, men gatewayen kunne ikke sjekke noen av dem mot teksten. Ingenting her er verifisert.',
          'The model reported findings, but the gateway could not check any of them against the text. Nothing here is verified.',
        ),
      },
      unreadable: {
        title: i18n.tr('Svaret kunne ikke leses', 'The reply could not be read'),
        note: i18n.tr('Ingen funn vises. Dette er ikke en ren gjennomgang.', 'No findings are shown. This is not a clean review.'),
      },
    }
    return { kind, ...copy[kind] }
  })

  const severityName = (severity: Severity): string => ({
    P0: i18n.tr('Kritisk', 'Critical'),
    P1: i18n.tr('Høy', 'High'),
    P2: i18n.tr('Middels', 'Medium'),
    P3: i18n.tr('Lav', 'Low'),
  })[severity]

  const checkName = (check: ReviewSourceCheck['check']): string => ({
    reply_contract: i18n.tr('Svarformat', 'Reply format'),
    line_locations: i18n.tr('Linjeangivelser', 'Line locations'),
    quoted_text: i18n.tr('Sitater', 'Quoted text'),
  })[check]

  const outcomeName = (outcome: ReviewSourceCheck['outcome']): string => ({
    passed: i18n.tr('Bestått', 'Passed'),
    failed: i18n.tr('Ikke bestått', 'Failed'),
    not_run: i18n.tr('Ikke kjørt', 'Not run'),
  })[outcome]

  return (
    <section class="verevon-review-run" aria-label={i18n.tr('Gjennomgang', 'Review')}>
      <header class="verevon-review-run__head">
        <span class="verevon-review-run__title">{i18n.tr('Gjennomgang', 'Review')}</span>
        <span class="verevon-review-run__label">{props.card.label}</span>
        <Show when={props.onClose !== undefined}>
          <button
            type="button"
            class="verevon-review-run__close"
            aria-label={i18n.tr('Lukk gjennomgangen', 'Close the review')}
            onClick={() => props.onClose?.()}
          >
            <X size={14} />
          </button>
        </Show>
      </header>

      <div class={`verevon-review-run__verdict verevon-review-run__verdict--${verdict().kind}`}>
        <span class="verevon-review-run__verdict-icon" aria-hidden="true">
          <VerdictIcon kind={verdict().kind} />
        </span>
        <div class="verevon-review-run__summary">
          <strong>{verdict().title}</strong>
          <p>{verdict().note}</p>
        </div>
      </div>

      <Show when={groups().length > 0}>
        <div class="verevon-review-run__groups">
          <For each={groups()}>
            {(group) => (
              <section class="verevon-review-run__group" data-severity={group.severity}>
                <h4 class="verevon-review-run__group-title">{`${group.severity} · ${severityName(group.severity)}`}</h4>
                <ol class="verevon-review-run__findings">
                  <For each={group.findings}>
                    {(finding) => (
                      <ReviewFindingItem finding={finding} locatable={props.locatable} onLocate={props.onLocate} />
                    )}
                  </For>
                </ol>
              </section>
            )}
          </For>
        </div>
      </Show>

      <Show when={props.card.limitReached}>
        <p class="verevon-review-run__notice" role="note">
          {i18n.tr(
            'Flere funn ble sjekket enn dette kortet rommer. De mest alvorlige vises.',
            'More findings were checked than this card holds. The most severe are shown.',
          )}
        </p>
      </Show>

      <section class="verevon-review-run__checks">
        <h4 class="verevon-review-run__group-title">{i18n.tr('Gatewaysjekker', 'Gateway checks')}</h4>
        <p class="verevon-review-run__note">
          {i18n.tr(
            'Gatewayen kjørte disse sjekkene selv. De er ikke modellens funn.',
            'The gateway ran these checks itself. They are not the model\'s findings.',
          )}
        </p>
        <ul class="verevon-review-run__check-list">
          <For each={props.card.sourceChecks}>
            {(check) => (
              <li class="verevon-review-run__check" data-outcome={check.outcome}>
                <span class="verevon-review-run__check-name">{checkName(check.check)}</span>
                <strong>{outcomeName(check.outcome)}</strong>
                <Show when={check.dropped > 0}>
                  <span class="verevon-review-run__check-dropped">
                    {i18n.tr(`${check.dropped} fjernet`, `${check.dropped} dropped`)}
                  </span>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </section>

      <footer class="verevon-review-run__foot">
        <Show when={props.card.modelUsed}>
          {(model) => <span>{model()}</span>}
        </Show>
        <span>{formatLatency(props.card.usage.latencyMs)}</span>
        <Show when={props.card.usage.costUsd != null}>
          <span>{formatUsd(props.card.usage.costUsd ?? 0)}</span>
        </Show>
      </footer>
    </section>
  )
}

/** One finding the gateway kept: its severity, title, the lines it points at, what is wrong, and the quote it cites. */
function ReviewFindingItem(props: {
  finding: ReviewFinding
  locatable: boolean
  onLocate?: (range: ReviewLineRange) => void
}) {
  const i18n = useI18n()
  const lines = () => (props.finding.lineStart === props.finding.lineEnd
    ? i18n.tr(`Linje ${props.finding.lineStart}`, `Line ${props.finding.lineStart}`)
    : i18n.tr(
      `Linjer ${props.finding.lineStart}–${props.finding.lineEnd}`,
      `Lines ${props.finding.lineStart}–${props.finding.lineEnd}`,
    ))

  return (
    <li class="verevon-review-run__finding">
      <div class="verevon-review-run__finding-head">
        <span class="verevon-review-run__severity">{props.finding.severity}</span>
        <strong class="verevon-review-run__finding-title">{props.finding.title}</strong>
        <Show
          when={props.locatable && props.onLocate !== undefined}
          fallback={<span class="verevon-review-run__lines">{lines()}</span>}
        >
          <button
            type="button"
            class="verevon-review-run__lines verevon-review-run__lines--action"
            title={i18n.tr('Vis linjene i koden', 'Show the lines in the code')}
            onClick={() => props.onLocate?.({ start: props.finding.lineStart, end: props.finding.lineEnd })}
          >
            {lines()}
          </button>
        </Show>
      </div>
      <p class="verevon-review-run__body">{props.finding.body}</p>
      <Show when={props.finding.suggestion}>
        <p class="verevon-review-run__suggestion">
          <span>{i18n.tr('Forslag', 'Suggestion')}</span> {props.finding.suggestion}
        </p>
      </Show>
      <blockquote class="verevon-review-run__quote">{props.finding.quote}</blockquote>
    </li>
  )
}

/** The verdict's icon. Each verdict has its own shape, so the state never rests on colour alone. */
function VerdictIcon(props: { kind: Verdict }) {
  return (
    <Switch>
      <Match when={props.kind === 'clean'}><CircleCheck size={16} /></Match>
      <Match when={props.kind === 'findings'}><AlertTriangle size={16} /></Match>
      <Match when={props.kind === 'unverified'}><CircleHelp size={16} /></Match>
      <Match when={props.kind === 'unreadable'}><AlertCircle size={16} /></Match>
    </Switch>
  )
}

/** The one line of copy a review that produced no card leaves behind. A refusal names the limit, never the text. */
export function ReviewRunFailure(props: { kind: ReviewRunFailureKind }) {
  const i18n = useI18n()
  const message = (): string => ({
    too_large: i18n.tr(
      'Teksten er for stor til å gjennomgås. En gjennomgang tar opptil 60 000 byte og 2 000 linjer.',
      'The text is too large to review. A review takes up to 60,000 bytes and 2,000 lines.',
    ),
    no_text: i18n.tr('Det er ingen tekst å gjennomgå.', 'There is no text to review.'),
    refused: i18n.tr('Gjennomgangen tok ikke imot denne teksten.', 'The review did not take this text.'),
    refused_locally: i18n.tr(
      'Teksten kan ikke gjennomgås. Den er tom, eller over grensen på 60 000 byte og 2 000 linjer.',
      'This text cannot be reviewed. It is empty, or over the limit of 60,000 bytes and 2,000 lines.',
    ),
    unavailable: i18n.tr(
      'Gjennomgangen er ikke tilgjengelig akkurat nå. Prøv igjen om litt.',
      'The review is not available right now. Try again in a moment.',
    ),
    off: i18n.tr('Gjennomgang er ikke slått på her ennå.', 'Review is not switched on here yet.'),
    failed: i18n.tr('Kunne ikke gjennomgå artefaktet. Prøv igjen.', 'Could not review the artifact. Please try again.'),
  })[props.kind]

  return (
    <p class="verevon-review-run__failure" role={props.kind === 'off' ? 'status' : 'alert'}>
      {message()}
    </p>
  )
}
