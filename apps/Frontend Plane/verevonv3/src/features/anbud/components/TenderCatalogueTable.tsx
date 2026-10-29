import { ArrowDown, ArrowUp, ChevronsUpDown } from 'lucide-solid'
import { For, Show } from 'solid-js'
import type { TenderMatch } from '@/shared/api/anbud-client'
import { useI18n } from '@/shared/i18n'
import { arrangementLabel, durationLabel, endDateLabel, winnersLabel, formatTenderMoney, noticeStatus, noticeTypeLabel, plannedNoticeLabel, tenderColumns, type TenderColumn, type TenderColumnVisibility } from '@/features/anbud/lib/notice-presentation'

export function TenderCatalogueTable(props: { rows: TenderMatch[]; columns: TenderColumnVisibility; onSelect: (id: string) => void; sort?: string; direction?: string; onSort?: (key: string) => void }) {
  const { tr } = useI18n()
  const cell = (notice: TenderMatch, key: TenderColumn) => {
    switch (key) {
      case 'buyer': return notice.buyer ?? '—'
      case 'noticeType': return <span class="anbud-table-tag">{noticeTypeLabel(notice, tr)}</span>
      case 'arrangement': return arrangementLabel(notice, tr)
      case 'status': return <span class={`anbud-notice-status anbud-notice-status--${noticeStatus(notice, tr).value}`}>{noticeStatus(notice, tr).label}</span>
      case 'estimatedValue': return formatTenderMoney(notice.estimatedValue)
      case 'awardedValue': return formatTenderMoney(notice.awardedValue)
      case 'deadline': return notice.deadline?.osloDay ?? plannedNoticeLabel(notice, tr) ?? '—'
      case 'published': return notice.publishedDate ?? '—'
      case 'winners': return winnersLabel(notice, tr)
      case 'duration': return durationLabel(notice, tr)
      case 'endDate': return endDateLabel(notice, tr)
      case 'issued': return notice.issuedAt?.slice(0, 10) ?? '—'
      case 'match': return notice.verdict === 'unknown' ? tr('Ikke vurdert', 'Not assessed') : notice.band === 'strong' ? tr('Sterk match', 'Strong match') : notice.band === 'good' ? tr('God match', 'Good match') : tr('Vurder match', 'Review match')
    }
  }
  const header = (key: string, label: string) => <button type="button" class="anbud-column-sort" aria-label={tr(`Sorter etter ${label}`, `Sort by ${label}`)} onClick={() => props.onSort?.(key)}>{label}<Show when={props.sort === key} fallback={<ChevronsUpDown size={13} />}><Show when={props.direction === 'desc'} fallback={<ArrowUp size={13} />}><ArrowDown size={13} /></Show></Show></button>
  const sortState = (key: string): 'ascending' | 'descending' | 'none' => props.sort === key ? props.direction === 'desc' ? 'descending' : 'ascending' : 'none'
  return <div class="anbud-table-wrap anbud-catalogue-table" tabIndex={0} role="region" aria-label={tr('Kunngjøringer – rull vannrett for flere kolonner', 'Notices – scroll horizontally for more columns')}>
    <table class="anbud-table"><thead><tr><th scope="col" aria-label={tr('Tittel', 'Title')} aria-sort={sortState('title')}>{header('title', tr('Tittel', 'Title'))}</th><For each={tenderColumns}>{column => <Show when={props.columns[column.id]}><th scope="col" aria-label={tr(column.no, column.en)} aria-sort={sortState(column.id)}>{header(column.id, tr(column.no, column.en))}</th></Show>}</For></tr></thead>
      <tbody><For each={props.rows}>{notice => <tr><td><button type="button" title={notice.title} onClick={() => props.onSelect(notice.noticeId)}>{notice.title}</button></td><For each={tenderColumns}>{column => <Show when={props.columns[column.id]}><td>{cell(notice, column.id)}</td></Show>}</For></tr>}</For></tbody>
    </table>
  </div>
}
