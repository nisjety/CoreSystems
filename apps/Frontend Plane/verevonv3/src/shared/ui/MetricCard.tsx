import type { JSX } from 'solid-js'

export function MetricCard(props: {
  label: string
  value: string
  delta: string
  children?: JSX.Element
}) {
  return (
    <article class="metric-card">
      <p>{props.label}</p>
      <strong>{props.value}</strong>
      <span>{props.delta}</span>
      {props.children}
    </article>
  )
}
