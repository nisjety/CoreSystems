import { For, Show } from 'solid-js'
import { ArrowLeft, FileText, Layers, LayoutGrid, ShieldCheck } from 'lucide-solid'
import { anbudDetail, anbudWorkspace, type TenderDetailSection } from '@/features/anbud/lib/anbud-state'
import { anbudStages } from '@/features/anbud/lib/pipeline'
import { noticeTypeLabel, sourceName } from '@/features/anbud/lib/notice-presentation'
import { useI18n } from '@/shared/i18n'
import { setPipelineStage } from '@/features/anbud/lib/workspace-sync'

export function TenderDetailSidebar() {
  const { tr } = useI18n()
  const notice = () => anbudDetail.data()?.result ?? anbudDetail.summary()
  const sections = () => [
    { id: 'brief' as TenderDetailSection, no: 'Brief', en: 'Brief', icon: LayoutGrid },
    { id: 'documents' as TenderDetailSection, no: 'Konkurransegrunnlag', en: 'Documents', icon: FileText, count: anbudDetail.data()?.notice.documentsTotal },
    { id: 'requirements' as TenderDetailSection, no: 'Kvalifikasjonskrav', en: 'Requirements', icon: ShieldCheck },
    { id: 'lots' as TenderDetailSection, no: 'Delkontrakter', en: 'Lots', icon: Layers, count: anbudDetail.data()?.notice.lotsTotal },
    { id: 'evaluation' as TenderDetailSection, no: 'Tildelingskriterier', en: 'Evaluation criteria', icon: ShieldCheck },
    { id: 'awards' as TenderDetailSection, no: 'Tildeling og kontrakt', en: 'Award and contract', icon: FileText, count: anbudDetail.data()?.notice.awards?.length },
    { id: 'changes' as TenderDetailSection, no: 'Endringer', en: 'Amendments', icon: FileText },
    { id: 'assessment' as TenderDetailSection, no: 'Vurdering mot kundeprofil', en: 'Company profile assessment', icon: ShieldCheck },
    { id: 'source' as TenderDetailSection, no: 'Kilde og sporbarhet', en: 'Source and provenance', icon: ShieldCheck },
  ]
  const dates = () => [
    { label: tr('Publisert', 'Published'), value: notice()?.publishedDate },
    { label: tr('Utstedt', 'Issued'), value: notice()?.issuedAt },
    { label: tr('Tilbudsfrist', 'Tender deadline'), value: notice()?.deadline?.at },
    { label: tr('Planlagt utlysning', 'Call planned'), value: notice()?.plannedNotice },
    { label: tr(`Hentet fra ${sourceName(anbudDetail.data()?.evidence.source)}`, `Fetched from ${sourceName(anbudDetail.data()?.evidence.source)}`), value: anbudDetail.data()?.evidence.fetchedAt },
  ].filter((item): item is { label: string; value: string } => !!item.value).sort((a, b) => Date.parse(a.value) - Date.parse(b.value))
  return <div class="anbud-detail-sidebar-content"><button type="button" class="anbud-detail-back" onClick={anbudDetail.close}><ArrowLeft size={15} />{tr('Tilbake til anbud', 'Back to tenders')}</button><Show when={notice()}>{item => <>
    <div class="anbud-sidebar-notice"><strong>{item().buyer ?? tr('Innkjøper ikke oppgitt', 'Buyer not stated')}</strong><p>{item().title}</p><small>{noticeTypeLabel(item(), tr)}</small><label>{tr('Pipeline-status', 'Pipeline stage')}<select value={anbudWorkspace.stages()[item().noticeId] ?? 'inbox'} onChange={e => setPipelineStage(item().noticeId, e.currentTarget.value)}><For each={anbudStages}>{stage => <option value={stage.id}>{tr(stage.no, stage.en)}</option>}</For></select></label></div>
    <nav aria-label={tr('Anbudinnhold', 'Tender content')}><For each={sections()}>{section => <button type="button" class="anbud-stage" aria-current={anbudDetail.section() === section.id ? 'page' : undefined} onClick={() => anbudDetail.setSection(section.id)}><section.icon size={16} /><span>{tr(section.no, section.en)}</span><Show when={section.count !== undefined}><span class="anbud-count">{section.count}</span></Show></button>}</For></nav>
    <section class="anbud-notice-timeline"><h3>{tr('Datoer', 'Dates')}</h3><For each={dates()}>{event => <div><span>{event.label}</span><time>{event.value.slice(0, 10)}</time></div>}</For></section>
    <p class="anbud-filter-note">{tr('Pipeline-status lagres for organisasjonen.', 'Pipeline stages are saved for your organisation.')}</p>
  </>}</Show></div>
}
