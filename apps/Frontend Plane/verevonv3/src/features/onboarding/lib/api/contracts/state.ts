export type OnboardingStateSnapshot<TState = Record<string, unknown>> = {
  step: string
  state?: TState
}
