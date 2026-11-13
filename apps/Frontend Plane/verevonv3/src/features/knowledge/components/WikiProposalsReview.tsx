import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { wikiProposalStatuses, type WikiProposalStatus } from '@/shared/actions/knowledge-wiki-proposals'
import {
  listWikiPageSummaries,
  listWikiProposals,
  loadWikiPageContent,
  reviewWikiProposal,
  type WikiPageSummary,
  type WikiProposal,
} from '@/shared/api/wiki-proposals-client'
import { ApiError } from '@/shared/api/http'
import { diffText, type DiffLine, type DiffResult } from '@/shared/chat-nodes/diff'
import { formatMoment } from '@/shared/i18n/format'
import { translateApiError, useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { safeHref } from '@/shared/lib/safe-link'
import { CheckCircle2, XCircle } from '@/shared/icons'
import { Badge } from '@/shared/ui/Badge'
import { Button } from '@/shared/ui/Button'
import './WikiProposalsReview.css'

// The wiki proposals agents filed, for a person to accept or reject (W5-094-b, F-t3-094). The view is mounted in the
// Knowledge page behind VITE_WIKI_PROPOSALS_REVIEW. Accept and reject are explicit clicks; nothing here decides for the
// person. An edit shows a line diff against the page as it is now; a new page shows its full text, unrendered. A
// proposal whose page appeared after it was filed is shown as a conflict, and nothing was written for it.

type Copy = { no: string; en: string }
type Notice = { tone: 'ok' | 'error'; text: string }

const LOAD_FALLBACK: Copy = {
  no: 'Wiki-forslagene kunne ikke lastes. Prøv igjen.',
  en: 'The wiki proposals could not be loaded. Try again.',
}
const REVIEW_FALLBACK: Copy = {
  no: 'Forslaget kunne ikke behandles. Prøv igjen.',
  en: 'The proposal could not be reviewed. Try again.',
}
const CONFLICT_COPY: Copy = {
  no: 'Det finnes allerede en side på denne adressen. Ingenting er skrevet, og forslaget står nå som konflikt.',
  en: 'A live page already exists at this path. Nothing was written, and the proposal is now a conflict.',
}
const DECIDED_COPY: Copy = {
  no: 'Forslaget er allerede behandlet. Listen er oppdatert.',
  en: 'This proposal was already decided. The list has been refreshed.',
}
// The Data Plane refuses accept and reject alike for an organisation with zero retention on: a wiki write is durable.
const ZERO_RETENTION_COPY: Copy = {
  no: 'Wiki-forslag kan ikke godkjennes eller avvises mens nulldatalagring (ZDR) er på for organisasjonen. Ingenting er endret.',
  en: 'Wiki proposals cannot be accepted or rejected while zero data retention is on for the organisation. Nothing was changed.',
}

export function WikiProposalsReview() {
  const i18n = useI18n()
  const [status, setStatus] = createSignal<WikiProposalStatus>('pending')
  const [openId, setOpenId] = createSignal<string | null>(null)
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<Notice | null>(null)

  const [listing, listingActions] = createResource(status, (value) => listWikiProposals({ status: value }))
  // The page titles name the page each edit changes. A failure leaves the titles out, and the view still works.
  const [pages] = createResource(() => listWikiPageSummaries())

  // The open proposal's page is loaded only while it is open: the current text is the "before" side of its diff.
  const openProposal = createMemo(() => listing()?.proposals.find((proposal) => proposal.id === openId()) ?? null)
  const openPageId = () => {
    const proposal = openProposal()
    return proposal?.kind === 'edit' && proposal.pageId ? proposal.pageId : undefined
  }
  const [openPage] = createResource(openPageId, (pageId) => loadWikiPageContent(pageId))
  const openDiff = createMemo<DiffResult | null>(() => {
    const proposal = openProposal()
    const page = openPage()
    // The loaded page is kept after a close, so it counts only for the proposal it was loaded for.
    if (!proposal || proposal.kind !== 'edit' || !page || page.id !== proposal.pageId) return null
    return diffText(page.content, proposal.proposedContent)
  })

  const pageOf = (proposal: WikiProposal): WikiPageSummary | undefined => pages()?.get(proposal.pageId)

  const statusText = (value: WikiProposalStatus) =>
    ({
      pending: i18n.tr('Venter', 'Waiting'),
      accepted: i18n.tr('Godkjent', 'Accepted'),
      rejected: i18n.tr('Avvist', 'Rejected'),
      conflict: i18n.tr('Konflikt', 'Conflict'),
    })[value]

  const emptyText = () =>
    status() === 'pending'
      ? i18n.tr('Ingen forslag venter på godkjenning.', 'No proposals are waiting for review.')
      : i18n.tr('Ingen forslag med denne statusen.', 'No proposals have this status.')

  const loadErrorText = () => {
    const failure = listing.error
    // The Data Plane answers 404 while its create-page proposals are switched off.
    if (failure instanceof ApiError && failure.status === 404) {
      return i18n.tr(
        'Wiki-forslag er ikke slått på i denne installasjonen.',
        'Wiki proposals are not switched on in this installation.',
      )
    }
    return translateApiError(failure, i18n.tr, LOAD_FALLBACK)
  }

  const decidedText = (proposal: WikiProposal, decision: 'accept' | 'reject') => {
    if (decision === 'reject') {
      return i18n.tr('Forslaget er avvist. Ingenting er skrevet.', 'The proposal is rejected. Nothing was written.')
    }
    return proposal.kind === 'create_page'
      ? i18n.tr('Siden er opprettet i wiki-en.', 'The page is created in the wiki.')
      : i18n.tr('Den nye versjonen er lagret i wiki-en.', 'The new version is saved in the wiki.')
  }

  function chooseStatus(next: WikiProposalStatus) {
    setStatus(next)
    setOpenId(null)
    setNotice(null)
  }

  function refresh() {
    void listingActions.refetch().catch(() => undefined)
  }

  async function review(proposal: WikiProposal, decision: 'accept' | 'reject') {
    if (busyId() !== null) return
    setBusyId(proposal.id)
    setNotice(null)
    let conflicted = false
    try {
      await reviewWikiProposal(proposal.id, decision)
      setOpenId(null)
      setNotice({ tone: 'ok', text: decidedText(proposal, decision) })
    } catch (error) {
      setOpenId(null)
      if (error instanceof ApiError && error.status === 409 && error.code === 'wiki_proposal_conflict') {
        conflicted = true
        setNotice({ tone: 'error', text: i18n.tr(CONFLICT_COPY.no, CONFLICT_COPY.en) })
      } else if (error instanceof ApiError && error.status === 409) {
        setNotice({ tone: 'error', text: i18n.tr(DECIDED_COPY.no, DECIDED_COPY.en) })
      } else if (error instanceof ApiError && error.code === 'zdr_persistence_forbidden') {
        setNotice({ tone: 'error', text: i18n.tr(ZERO_RETENTION_COPY.no, ZERO_RETENTION_COPY.en) })
      } else {
        setNotice({ tone: 'error', text: translateApiError(error, i18n.tr, REVIEW_FALLBACK) })
      }
    } finally {
      setBusyId(null)
      // A conflict moves the person to the list where it now waits. That change fetches the list itself.
      if (conflicted) setStatus('conflict')
      else refresh()
    }
  }

  return (
    <section class="verevon-panel wiki-proposals-review" aria-labelledby="wiki-proposals-review-title">
      <header class="wiki-proposals-review__header">
        <h2 id="wiki-proposals-review-title">{i18n.tr('Wiki-forslag', 'Wiki proposals')}</h2>
        <p class="wiki-proposals-review__lead">
          {i18n.tr(
            'Agenter foreslår endringer i wiki-en og nye sider. Godkjenner du et forslag, lagres det. Avviser du det, skrives ingenting.',
            'Agents propose wiki edits and new pages. Accepting a proposal saves it; rejecting writes nothing.',
          )}
        </p>
      </header>

      <div class="wiki-proposals-review__filters" role="group" aria-label={i18n.tr('Filter på status', 'Filter by status')}>
        <For each={wikiProposalStatuses}>
          {(value) => (
            <button
              type="button"
              class="wiki-proposals-review__filter"
              aria-pressed={status() === value ? 'true' : 'false'}
              onClick={() => chooseStatus(value)}
            >
              {statusText(value)}
            </button>
          )}
        </For>
      </div>

      <Show when={notice()}>
        {(current) => (
          <p
            class={cn(
              'wiki-proposals-review__notice',
              current().tone === 'error' && 'wiki-proposals-review__notice--error',
            )}
            role="status"
          >
            {current().text}
          </p>
        )}
      </Show>

      <Show
        when={!listing.error}
        fallback={<p class="wiki-proposals-review__empty" role="alert">{loadErrorText()}</p>}
      >
        <Show
          when={listing()}
          fallback={<p class="wiki-proposals-review__empty">{i18n.tr('Laster forslag …', 'Loading proposals …')}</p>}
        >
          {(current) => (
            <Show
              when={current().proposals.length > 0}
              fallback={<p class="wiki-proposals-review__empty">{emptyText()}</p>}
            >
              <ol class="wiki-proposals-review__list">
                <For each={current().proposals}>
                  {(proposal) => (
                    <WikiProposalCard
                      proposal={proposal}
                      page={pageOf(proposal)}
                      statusText={statusText(proposal.status)}
                      open={openId() === proposal.id}
                      busy={busyId() !== null}
                      diff={openId() === proposal.id ? openDiff() : null}
                      pageFailed={openId() === proposal.id && Boolean(openPage.error)}
                      onToggle={() => setOpenId(openId() === proposal.id ? null : proposal.id)}
                      onReview={(decision) => void review(proposal, decision)}
                    />
                  )}
                </For>
              </ol>
            </Show>
          )}
        </Show>
      </Show>
    </section>
  )
}

type WikiProposalCardProps = {
  proposal: WikiProposal
  page: WikiPageSummary | undefined
  statusText: string
  open: boolean
  busy: boolean
  diff: DiffResult | null
  pageFailed: boolean
  onToggle: () => void
  onReview: (decision: 'accept' | 'reject') => void
}

function WikiProposalCard(props: WikiProposalCardProps) {
  const i18n = useI18n()
  const isEdit = () => props.proposal.kind === 'edit'
  const pending = () => props.proposal.status === 'pending'
  const title = () =>
    isEdit()
      ? props.page?.title || i18n.tr('Eksisterende side', 'Existing page')
      : props.proposal.proposedTitle || i18n.tr('Ny side', 'New page')
  const path = () => (isEdit() ? props.page?.path || null : props.proposal.proposedPath)

  return (
    <li class="wiki-proposals-review__card">
      <div class="wiki-proposals-review__badges">
        <Badge tone={isEdit() ? 'neutral' : 'accent'}>
          {isEdit() ? i18n.tr('Endring', 'Edit') : i18n.tr('Ny side', 'New page')}
        </Badge>
        <Show when={!pending()}>
          <Badge tone={props.proposal.status === 'conflict' ? 'risk' : 'neutral'}>{props.statusText}</Badge>
        </Show>
      </div>
      <h3 class="wiki-proposals-review__title">{title()}</h3>
      <Show when={path()}>
        {(value) => <p class="wiki-proposals-review__path">{value()}</p>}
      </Show>
      <Show when={props.proposal.editReason}>
        {(reason) => (
          <p class="wiki-proposals-review__reason">
            <span class="wiki-proposals-review__label">{i18n.tr('Begrunnelse', 'Reason')}</span> {reason()}
          </p>
        )}
      </Show>
      <dl class="wiki-proposals-review__meta">
        <Show when={props.proposal.proposedByAgent}>
          {(agent) => (
            <div>
              <dt>{i18n.tr('Agent', 'Agent')}</dt>
              <dd>{agent()}</dd>
            </div>
          )}
        </Show>
        <div>
          <dt>{i18n.tr('Opprettet', 'Created')}</dt>
          <dd>{formatMoment(props.proposal.createdAt, i18n.locale())}</dd>
        </div>
        <Show when={props.proposal.reviewedBy}>
          {(reviewer) => (
            <div>
              <dt>{i18n.tr('Behandlet av', 'Reviewed by')}</dt>
              <dd>{reviewer()}</dd>
            </div>
          )}
        </Show>
      </dl>
      <Show when={props.proposal.sourceRefs.length > 0}>
        <div class="wiki-proposals-review__refs">
          <span class="wiki-proposals-review__label">{i18n.tr('Kilder', 'Sources')}</span>
          <ul>
            <For each={props.proposal.sourceRefs}>
              {(source) => (
                <li>
                  <SourceRef value={source} />
                </li>
              )}
            </For>
          </ul>
        </div>
      </Show>
      <div class="wiki-proposals-review__actions">
        <Button size="sm" variant="ghost" aria-expanded={props.open ? 'true' : 'false'} onClick={() => props.onToggle()}>
          {props.open
            ? i18n.tr('Skjul', 'Hide')
            : isEdit()
              ? i18n.tr('Vis endringer', 'Show changes')
              : i18n.tr('Vis forhåndsvisning', 'Show preview')}
        </Button>
        <Show when={pending()}>
          <Button size="sm" variant="secondary" disabled={props.busy} onClick={() => props.onReview('reject')}>
            <XCircle class="size-4" />
            {i18n.tr('Avvis', 'Reject')}
          </Button>
          <Button size="sm" variant="primary" disabled={props.busy} onClick={() => props.onReview('accept')}>
            <CheckCircle2 class="size-4" />
            {i18n.tr('Godkjenn', 'Accept')}
          </Button>
        </Show>
      </div>
      <Show when={props.open}>
        <div class="wiki-proposals-review__panel">
          <Show when={isEdit()} fallback={<ProposalPreview content={props.proposal.proposedContent} />}>
            <EditChanges
              diff={props.diff}
              failed={props.pageFailed}
              proposedContent={props.proposal.proposedContent}
            />
          </Show>
        </div>
      </Show>
    </li>
  )
}

/** A source ref is a link when it is a safe web or mail address, and plain text otherwise. */
function SourceRef(props: { value: string }) {
  return (
    <Show
      when={safeHref(props.value)}
      fallback={<span class="wiki-proposals-review__ref">{props.value}</span>}
    >
      {(href) => (
        <a class="wiki-proposals-review__ref" href={href()} target="_blank" rel="noopener noreferrer">
          {props.value}
        </a>
      )}
    </Show>
  )
}

function EditChanges(props: { diff: DiffResult | null; failed: boolean; proposedContent: string }) {
  const i18n = useI18n()
  return (
    <Show
      when={props.diff}
      fallback={
        <p class="wiki-proposals-review__empty">
          {props.failed
            ? i18n.tr('Gjeldende side kunne ikke lastes.', 'The current page could not be loaded.')
            : i18n.tr('Laster gjeldende side …', 'Loading the current page …')}
        </p>
      }
    >
      {(result) => <DiffView result={result()} proposedContent={props.proposedContent} />}
    </Show>
  )
}

function DiffView(props: { result: DiffResult; proposedContent: string }) {
  const i18n = useI18n()
  return (
    <Show
      when={!props.result.truncated}
      fallback={
        <>
          <p class="wiki-proposals-review__empty">
            {i18n.tr(
              'Siden er for lang til å vise linje for linje. Hele forslaget vises under.',
              'The page is too long to show line by line. The full proposal is shown below.',
            )}
          </p>
          <ProposalPreview content={props.proposedContent} />
        </>
      }
    >
      <Show
        when={props.result.hunks.length > 0}
        fallback={<p class="wiki-proposals-review__empty">{i18n.tr('Ingen endringer i teksten.', 'No changes to the text.')}</p>}
      >
        <div class="wiki-proposals-review__diff" role="group" aria-label={i18n.tr('Endringer i teksten', 'Changes to the text')}>
          <For each={props.result.hunks}>
            {(hunk) => (
              <div class="wiki-proposals-review__hunk">
                <For each={hunk.lines}>
                  {(line) => (
                    <div class={cn('wiki-proposals-review__line', `wiki-proposals-review__line--${line.kind}`)}>
                      <span class="wiki-proposals-review__mark" aria-hidden="true">
                        {markOf(line.kind)}
                      </span>
                      <span class="wiki-proposals-review__sr">{srLabel(line.kind, i18n.tr)}</span>
                      <span class="wiki-proposals-review__text">{line.text === '' ? ' ' : line.text}</span>
                    </div>
                  )}
                </For>
              </div>
            )}
          </For>
        </div>
      </Show>
    </Show>
  )
}

function ProposalPreview(props: { content: string }) {
  const i18n = useI18n()
  return (
    <>
      <p class="wiki-proposals-review__label">{i18n.tr('Slik blir siden', 'How the page will read')}</p>
      <pre class="wiki-proposals-review__preview">{props.content}</pre>
    </>
  )
}

function markOf(kind: DiffLine['kind']): string {
  if (kind === 'added') return '+'
  if (kind === 'removed') return '-'
  return ' '
}

function srLabel(kind: DiffLine['kind'], tr: (no: string, en: string) => string): string {
  if (kind === 'added') return tr('Lagt til: ', 'Added: ')
  if (kind === 'removed') return tr('Fjernet: ', 'Removed: ')
  return ''
}
