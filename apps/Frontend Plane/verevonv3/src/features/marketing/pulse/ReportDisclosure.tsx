import { ChevronDown } from 'lucide-solid'
import type { JSX } from 'solid-js'

/** Native disclosure keeps reports mounted and keyboard controls predictable. */
export function ReportDisclosure(props: { title: string; source: string; children: JSX.Element; ref?: (element: HTMLDetailsElement) => void; testId?: string }) {
  return <details ref={element => props.ref?.(element)} class="pulse-report-disclosure" data-testid={props.testId}>
    <summary><span><small>{props.source}</small><strong>{props.title}</strong></span><ChevronDown size={18} aria-hidden="true"/></summary>
    <div class="pulse-report-disclosure__body">{props.children}</div>
  </details>
}
