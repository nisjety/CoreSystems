import type { LiveKnowledgeSource } from '@/shared/api/knowledge-live-client'
import type { OperatingMapWorkflow } from '@/shared/api/operating-map-client'

/** Resolve only references present in the current, scoped knowledge payload. */
export function resolveWorkflowEvidence(workflow: OperatingMapWorkflow, sources: readonly LiveKnowledgeSource[]) {
  const refs = new Set(workflow.evidenceRefs.filter(Boolean))
  const resolvedRefs = new Set<string>()
  const matchedSources = sources.filter((source) => {
    let matched = false
    if (refs.has(source.id)) {
      resolvedRefs.add(source.id)
      matched = true
    }
    for (const chunk of source.chunksPreview) {
      if (refs.has(chunk.id)) {
        resolvedRefs.add(chunk.id)
        matched = true
      }
    }
    return matched
  })
  return {
    sources: matchedSources,
    unresolvedRefs: [...refs].filter((ref) => !resolvedRefs.has(ref)),
  }
}
