import { SEOEvidencePanel } from '@/features/visibility/components/SEOEvidencePanel'
import { For, Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadSiteAudit, startSiteAudit, type AuditFinding, type SiteAudit } from '@/shared/api/marketing/audit-client'
import { saveMarketingAuditFinding } from '@/shared/api/marketing/marketing-work-client'
import type { DiscoveryContext } from '@/shared/api/marketing/discovery-client'
import { auditRuleLabel, auditStatusLabel } from './audit-work-labels'
import './site-audit-panel.css'

type Props = { websiteId: string; site: string; revision: number; onWorkSaved: () => void; onSettled?: () => void }

/** Explicit bounded runs, retained evidence and saved tasks—not a fabricated health score. */
export function SiteAuditPanel(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [snapshot, setSnapshot] = createSignal<SiteAudit | null>(null)
  const [busy, setBusy] = createSignal(false), [saving, setSaving] = createSignal('')
  const [error, setError] = createSignal(''), [notice, setNotice] = createSignal('')
  let generation = 0, deadline = 0
  let controller: AbortController | undefined, writeController: AbortController | undefined
  let timer: ReturnType<typeof setTimeout> | undefined
  let watchedAudit = '', notifiedAudits: ReadonlySet<string> = new Set()
  const context = (): DiscoveryContext => ({ websiteId: props.websiteId, site: props.site, websiteRevision: props.revision })
  const clearTimer = () => { if (timer !== undefined) clearTimeout(timer); timer = undefined }
  const stamp = (value: string) => value ? new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value)) : '—'
  async function request(start: boolean, selected: DiscoveryContext, version: number, auditId?: string) {
    clearTimer(); controller?.abort()
    const current = new AbortController(); controller = current
    setBusy(true); setError('')
    try {
      const value = await (start ? startSiteAudit(selected, current.signal) : loadSiteAudit(selected, current.signal, auditId))
      if (current.signal.aborted || version !== generation) return
      setSnapshot(value)
      if (value.status === 'running') {
        watchedAudit = value.id
        if (Date.now() + 3000 < deadline) timer = setTimeout(() => { void request(false, selected, version, value.id) }, 3000)
        else setNotice(i18n.tr('Statuslesing er stoppet. Oppdater for å se lagret resultat; ingen ny revisjon startes.', 'Status polling stopped. Refresh to read the saved result; no new audit is started.'))
      } else if (value.id !== '' && (value.status === 'complete' || value.status === 'failed') && (start || watchedAudit === value.id) && !notifiedAudits.has(value.id)) {
        notifiedAudits = new Set([...notifiedAudits, value.id])
        watchedAudit = ''
        props.onSettled?.()
      }
    } catch {
      if (!current.signal.aborted && version === generation) setError(i18n.tr('Revisjonen kunne ikke lastes. Oppdater status før du starter på nytt.', 'Audit could not be loaded. Refresh status before starting again.'))
    } finally { if (!current.signal.aborted && version === generation) setBusy(false) }
  }
  createEffect(() => {
    const selected = context(), version = ++generation
    clearTimer(); controller?.abort(); writeController?.abort()
    watchedAudit = ''; notifiedAudits = new Set()
    setSnapshot(null); setSaving(''); setNotice(''); setError(''); deadline = Date.now() + 910_000
    void request(false, selected, version)
    onCleanup(() => { clearTimer(); controller?.abort(); writeController?.abort() })
  })
  function refresh(start: boolean, auditId?: string) {
    if (busy() || saving()) return
    setNotice(''); deadline = Date.now() + 910_000
    void request(start, context(), generation, auditId)
  }
  async function save(finding: AuditFinding) {
    const audit = snapshot(); if (!audit || saving() || busy()) return
    const version = generation, selected = context()
    const current = new AbortController(); writeController = current
    setSaving(finding.id); setError(''); setNotice('')
    try {
      await saveMarketingAuditFinding({ ...selected, auditId: audit.id, findingId: finding.id }, current.signal)
      if (!current.signal.aborted && version === generation) { props.onWorkSaved(); setNotice(i18n.tr('Funnet er lagret i arbeidskøen med kildebevis.', 'Finding saved to the work queue with source evidence.')) }
    } catch { if (!current.signal.aborted && version === generation) setError(i18n.tr('Lagringen kunne ikke bekreftes. Last arbeidskøen på nytt før du prøver igjen.', 'The save result could not be confirmed. Refresh the work queue before retrying.')) }
    finally { if (!current.signal.aborted && version === generation) setSaving('') }
  }
  return <section class="site-audit-panel" aria-labelledby={`${id}-title`} data-testid="site-audit-panel">
    <header><div><span>Quarry · Visibility Core</span><h2 id={`${id}-title`}>{i18n.tr('Nettstedsrevisjon', 'Site audit')}</h2><p>{i18n.tr('Inntil 100 offentlige sider per kjøring. Bevis først, tiltak etterpå.', 'Up to 100 public pages per run. Evidence first, actions next.')}</p></div>
      <div><button type="button" disabled={busy() || Boolean(saving())} onClick={() => refresh(false)}>{i18n.tr('Oppdater status', 'Refresh audit status')}</button><button type="button" disabled={busy() || Boolean(saving()) || snapshot()?.status === 'running'} onClick={() => refresh(true)}>{i18n.tr('Kjør nettstedsrevisjon', 'Run site audit')}</button></div>
    </header>
    <Show when={error()}><p role="alert">{error()}</p></Show><Show when={notice()}><p role="status">{notice()}</p></Show>
    <Show when={snapshot()}>{(audit) => <>
      <p class="site-audit-panel__status" role="status">{audit().status === 'idle' ? i18n.tr('Ingen lagret revisjon ennå.', 'No saved audit yet.') : audit().status === 'running' ? i18n.tr('Revisjon pågår. Statuslesing starter ikke nye sidehentinger.', 'Audit running. Status reads do not start additional captures.') : `${audit().pages.filter((page) => page.status === 'checked').length}/${audit().pages.length} ${i18n.tr('sider kontrollert', 'pages checked')} · ${audit().findings.length} ${i18n.tr('funn', 'findings')} · ${stamp(audit().completedAt)}`}</p>
      <Show when={audit().coverage}>{coverage=><p class="site-audit-panel__status">{coverage().attempted}/{coverage().discovered} {i18n.tr('kjente adresser forsøkt', 'known URLs attempted')} · {coverage().pending} {i18n.tr('ikke kontrollert', 'not checked')}<Show when={coverage().sitemap}>{s=><> · {s().sitemapValid}/{s().sitemapFiles} {i18n.tr('nettstedskart lest','sitemaps read')}{s().sitemapPartial?` · ${i18n.tr('ufullstendig kartdekning','incomplete sitemap coverage')}`:''}</>}</Show><Show when={coverage().limitReached}> · {i18n.tr('Kjøregrensen er nådd.', 'Run limit reached.')}</Show><Show when={coverage().extractionPartial}> · {i18n.tr('Noe sidebevis er avkortet.', 'Some page evidence is truncated.')}</Show></p>}</Show>
      <Show when={audit().reason}><p>{i18n.tr('Kildestatus', 'Source status')}: {audit().reason}. {i18n.tr('Manglende bevis er ikke et bestått resultat.', 'Missing evidence is not a passing result.')}</p></Show>
      <Show when={audit().pages.length > 0}><details><summary>{i18n.tr('Kontrollerte sider og dekning', 'Pages and check coverage')}</summary><For each={audit().pages}>{(page) => <div class="site-audit-panel__page"><strong>{page.url}</strong><span>{page.status === 'checked' ? `HTTP ${page.httpStatus}` : `${i18n.tr('Ikke kontrollert', 'Not checked')} · ${page.reason ?? 'unavailable'}`}</span><For each={page.checks}>{(check) => <small>{auditRuleLabel(check.ruleId, i18n.tr)}: {auditStatusLabel(check.status, i18n.tr)}</small>}</For><Show when={page.seo}>{seo=><SEOEvidencePanel evidence={seo()}/>}</Show></div>}</For></details></Show>
      <div class="site-audit-panel__findings"><For each={audit().findings}>{(finding) => <article><div><strong>{auditRuleLabel(finding.ruleId, i18n.tr)}</strong><p>{finding.pageUrl}</p><small>{finding.severity} · {stamp(finding.observedAt)}</small><Show when={finding.relatedUrls.length}><p>{finding.relatedUrls.join(' · ')}</p></Show></div><button type="button" disabled={busy() || Boolean(saving())} onClick={() => void save(finding)}>{saving() === finding.id ? i18n.tr('Lagrer…', 'Saving…') : i18n.tr('Lagre som oppgave', 'Save as task')}</button></article>}</For></div>
      <Show when={audit().history.length}><details><summary>{i18n.tr('Lagrede kjøringer', 'Saved runs')}</summary><For each={audit().history}>{(receipt) => <button type="button" class="site-audit-panel__receipt" disabled={busy() || Boolean(saving())} onClick={() => refresh(false, receipt.id)}>{stamp(receipt.completedAt || receipt.startedAt)} · {receipt.pageCount} {i18n.tr('sider', 'pages')} · {receipt.findingCount} {i18n.tr('funn', 'findings')}</button>}</For></details></Show>
    </>}</Show>
    <footer>{i18n.tr('Dette er et begrenset utvalg, ikke en fullstendig revisjon eller et mål på nettstedets totale helse. Metadata er observerte signaler. Brutte lenker kan bare bekreftes mot sider som faktisk er kontrollert. Rå HTML lagres ikke i Marketing Core.', 'This is a bounded sample, not an exhaustive audit or a whole-site health score. Metadata are observed signals. Broken links can only be confirmed against pages actually checked. Raw HTML is not stored in Marketing Core.')}</footer>
  </section>
}
