import { Calendar } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import type { ChatWorkPeriod } from '@/shared/actions/chat-work-period'
import { createSignal, For } from 'solid-js'

const quickPeriods = [
  { kind: 'this_month', no: 'Denne måneden', en: 'This month' },
  { kind: 'last_month', no: 'Forrige måned', en: 'Last month' },
  { kind: 'last_quarter', no: 'Forrige kvartal', en: 'Last quarter' },
  { kind: 'year_to_date', no: 'Hittil i år', en: 'Year to date' },
  { kind: 'rolling_12', no: 'Siste 12 måneder', en: 'Last 12 months' },
] as const

export function ChatWorkPeriodPicker(props: { value: ChatWorkPeriod; onChange: (value: ChatWorkPeriod) => void }) {
  const i18n = useI18n()
  const [from, setFrom] = createSignal('')
  const [to, setTo] = createSignal('')
  let disclosure: HTMLDetailsElement | undefined
  const label = () => props.value.kind === 'none'
    ? i18n.tr('Periode', 'Period')
    : props.value.kind === 'custom'
      ? `${props.value.from} – ${props.value.to}`
      : (() => {
          const preset = quickPeriods.find((item) => item.kind === props.value.kind)
          return preset ? i18n.tr(preset.no, preset.en) : i18n.tr('Periode', 'Period')
        })()
  const choose = (value: ChatWorkPeriod) => {
    props.onChange(value)
    if (disclosure) disclosure.open = false
  }
  return (
    <details class="chat-work-period" ref={disclosure}>
      <summary aria-label={i18n.tr(`Arbeidsperiode: ${label()}`, `Working period: ${label()}`)} title={i18n.tr('Legges synlig til spørsmålet du sender', 'Added visibly to the question you send')}>
        <Calendar size={14} aria-hidden="true" />
        <span>{label()}</span>
      </summary>
      <div class="chat-work-period__menu" role="group" aria-label={i18n.tr('Velg arbeidsperiode', 'Choose working period')}>
        <p>{i18n.tr('Perioden legges til spørsmålet. Verevon sier fra hvis kildene ikke kan avgrenses.', 'The period is added to your question. Verevon will say if the sources cannot be limited.')}</p>
        <button type="button" aria-label={i18n.tr('Ingen periode', 'No period')} aria-pressed={props.value.kind === 'none' ? 'true' : 'false'} onClick={() => choose({ kind: 'none' })}>{i18n.tr('Ingen periode', 'No period')}</button>
        <For each={quickPeriods}>{(period) => (
          <button type="button" aria-label={i18n.tr(period.no, period.en)} aria-pressed={props.value.kind === period.kind ? 'true' : 'false'} onClick={() => choose({ kind: period.kind })}>{i18n.tr(period.no, period.en)}</button>
        )}</For>
        <div class="chat-work-period__custom">
          <label>{i18n.tr('Fra', 'From')}<input type="date" aria-label={i18n.tr('Fra dato', 'From date')} value={from()} onInput={(event) => setFrom(event.currentTarget.value)} /></label>
          <label>{i18n.tr('Til', 'To')}<input type="date" aria-label={i18n.tr('Til dato', 'To date')} min={from()} value={to()} onInput={(event) => setTo(event.currentTarget.value)} /></label>
          <button type="button" disabled={!from() || !to() || from() > to()} onClick={() => choose({ kind: 'custom', from: from(), to: to() })}>{i18n.tr('Bruk datoer', 'Use dates')}</button>
        </div>
      </div>
    </details>
  )
}
