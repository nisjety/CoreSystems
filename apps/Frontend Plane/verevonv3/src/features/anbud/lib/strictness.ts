import type { TenderStrictness } from '@/shared/api/anbud-client'

/** The slider's steps, from the closest reach to the widest. */
export const strictnessSteps: readonly TenderStrictness[] = ['strict', 'balanced', 'wide']

/** The steps wider than `step`, in order. */
export function widerSteps(step: TenderStrictness): TenderStrictness[] {
  return strictnessSteps.slice(strictnessSteps.indexOf(step) + 1)
}

type Translate = (noText: string, enText: string) => string

export function strictnessLabel(step: TenderStrictness, tr: Translate): string {
  const labels: Record<TenderStrictness, string> = {
    strict: tr('Strikt', 'Strict'),
    balanced: tr('Balansert', 'Balanced'),
    wide: tr('Bredt', 'Wide'),
  }
  return labels[step]
}
