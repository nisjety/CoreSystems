import { TaskPlanningEditor } from './TaskPlanningEditor'
import { planMarketingTask, type TaskPlanning } from '@/shared/api/marketing/marketing-work-client'
import { Activity, CircleDashed, CircleDot, LoaderCircle } from 'lucide-solid'
import { For, Show, createEffect, createSignal, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadMarketingWork, updateMarketingTask, verifyMarketingTask, type MarketingTask, type MarketingTaskStatus, type MarketingVerification, type MarketingWorkSnapshot } from '../../../shared/api/marketing/marketing-work-client.ts'
import { MarketingTaskEvidence } from './MarketingTaskEvidence.tsx'
import { technicalCheckLabel, technicalStatusLabel } from './technical-work-labels.ts'
import { recheckTechnicalTask } from './technical-work-recheck.ts'
import { auditRuleLabel, auditStatusLabel } from './audit-work-labels.ts'
import './marketing-work-queue.css'

type Props = { canWrite?:boolean; websiteId: string; site: string; revision: number; refreshKey: number; view: 'calendar' | 'board'; onSnapshot: (snapshot: MarketingWorkSnapshot | null) => void; onTechnicalRechecked?: () => void }

export function MarketingWorkQueue(props: Props) {
  const i18n = useI18n()
  const [snapshot, setSnapshot] = createSignal<MarketingWorkSnapshot | null>(null)
  const [loading, setLoading] = createSignal(true)
  const [failed, setFailed] = createSignal(false)
  const [busy, setBusy] = createSignal<ReadonlySet<string>>(new Set())
  const [error, setError] = createSignal('')
  const [localRefresh, setLocalRefresh] = createSignal(0)
  const [showClosed, setShowClosed] = createSignal(false)
  const [activityLimit, setActivityLimit] = createSignal(20)
  let writeController: AbortController | undefined

  createEffect(() => {
    const websiteId = props.websiteId; const site = props.site; const revision = props.revision; const refreshKey = props.refreshKey
    const onSnapshot = props.onSnapshot
    void revision
    void refreshKey
    localRefresh()
    const controller = new AbortController()
    setSnapshot(null); onSnapshot(null); setLoading(true); setFailed(false); setError(''); setActivityLimit(20)
    writeController?.abort(); setBusy(new Set<string>())
    void loadMarketingWork(websiteId, site, controller.signal).then((value) => {
      if (!controller.signal.aborted && value.websiteId === websiteId && value.site === site) { setSnapshot(value); onSnapshot(value) }
    }).catch(() => { if (!controller.signal.aborted) { setFailed(true); onSnapshot(null) } })
      .finally(() => { if (!controller.signal.aborted) setLoading(false) })
    onCleanup(() => { controller.abort(); writeController?.abort() })
  })

  const visibleTasks = () => snapshot()?.tasks.filter((task) => showClosed() || task.status === 'todo' || task.status === 'in_progress') ?? []
  const prioritizedTasks=()=>[...visibleTasks()].sort((a,b)=>{const ranks={urgent:0,high:1,normal:2,low:3};return ranks[a.planning?.priority??'normal']-ranks[b.planning?.priority??'normal']||(a.planning?.dueDate||'9999').localeCompare(b.planning?.dueDate||'9999')})
  const trStatus = (status: MarketingTaskStatus) => ({ todo: i18n.tr('Åpen', 'To do'), in_progress: i18n.tr('Pågår', 'In progress'), done: i18n.tr('Fullført', 'Done'), dismissed: i18n.tr('Avvist', 'Dismissed') })[status]
  const taskTitle = (task: MarketingTask) => task.evidence.kind==='search_group_review'?`${task.evidence.review.rule.name} · ${task.evidence.key}`: task.evidence.kind === 'pagespeed_check' ? technicalCheckLabel(task.evidence.key, i18n.tr) : task.evidence.kind === 'site_audit_finding' ? auditRuleLabel(task.evidence.key, i18n.tr) : task.evidence.key
  const verificationLabel = (verification: MarketingVerification) => 'auditId' in verification || 'review' in verification ? auditStatusLabel(verification.status, i18n.tr) : technicalStatusLabel(verification.status, i18n.tr)
  const stamp = (value: string) => new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value))

  async function plan(task:MarketingTask,planning:TaskPlanning){if(busy().size>0||props.canWrite===false)return;const key=`${props.websiteId}|${props.site}|${props.revision}`;const controller=new AbortController();writeController=controller;setBusy(new Set([task.id]));setError('');try{const value=await planMarketingTask({websiteId:props.websiteId,site:props.site,websiteRevision:props.revision,taskId:task.id,revision:task.revision},planning,controller.signal);if(!controller.signal.aborted&&key===`${props.websiteId}|${props.site}|${props.revision}`){setSnapshot(value);props.onSnapshot(value)}}catch{if(!controller.signal.aborted)setError(i18n.tr('Planleggingen kunne ikke lagres. Kontroller at ansvarlig fortsatt er medlem, og last arbeidskøen på nytt.','Could not save planning. Check current membership and reload the work queue.'))}finally{if(writeController===controller)setBusy(new Set<string>())}}
  async function changeStatus(task: MarketingTask, status: MarketingTaskStatus) {
    if (busy().size > 0) return
    const contextKey = `${props.websiteId}|${props.site}|${props.revision}`
    const controller = new AbortController(); writeController = controller
    setBusy((current) => new Set(current).add(task.id)); setError('')
    try {
      const value = await updateMarketingTask({ taskId: task.id, websiteId: props.websiteId, site: props.site, websiteRevision: props.revision, revision: task.revision, status }, controller.signal)
      if (!controller.signal.aborted && `${props.websiteId}|${props.site}|${props.revision}` === contextKey && value.websiteId === props.websiteId && value.site === props.site) { setSnapshot(value); props.onSnapshot(value) }
    } catch { if (!controller.signal.aborted && `${props.websiteId}|${props.site}|${props.revision}` === contextKey) setError(i18n.tr('Oppgaven ble ikke oppdatert. Last inn på nytt og prøv igjen.', 'Task was not updated. Refresh and try again.')) }
    finally { if (writeController === controller) setBusy(new Set<string>()) }
  }

  async function recheck(task: MarketingTask) {
    if (busy().size > 0 || task.evidence.kind === 'gsc_opportunity') return
    const controller = new AbortController(); writeController = controller
    const contextKey = `${props.websiteId}|${props.site}|${props.revision}`
    setBusy(new Set([task.id])); setError('')
    try {
      const value = (task.evidence.kind === 'site_audit_finding'||task.evidence.kind==='search_group_review')
        ? await verifyMarketingTask({ taskId: task.id, websiteId: props.websiteId, site: props.site, websiteRevision: props.revision, revision: task.revision }, controller.signal)
        : await recheckTechnicalTask(task, props.revision, controller.signal)
      if (!controller.signal.aborted && `${props.websiteId}|${props.site}|${props.revision}` === contextKey) { setSnapshot(value); props.onSnapshot(value); if (task.evidence.kind === 'pagespeed_check') props.onTechnicalRechecked?.() }
    } catch (cause) {
      if (!controller.signal.aborted && `${props.websiteId}|${props.site}|${props.revision}` === contextKey) setError(cause instanceof Error && cause.message === 'work_conflict'
        ? task.evidence.kind === 'search_group_review'
          ? i18n.tr('Ingen nyere søkerapport eller oppgaven er endret. Synkroniser søkehistorikken i SEO-grupper og last arbeidskøen på nytt før du kontrollerer igjen.', 'No newer search report is available or the task changed. Sync search history in SEO groups and reload the work queue before checking again.')
          : task.evidence.kind === 'site_audit_finding'
          ? i18n.tr('Ingen nyere fullført revisjon eller oppgaven er endret. Last arbeidskøen på nytt og kjør nettstedsrevisjonen før du bekrefter kontrollen.', 'No newer completed audit is available or the task changed. Reload the work queue and run the website audit before confirming the check.')
          : i18n.tr('Ingen nyere måling eller oppgaven er endret. Ingen ny kontroll ble lagret. Last arbeidskøen på nytt; vent før du starter en ny måling.', 'No newer audit is available or the task changed. No verification was saved. Reload the work queue; wait before starting another audit.')
        : i18n.tr('Kontrollresultatet kunne ikke bekreftes. Last arbeidskøen på nytt for å se om et resultat ble lagret før du prøver igjen.', 'The recheck result could not be confirmed. Reload the work queue to see whether a result was saved before trying again.'))
    } finally { if (writeController === controller) setBusy(new Set<string>()) }
  }

  return <>
    <Show when={props.view === 'calendar'}>
      <div class="marketing-pulse__breakdown-grid marketing-pulse__breakdown-grid--calendar">
        <Show when={prioritizedTasks().some(t=>t.planning?.dueDate)} fallback={<div class="marketing-pulse__calendar-empty" data-testid="pulse-calendar-empty"><CircleDashed size={20}/><div><strong>{i18n.tr('Ingen datoer å vise ennå','No dates to show yet')}</strong><p>{i18n.tr('Legg til en forfallsdato i oppgavens planlegging.','Add a due date in task planning.')}</p></div></div>}><For each={prioritizedTasks().filter(t=>t.planning?.dueDate).sort((a,b)=>a.planning!.dueDate.localeCompare(b.planning!.dueDate))}>{t=><article class="marketing-work__task"><time dateTime={t.planning!.dueDate}>{t.planning!.dueDate}</time><strong>{taskTitle(t)}</strong><span>{trStatus(t.status)}</span><Show when={props.canWrite!==false}><TaskPlanningEditor task={t} busy={busy().size>0} onSave={p=>void plan(t,p)}/></Show></article>}</For></Show>
      </div>
    </Show>
    <Show when={props.view === 'board'}>
      <div class="marketing-pulse__breakdown-grid" data-workspace-view="board">
        <section class="marketing-pulse__ledger marketing-pulse__ledger--backlog" aria-labelledby="marketing-pulse-backlog-title">
          <div class="marketing-pulse__ledger-heading"><div><CircleDot size={18} strokeWidth={1.8} /><h3 id="marketing-pulse-backlog-title">{i18n.tr('Oppgaveliste', 'Backlog')}</h3><strong data-testid="pulse-backlog-count">{snapshot()?.summary.open ?? (failed() ? '—' : '…')}</strong></div><label><input type="checkbox" checked={showClosed()} onChange={(event) => setShowClosed(event.currentTarget.checked)} />{i18n.tr('Vis avsluttede', 'Show closed')}</label></div>
          <Show when={!loading()} fallback={<div class="marketing-work__state" role="status"><LoaderCircle size={16} class="marketing-work__spin" />{i18n.tr('Laster arbeidskø…', 'Loading work queue…')}</div>}>
            <Show when={!failed()} fallback={<p class="marketing-work__state" role="alert">{i18n.tr('Arbeidskøen kunne ikke lastes.', 'Work queue could not be loaded.')}</p>}>
              <Show when={visibleTasks().length > 0} fallback={<div class="marketing-pulse__ledger-row" data-testid="pulse-backlog-empty-row"><span class="marketing-pulse__ledger-row-marker"><CircleDot size={18} /></span><div><strong>{i18n.tr('Ingen prioriterte oppgaver ennå', 'No prioritized work yet')}</strong><p>{i18n.tr('Lagre et konkret forslag fra Søkeinnsikt for å legge det til her.', 'Save a specific suggestion from Search intelligence to add it here.')}</p></div></div>}>
                <For each={prioritizedTasks()}>{(task) => <article class="marketing-work__task" data-testid="marketing-work-task">
                  <span class="marketing-pulse__ledger-row-marker"><CircleDot size={18} /></span>
                  <div class="marketing-work__task-body"><div class="marketing-work__task-title"><strong>{taskTitle(task)}</strong><span data-status={task.status}>{trStatus(task.status)}</span></div><MarketingTaskEvidence task={task} /><Show when={props.canWrite!==false}><TaskPlanningEditor task={task} busy={busy().size>0} onSave={p=>void plan(task,p)}/></Show>
                    <Show when={task.evidence.kind === 'pagespeed_check'}><div class="marketing-work__actions"><button type="button" disabled={busy().size > 0} onClick={() => void recheck(task)}>{busy().has(task.id) ? i18n.tr('Arbeider…', 'Working…') : i18n.tr('Kontroller på nytt', 'Recheck')}</button><small>{i18n.tr('Starter en labmåling. Oppgavestatus endres ikke automatisk.', 'Starts a lab audit. Task status is not changed automatically.')}</small></div></Show>
                    <Show when={task.evidence.kind === 'site_audit_finding'||task.evidence.kind==='search_group_review'}><div class="marketing-work__actions"><button type="button" disabled={busy().size > 0} onClick={() => void recheck(task)}>{busy().has(task.id) ? i18n.tr('Arbeider…', 'Working…') : task.evidence.kind==='search_group_review'?i18n.tr('Kontroller mot nyere søkerapport','Check newer search report'):i18n.tr('Bekreft siste revisjonskontroll', 'Confirm latest audit check')}</button><small>{task.evidence.kind==='search_group_review'?i18n.tr('Synkroniser søkehistorikken først. Den opprinnelige regelen kontrolleres i en nyere rapport; manglende rader er uavklart.','Sync search history first. The original rule is checked against a newer report; missing rows remain inconclusive.'):i18n.tr('Kjør en ny nettstedsrevisjon i panelet over først. Dette lagrer bare den siste fullførte kontrollen; ingen ny innhenting startes.', 'Run a new website audit in the panel above first. This only saves the latest completed check; no new capture is started.')}</small></div></Show>
                    <div class="marketing-work__actions"><Show when={task.status === 'todo'}><button type="button" disabled={busy().size > 0} onClick={() => void changeStatus(task, 'in_progress')}>{i18n.tr('Start', 'Start')}</button></Show><Show when={task.status !== 'todo'}><button type="button" disabled={busy().size > 0} onClick={() => void changeStatus(task, 'todo')}>{i18n.tr('Tilbake til åpen', 'Back to open')}</button></Show><Show when={task.status !== 'done'}><button type="button" disabled={busy().size > 0} onClick={() => void changeStatus(task, 'done')}>{i18n.tr('Fullfør', 'Complete')}</button></Show><Show when={task.status !== 'dismissed'}><button type="button" disabled={busy().size > 0} onClick={() => void changeStatus(task, 'dismissed')}>{i18n.tr('Avvis', 'Dismiss')}</button></Show></div>
                  </div>
                </article>}</For>
              </Show>
            </Show>
          </Show>
        </section>
        <section class="marketing-pulse__ledger" aria-labelledby="marketing-pulse-log-title">
          <div class="marketing-pulse__ledger-heading"><div><Activity size={18} strokeWidth={1.8} /><h3 id="marketing-pulse-log-title">{i18n.tr('Aktivitetslogg', 'Activity log')}</h3><strong data-testid="pulse-log-count">{snapshot()?.activity.length ?? (failed() ? '—' : '…')}</strong></div><span>{i18n.tr('Siste 1 000 oppføringer', 'Latest 1,000 entries')}</span></div>
          <Show when={!loading()} fallback={<div class="marketing-work__state" role="status">{i18n.tr('Laster aktivitet…', 'Loading activity…')}</div>}>
            <Show when={!failed()} fallback={<p class="marketing-work__state" role="alert">{i18n.tr('Aktivitetsloggen kunne ikke lastes.', 'Activity log could not be loaded.')}</p>}>
              <Show when={(snapshot()?.activity.length ?? 0) > 0} fallback={<div class="marketing-pulse__ledger-row"><span class="marketing-pulse__ledger-row-marker"><CircleDashed size={18} /></span><div><strong>{i18n.tr('Ingen aktivitet registrert', 'No activity recorded')}</strong><p>{i18n.tr('Opprettede oppgaver og statusendringer vises her.', 'Saved tasks and status changes appear here.')}</p></div></div>}>
                <For each={snapshot()?.activity.slice(0, activityLimit()) ?? []}>{(entry) => {
                  const task = snapshot()?.tasks.find((item) => item.id === entry.taskId)
                  return <div class="marketing-work__activity"><span class="marketing-pulse__ledger-row-marker"><CircleDashed size={16} /></span><div><strong>{entry.kind === 'created' ? i18n.tr('Lagt til i oppgavelisten', 'Added to backlog') : entry.kind === 'verified' ? i18n.tr('Kontroll lagret', 'Verification saved') : entry.kind==='planning_changed'?i18n.tr('Planlegging endret','Planning changed'): i18n.tr('Status endret', 'Status changed')}{task ? ` · ${taskTitle(task)}` : ''}</strong><p>{trStatus(entry.status)} · {stamp(entry.occurredAt)}</p><Show when={entry.verification}>{(verification) => <p>{verificationLabel(verification())} · {i18n.tr('målt', 'measured')} {stamp(verification().observedAt)}</p>}</Show></div></div>
                }}</For>
                <Show when={(snapshot()?.activity.length ?? 0) > activityLimit()}><button type="button" onClick={() => setActivityLimit((value) => value + 20)}>{i18n.tr('Vis mer aktivitet', 'Show more activity')}</button></Show>
              </Show>
            </Show>
          </Show>
        </section>
      </div>
    </Show>
    <Show when={error()}><p class="marketing-work__error" role="alert">{error()}</p></Show>
    <Show when={failed() || error()}><button type="button" class="marketing-work__reload" disabled={loading() || busy().size > 0} onClick={() => setLocalRefresh((value) => value + 1)}>{i18n.tr('Last arbeidskø på nytt', 'Reload work queue')}</button></Show>
  </>
}
