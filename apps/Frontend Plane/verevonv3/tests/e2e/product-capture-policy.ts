import { isAbsolute, relative, resolve, sep } from 'node:path'

const scenarioIds = ['01-kundesvar', '02-salgsrapport', '03-kampanje', '04-prosjektplan']
const gates = ['correctness', 'repeatability', 'durability', 'recovery', 'trust', 'experience', 'performance']
type Manifest = {
  recordingReadiness?: { status?: string; gates?: Record<string, string> }
  tasks: Array<{ id: string; currentEvidence?: { consecutivePassesOnReleaseBuild?: number } }>
}

/** Rehearsal evidence is private and never grants publication approval. */
export function productCaptureSettings(mode: string | undefined, manifest: Manifest, output: string | undefined, repository: string) {
  if (!mode) return undefined
  if (mode !== 'rehearsal' && mode !== 'release') throw new Error('PRODUCT_RECORDING_MODE must be rehearsal or release')
  if (!output || !isAbsolute(output)) throw new Error('Recording requires an absolute PRODUCT_RECORDING_OUTPUT outside the repository')
  const outputDir = resolve(output)
  const within = (parent: string, child: string) => {
    const relation = relative(parent, child)
    return !relation || (!relation.startsWith('..' + sep) && !isAbsolute(relation))
  }
  // Playwright clears its output directory. An ancestor is as unsafe as a
  // child of the repository, even though it is technically "outside" it.
  if (within(resolve(repository), outputDir) || within(outputDir, resolve(repository))) {
    throw new Error('Recording output must stay outside the repository and public media folders')
  }
  if (mode === 'release') {
    if (!['ready-for-recording', 'approved'].includes(manifest.recordingReadiness?.status ?? '')
      || gates.some(gate => manifest.recordingReadiness?.gates?.[gate] !== 'passed')
      || manifest.tasks.length !== scenarioIds.length
      || scenarioIds.some(id => manifest.tasks.filter(task => task.id === id).length !== 1)
      || manifest.tasks.some(task => !Number.isInteger(task.currentEvidence?.consecutivePassesOnReleaseBuild)
        || (task.currentEvidence?.consecutivePassesOnReleaseBuild ?? 0) < 5)) {
      throw new Error('Release recording is gated: all four scenarios need factual review, five consecutive passes and every readiness gate')
    }
  }
  return { mode, outputDir, publicationApproved: false as const }
}
