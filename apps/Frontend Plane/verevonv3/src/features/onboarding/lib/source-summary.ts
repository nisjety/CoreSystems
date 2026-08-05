import type { OnboardingState } from '@/features/onboarding/lib/model'

type ConnectedSource = OnboardingState['connectors'][number]

export type OnboardingSourceDetail = {
  connectorId: string
  connectorLabel: string
  sourceCount: number
  sources: string[]
  status: ConnectedSource['status']
}

export type OnboardingSourceSummary = {
  connectorCount: number
  connectedSourceCount: number
  sourceDetails: OnboardingSourceDetail[]
  totalSourceCount: number
  websiteSourceCount: number
}

export function summarizeOnboardingSources(input: {
  connectors: readonly ConnectedSource[]
  websiteUrl?: string
}): OnboardingSourceSummary {
  const sourceDetails = input.connectors.map((connector) => {
    const sources = normalizeSourceNames(connector.sources)
    const explicitCount = positiveInteger(connector.sourceCount)
    const sourceCount = explicitCount ?? (sources.length > 0 ? sources.length : 1)

    return {
      connectorId: connector.id,
      connectorLabel: connector.label,
      sourceCount,
      sources,
      status: connector.status,
    }
  })
  const connectedSourceCount = sourceDetails.reduce((sum, detail) => sum + detail.sourceCount, 0)
  const websiteSourceCount = input.websiteUrl?.trim() ? 1 : 0

  return {
    connectorCount: input.connectors.length,
    connectedSourceCount,
    sourceDetails,
    totalSourceCount: connectedSourceCount + websiteSourceCount,
    websiteSourceCount,
  }
}

function normalizeSourceNames(sources: readonly string[] | undefined): string[] {
  return Array.from(
    new Set(
      (sources ?? [])
        .map((source) => source.trim())
        .filter(Boolean),
    ),
  )
}

function positiveInteger(value: number | undefined): number | undefined {
  if (!Number.isFinite(value) || value === undefined) return undefined
  const normalized = Math.floor(value)
  return normalized > 0 ? normalized : undefined
}
