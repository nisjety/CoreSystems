import { createEffect, createSignal, For, Match, onCleanup, Show, Switch } from 'solid-js'
import { ArrowLeft, Building2, CalendarDays, ExternalLink, FileText, Layers, ShieldCheck } from 'lucide-solid'
import { fetchTenderAudit, fetchTenderNotice, type TenderAudit } from '@/shared/api/anbud-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import { anbudDetail, anbudWorkspace, anbudState, type TenderDetailSection } from '@/features/anbud/lib/anbud-state'
import { TenderAuditPanel } from '@/features/anbud/components/TenderAuditPanel'
import { anbudStages } from '@/features/anbud/lib/pipeline'
import { arrangementLabel, durationLabel, endDateLabel, winnersLabel, formatTenderMoney, noticeStatus, noticeTypeLabel, validDocumentUrl } from '@/features/anbud/lib/notice-presentation'
import { setPipelineStage } from '@/features/anbud/lib/workspace-sync'

export function TenderDetailView() {
  const { tr } = useI18n()
  let body: HTMLDivElement | undefined
  createEffect(() => { anbudDetail.section(); if (body) body.scrollTop = 0 })
  const [busy, setBusy] = createSignal(true)
  const [error, setError] = createSignal<string | null>(null)
  const [retry, setRetry] = createSignal(0)
  const [assessment, setAssessment] = createSignal<TenderAudit | null>(null)
  const [assessmentBusy, setAssessmentBusy] = createSignal(false)
  const [assessmentError, setAssessmentError] = createSignal<string | null>(null)
  createEffect(() => {
    const org = getSession().activeOrg?.id
    const profile = anbudState.submitted()
    const id = anbudDetail.id()
    const strictness = anbudState.strictness()
    if (anbudDetail.section() !== 'assessment' || !profile || !id || !org) return
    const controller = new AbortController()
    setAssessmentBusy(true); setAssessmentError(null); setAssessment(null)
    fetchTenderAudit(id, { profile, strictness }, controller.signal).then(value => {
      if (!controller.signal.aborted) { setAssessment(value); setAssessmentBusy(false) }
    }, err => {
      if (!controller.signal.aborted) { setAssessmentError(translateApiError(err, tr, { no: 'Vurderingen kunne ikke lastes.', en: 'The assessment could not be loaded.' })); setAssessmentBusy(false) }
    })
    onCleanup(() => controller.abort())
  })
  const notice = () => anbudDetail.data()?.result ?? anbudDetail.summary()
  const date = (value?: string) => value && !Number.isNaN(Date.parse(value)) ? new Date(value).toLocaleDateString(tr('nb-NO', 'en-GB'), { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'Europe/Oslo' }) : tr('Ikke oppgitt', 'Not stated')
  createEffect(() => {
    const id = anbudDetail.id()
    const org = getSession().activeOrg?.id
    retry()
    if (!id || !org) return
    const controller = new AbortController()
    setBusy(true); setError(null)
    fetchTenderNotice(id, controller.signal).then(value => {
      if (controller.signal.aborted) return
      anbudDetail.setData(value); setBusy(false)
    }, err => {
      if (controller.signal.aborted) return
      setError(translateApiError(err, tr, { no: 'Kunngjøringen kunne ikke lastes.', en: 'The notice could not be loaded.' })); setBusy(false)
    })
    onCleanup(() => controller.abort())
  })
  return <div ref={body} class="anbud-body anbud-detail-body"><main class="anbud-detail" aria-label={tr('Kunngjøringsdetaljer', 'Notice details')}>
    <button type="button" class="anbud-detail-back" onClick={anbudDetail.close}><ArrowLeft size={16} />{tr('Tilbake til anbud', 'Back to tenders')}</button>
    <Show when={notice()}>{item => <>
      <header class="anbud-detail-heading"><div class="anbud-card-buyer"><Building2 size={16} />{item().buyer ?? tr('Innkjøper ikke oppgitt', 'Buyer not stated')}</div><h1>{item().title}</h1><div class="anbud-detail-labels"><span class={`anbud-notice-status anbud-notice-status--${noticeStatus(item(), tr).value}`}>{noticeStatus(item(), tr).label}</span><span>{noticeTypeLabel(item(), tr)}</span><span>{item().noticeId}</span></div></header>
      <Show when={busy()}><p class="anbud-detail-loading" role="status">{tr('Laster kunngjøringen…', 'Loading notice…')}</p></Show>
      <Show when={error()}><div role="alert" class="anbud-detail-error"><p>{error()}</p><button type="button" class="anbud-button" onClick={() => setRetry(value => value + 1)}>{tr('Prøv igjen', 'Try again')}</button></div></Show>
      <Show when={!busy() && !error() && anbudDetail.data()}>{data => <>
        <div class="anbud-detail-status"><CalendarDays size={18} /><span>{item().deadline ? item().deadline!.daysLeft < 0 ? tr('Tilbudsfristen er utløpt.', 'The tender deadline has passed.') : tr(`${item().deadline!.daysLeft} dager igjen til tilbudsfristen.`, `${item().deadline!.daysLeft} days until the tender deadline.`) : item().plannedNotice ? tr(`Dette er en veiledende kunngjøring. Utlysningen er planlagt ${date(item().plannedNotice)}.`, `This is a prior information notice. The call for tenders is planned for ${date(item().plannedNotice)}.`) : tr('Ingen tilbudsfrist er oppgitt i kunngjøringen.', 'No tender deadline is stated in the notice.')}</span></div>
        <dl class="anbud-detail-facts"><div><dt>{tr('Tilbudsfrist', 'Tender deadline')}</dt><dd>{date(item().deadline?.at)}</dd></div><Show when={item().plannedNotice}><div><dt>{tr('Planlagt utlysning', 'Call planned')}</dt><dd>{date(item().plannedNotice)}</dd></div></Show><div><dt>{tr('Ordningstype', 'Arrangement')}</dt><dd>{arrangementLabel(item(), tr)}</dd></div><div><dt>{tr('Kunngjort verdi', 'Estimated value')}</dt><dd>{formatTenderMoney(item().estimatedValue)}</dd></div><div><dt>{tr('Faktisk kontraktsverdi', 'Awarded value')}</dt><dd>{formatTenderMoney(item().awardedValue)}</dd></div><div><dt>{tr('Publisert', 'Published')}</dt><dd>{date(item().publishedDate)}</dd></div><div><dt>{tr('Kontraktsvarighet', 'Contract duration')}</dt><dd>{durationLabel(item(), tr)}</dd></div><div><dt>{tr('Sluttdato', 'End date')}</dt><dd>{endDateLabel(item(), tr)}</dd></div><div><dt>{tr('Vinnere', 'Winners')}</dt><dd>{winnersLabel(item(), tr)}</dd></div></dl>
        <label class="anbud-detail-mobile-nav">{tr('Innhold', 'Content')}<select aria-label={tr('Anbudinnhold', 'Tender content')} value={anbudDetail.section()} onChange={e => anbudDetail.setSection(e.currentTarget.value as TenderDetailSection)}><option value="brief">Brief</option><option value="documents">{tr('Konkurransegrunnlag', 'Documents')}</option><option value="requirements">{tr('Kvalifikasjonskrav', 'Requirements')}</option><option value="lots">{tr('Delkontrakter', 'Lots')}</option><option value="evaluation">{tr('Tildelingskriterier', 'Evaluation criteria')}</option><option value="awards">{tr('Tildeling og kontrakt', 'Award and contract')}</option><option value="changes">{tr('Endringer', 'Amendments')}</option><option value="assessment">{tr('Vurdering mot kundeprofil', 'Company profile assessment')}</option><option value="source">{tr('Kilde og sporbarhet', 'Source and provenance')}</option></select></label>
        <Switch>
          <Match when={anbudDetail.section() === 'brief'}>
            <section class="anbud-detail-panel"><header><h2>{tr('Om anskaffelsen', 'About the procurement')}</h2><FileText size={18} /></header><Show when={data().notice.description} fallback={<p class="anbud-muted">{tr('Ingen beskrivelse er tilgjengelig i de innleste metadataene.', 'No description is available in the loaded metadata.')}</p>}><p class="anbud-detail-description">{data().notice.description}</p></Show><Show when={data().descriptionTruncated}><p class="anbud-muted">{tr('Beskrivelsen er forkortet. Se konkurransegrunnlaget for hele innholdet.', 'The description is shortened. See the procurement documents for the complete content.')}</p></Show><footer><span>{tr('Konkurranseform', 'Procedure')}: {item().procedureType ?? '—'}</span><button type="button" class="anbud-button" onClick={() => anbudDetail.setSection('documents')}>{tr(`Se dokumenter (${data().notice.documentsTotal})`, `View documents (${data().notice.documentsTotal})`)}<ExternalLink size={14} /></button></footer></section>
            <section class="anbud-detail-panel anbud-detail-decision"><div><h2>{tr('Er anbudet aktuelt?', 'Is this tender relevant?')}</h2><p>{tr('Flytt anbudet til riktig steg i pipeline. Status lagres for organisasjonen.', 'Move the tender to a pipeline stage. Stages are saved for your organisation.')}</p></div><select aria-label={tr('Pipeline-status', 'Pipeline stage')} value={anbudWorkspace.stages()[item().noticeId] ?? 'inbox'} onChange={e => setPipelineStage(item().noticeId, e.currentTarget.value)}><For each={anbudStages}>{stage => <option value={stage.id}>{tr(stage.no, stage.en)}</option>}</For></select></section>
            <section class="anbud-detail-panel"><header><h2>{tr('Kunngjøringsopplysninger', 'Notice information')}</h2><Layers size={18} /></header><dl class="anbud-detail-metadata"><dt>{tr('Innkjøper', 'Buyer')}</dt><dd>{item().buyer ?? '—'}</dd><dt>{tr('Kunngjøringstype', 'Notice type')}</dt><dd>{noticeTypeLabel(item(), tr)}</dd><dt>{tr('Publisert', 'Published')}</dt><dd>{date(item().publishedDate)}</dd><dt>{tr('Utstedt', 'Issued')}</dt><dd>{date(item().issuedAt)}</dd><dt>CPV</dt><dd>{item().cpv.join(', ') || '—'}</dd><dt>{tr('Regioner (NUTS)', 'Regions (NUTS)')}</dt><dd>{item().nuts.join(', ') || '—'}</dd></dl></section>

          </Match>
          <Match when={anbudDetail.section() === 'documents'}>
            <section class="anbud-detail-panel"><header><h2>{tr('Konkurransegrunnlag', 'Procurement documents')}</h2><FileText size={18} /></header><p class="anbud-muted">{tr('Lenker fra den opprinnelige kunngjøringen. Dokumentene åpnes hos utgiveren.', 'Links from the original notice. Documents open on the publisher’s website.')}</p><Show when={data().notice.documents.length} fallback={<p>{tr('Ingen dokumentlenker er oppgitt i de innleste metadataene.', 'No document links are stated in the loaded metadata.')}</p>}><ul class="anbud-document-list"><For each={data().notice.documents}>{(document, index) => <li><FileText size={18} /><div><strong>{tr(`Dokument ${index() + 1}`, `Document ${index() + 1}`)}</strong><small>{document.lot || tr('Hele anskaffelsen', 'Entire procurement')}</small></div><Show when={validDocumentUrl(document.url)} fallback={<span class="anbud-muted">{tr('Ugyldig lenke', 'Invalid link')}</span>}>{url => <a href={url()} target="_blank" rel="noopener noreferrer">{tr('Åpne dokument', 'Open document')}<ExternalLink size={14} /></a>}</Show></li>}</For></ul></Show></section>
          </Match>
          <Match when={anbudDetail.section() === 'requirements'}>
            <section class="anbud-detail-panel"><header><h2>{tr('Kvalifikasjonskrav', 'Qualification requirements')}</h2><ShieldCheck size={18} /></header><Show when={data().notice.criteriaInDocuments}><p>{tr('Kunngjøringen viser til konkurransegrunnlaget for kvalifikasjonskrav.', 'The notice refers to procurement documents for qualification requirements.')}</p></Show><Show when={data().notice.selectionCriteria.length} fallback={<p>{tr('Ingen kvalifikasjonskrav er lest fra kunngjøringen. Det betyr ikke at anskaffelsen er uten krav.', 'No qualification requirements were read from the notice. This does not mean the procurement has no requirements.')}</p>}><For each={data().notice.selectionCriteria}>{criterion => <article class="anbud-detail-item"><h3>{criterion.name || criterion.type}</h3><p>{criterion.description || '—'}</p><Show when={criterion.lot}><small>{criterion.lot}</small></Show></article>}</For></Show><Show when={data().notice.exclusionGrounds}><h3>{tr('Avvisningsgrunner', 'Exclusion grounds')} ({data().notice.exclusionGrounds})</h3><For each={data().notice.exclusionCriteria}>{criterion => <article class="anbud-detail-item"><h3>{criterion.type}</h3><p>{criterion.description || tr('Ingen utfyllende beskrivelse i kunngjøringen.', 'No further description in the notice.')}</p><small>{criterion.lot}</small></article>}</For></Show><p class="anbud-filter-note">{tr('Kravene vises fra kilden. Selskapets kvalifisering er ikke vurdert i denne visningen.', 'Requirements are shown from the source. Company qualification is not assessed in this view.')}</p></section>
          </Match>
          <Match when={anbudDetail.section() === 'lots'}>
            <section class="anbud-detail-panel"><header><h2>{tr('Delkontrakter', 'Lots')}</h2><Layers size={18} /></header><Show when={data().notice.lots.length} fallback={<p>{tr('Ingen delkontrakter er oppgitt.', 'No lots are stated.')}</p>}><For each={data().notice.lots}>{lot => <article class="anbud-detail-item"><h3>{lot.title || lot.id}</h3><small>{lot.id}</small><Show when={item().contracts?.find(contract => contract.lot === lot.id)}>{contract => <dl class="anbud-detail-metadata"><dt>{tr('Kontraktsvarighet', 'Contract duration')}</dt><dd>{durationLabel({ ...item(), contracts: [contract()] }, tr)}</dd><dt>{tr('Startdato', 'Start date')}</dt><dd>{date(contract().startDate)}</dd><dt>{tr('Sluttdato', 'End date')}</dt><dd>{date(contract().endDate)}{contract().endDate && contract().endBasis !== 'stated' ? tr(' (beregnet)', ' (calculated)') : ''}</dd></dl>}</Show><dl class="anbud-detail-metadata"><dt>{tr('Verdi', 'Value')}</dt><dd>{formatTenderMoney(lot.value)}</dd><dt>{tr('Frist', 'Deadline')}</dt><dd>{date(lot.tenderDeadline)}</dd><dt>{tr('Deltakelsesfrist', 'Participation deadline')}</dt><dd>{date(lot.participationDeadline)}</dd><dt>{tr('Maksimalt antall fornyelser', 'Maximum renewals')}</dt><dd>{lot.renewals ?? '—'}</dd><dt>CPV</dt><dd>{lot.cpv.join(', ') || '—'}</dd></dl><For each={lot.awardCriteria}>{criterion => <p><strong>{criterion.name || criterion.type}</strong>{criterion.weightPercent !== undefined ? ` · ${criterion.weightPercent}%` : ''}<br />{criterion.description}</p>}</For></article>}</For></Show></section>
          </Match>
          <Match when={anbudDetail.section() === 'evaluation'}>
            <section class="anbud-detail-panel"><h2>{tr('Tildelingskriterier', 'Evaluation criteria')}</h2><Show when={data().notice.lots.some(lot => lot.awardCriteria.length)} fallback={<p>{tr('Ingen tildelingskriterier er oppgitt i kunngjøringen. Se konkurransegrunnlaget.', 'No evaluation criteria are stated in the notice. See the procurement documents.')}</p>}><For each={data().notice.lots.filter(lot => lot.awardCriteria.length)}>{lot => <article class="anbud-detail-item"><h3>{lot.title || lot.id}</h3><For each={lot.awardCriteria}>{criterion => <div><h3>{criterion.name || criterion.type}{criterion.weightPercent !== undefined ? ` · ${criterion.weightPercent}%` : ''}</h3><p>{criterion.description || '—'}</p></div>}</For></article>}</For></Show></section>
          </Match>
          <Match when={anbudDetail.section() === 'awards'}>
            <section class="anbud-detail-panel"><h2>{tr('Tildeling og kontrakt', 'Award and contract')}</h2><dl class="anbud-detail-metadata"><dt>{tr('Tildelingsdato', 'Award date')}</dt><dd>{date(data().notice.awardDate)}</dd><dt>{tr('Vinnere', 'Winners')}</dt><dd>{winnersLabel(item(), tr)}</dd></dl><Show when={data().notice.awards?.length} fallback={<p>{tr('Ingen tildelingsresultater er oppgitt i kunngjøringen.', 'No award results are stated in the notice.')}</p>}><For each={data().notice.awards}>{award => <article class="anbud-detail-item"><h3>{award.lot}</h3><dl class="anbud-detail-metadata"><dt>{tr('Resultatkode', 'Result code')}</dt><dd>{award.status}</dd><dt>{tr('Mottatte tilbud', 'Tenders received')}</dt><dd>{award.tendersReceived ?? '—'}</dd><dt>{tr('Kontrakt signert', 'Contract signed')}</dt><dd>{date(award.contractSigned)}</dd><dt>{tr('Kontraktsverdi', 'Contract value')}</dt><dd>{formatTenderMoney(award.value)}</dd></dl></article>}</For></Show></section>
          </Match>
          <Match when={anbudDetail.section() === 'changes'}>
            <section class="anbud-detail-panel"><h2>{tr('Endringer', 'Amendments')}</h2><Show when={data().notice.change} fallback={<p>{tr('Kunngjøringen inneholder ingen endringsopplysninger.', 'This notice contains no amendment information.')}</p>}>{change => <><p class="anbud-detail-description">{change().reason || tr('Ingen begrunnelse oppgitt.', 'No reason stated.')}</p><dl class="anbud-detail-metadata"><dt>{tr('Erstatter kunngjøring', 'Replaces notice')}</dt><dd>{change().replaces || '—'}</dd><dt>{tr('Årsakskode', 'Reason code')}</dt><dd>{change().reasonCode || '—'}</dd><dt>{tr('Endrede seksjoner', 'Changed sections')}</dt><dd>{change().sections.join(', ') || '—'}</dd></dl></>}</Show></section>
          </Match>
          <Match when={anbudDetail.section() === 'assessment'}>
            <section class="anbud-detail-panel"><h2>{tr('Vurdering mot kundeprofil', 'Company profile assessment')}</h2><Show when={anbudState.submitted()} fallback={<><p>{tr('Velg kundeprofil for å vurdere kvalifisering og relevans.', 'Choose a company profile to assess qualification and relevance.')}</p><button type="button" class="anbud-button" onClick={() => anbudWorkspace.setProfileOpen(true)}>{tr('Sett opp kundeprofil', 'Set up company profile')}</button></>}><TenderAuditPanel audit={assessment()} busy={assessmentBusy()} error={assessmentError()} onClose={() => anbudDetail.setSection('brief')} /></Show></section>
          </Match>
          <Match when={anbudDetail.section() === 'source'}>
            <section class="anbud-detail-panel"><header><h2>{tr('Kilde og sporbarhet', 'Source and provenance')}</h2><ShieldCheck size={18} /></header><dl class="anbud-detail-metadata"><dt>{tr('Kilde', 'Source')}</dt><dd>Doffin</dd><dt>{tr('Kunngjørings-ID', 'Notice ID')}</dt><dd>{data().evidence.noticeId}</dd><dt>{tr('Hentet', 'Fetched')}</dt><dd>{date(data().evidence.fetchedAt)}</dd><dt>SHA-256</dt><dd><code>{data().evidence.sha256}</code></dd><dt>{tr('Kildestørrelse', 'Source size')}</dt><dd>{data().evidence.bytes.toLocaleString()} bytes</dd></dl><Show when={item().metadataAvailable === false}><p>{tr('Kunngjøringen er lagret, men XML-formatet støttes ikke fullt ut ennå.', 'The notice is stored, but its XML format is not fully supported yet.')}</p></Show></section>
          </Match>
        </Switch>
      </>}</Show>
    </>}</Show>
  </main></div>
}
