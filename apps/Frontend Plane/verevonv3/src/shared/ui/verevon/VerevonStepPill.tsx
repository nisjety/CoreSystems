export function VerevonStepPill(props: { current: number; total: number; label?: string; ofLabel?: string }) {
  return <span class="onboarding-step-pill">{props.label ?? 'Steg'} {props.current} {props.ofLabel ?? 'av'} {props.total}</span>
}
