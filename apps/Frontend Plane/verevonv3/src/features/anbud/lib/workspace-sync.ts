import {
  markWorkspaceSeen,
  readWorkspacePipeline,
  readWorkspaceProfile,
  readWorkspaceSeen,
  suggestWorkspaceProfile,
  writeWorkspaceProfile,
  writeWorkspaceStage,
  type StoredCompanyProfile,
  type TenderProfile,
  type TenderStrictness,
} from '@/shared/api/anbud-client'
import { anbudState, anbudWorkspace, type ProfileSuggestion } from '@/features/anbud/lib/anbud-state'
import { draftFromProfile, emptyProfileDraft } from '@/features/anbud/lib/profile'

/**
 * The organisation's workspace as the tender pages keep it: its company profile, and where each tender stands in
 * its pipeline. The server holds both. The page shows them and changes them here.
 */

export const PIPELINE_UPDATED_EVENT = 'anbud:pipeline-updated'

const strictnessSteps: TenderStrictness[] = ['strict', 'balanced', 'wide']
const trackedFields = ['orgNumber', 'cpv', 'nuts', 'keywords', 'valueRange'] as const

// What the user changed since the current load began. A load answers later than the user can act, so a change made
// in the meantime is kept over the saved state, which is then only the older one.
let profileChanged = false
const stagesChanged = new Map<string, string>()

// The tracked fields as the page last loaded or saved them. A save compares against this to find what the user changed.
let baseline: Record<string, string> = {}

// Every write waits for the one before it, so a quick second change cannot be overtaken by the first. A write that
// fails does not stop the ones after it.
let queue: Promise<unknown> = Promise.resolve()
function queued(write: () => Promise<void>): Promise<void> {
  const next = queue.then(write)
  queue = next.catch(() => undefined)
  return next
}

function snapshot(profile: TenderProfile): Record<string, string> {
  return Object.fromEntries(trackedFields.map((field) => [field, JSON.stringify(profile[field] ?? null)]))
}

/** The part of a stored profile the tender pages search with; the rest is what the page keeps about it. */
function profileOf(stored: StoredCompanyProfile): TenderProfile {
  return { orgNumber: stored.orgNumber, cpv: stored.cpv, nuts: stored.nuts, keywords: stored.keywords, valueRange: stored.valueRange }
}

/**
 * Puts a stored profile in the form. Searched with only when it is submitted: a suggestion the organisation has not
 * checked is a draft, and nothing is searched with it until the organisation saves it.
 */
function applyProfile(stored: StoredCompanyProfile, searchable: boolean): void {
  const profile = profileOf(stored)
  anbudState.setDraft(draftFromProfile(profile))
  anbudState.setStrictness(strictnessSteps.includes(stored.strictness) ? stored.strictness : 'balanced')
  anbudState.setSubmitted(searchable ? profile : null)
  baseline = snapshot(profile)
  anbudWorkspace.setProfileSources(stored.sources ?? {})
}

function clearProfile(): void {
  anbudState.setDraft(emptyProfileDraft)
  anbudState.setStrictness('balanced')
  anbudState.setSubmitted(null)
  baseline = {}
  anbudWorkspace.setProfileSources({})
}

/** What a suggested profile says about the wins it came from, for the notice above the page. */
function countsOf(stored: StoredCompanyProfile): ProfileSuggestion | null {
  const wins = stored.suggestedFrom?.wins ?? 0
  if (wins === 0) return null
  return { wins, cpv: stored.cpv.length, nuts: stored.nuts.length }
}

/**
 * Asks the server to draft a profile from the organisation's own wins. The server keeps the draft, unconfirmed, and
 * answers with it. A failed suggestion leaves the page empty rather than failing the load; an aborted one still stops it.
 */
async function suggestedProfile(signal: AbortSignal): Promise<StoredCompanyProfile | null> {
  try {
    const answer = await suggestWorkspaceProfile(signal)
    if (!answer.applied || !answer.profile) return null
    return answer.profile.profile
  } catch (err) {
    if (signal.aborted) throw err
    return null
  }
}

/**
 * Loads what the organisation saved and puts it in the page. A confirmed profile is searched with at once. A
 * suggestion waits in the editor until the organisation checks it. Resolves true when a confirmed profile is in the
 * page, so the page can search with it. Nothing from another organisation's load is kept.
 */
export async function loadWorkspace(signal: AbortSignal): Promise<boolean> {
  profileChanged = false
  stagesChanged.clear()
  const [saved, stages, lastSeen] = await Promise.all([readWorkspaceProfile(signal), readWorkspacePipeline(signal), loadLastSeen(signal)])
  let searchable = false
  let suggestion: ProfileSuggestion | null = null
  if (!profileChanged) {
    if (saved) {
      const draft = saved.profile.confirmed === false
      applyProfile(saved.profile, !draft)
      searchable = !draft
      if (draft) {
        suggestion = countsOf(saved.profile)
        anbudWorkspace.setProfileOpen(true)
      }
    } else {
      clearProfile()
      const drafted = await suggestedProfile(signal)
      if (drafted && !profileChanged) {
        applyProfile(drafted, false)
        suggestion = countsOf(drafted)
        anbudWorkspace.setProfileOpen(true)
      }
    }
  }
  anbudWorkspace.setStages({ ...Object.fromEntries(stages.map((item) => [item.noticeId, item.stage])), ...Object.fromEntries(stagesChanged) })
  anbudWorkspace.setProfileSuggestion(suggestion)
  anbudWorkspace.setLastSeen(lastSeen)
  return searchable
}

/**
 * When the organisation last marked its tenders as seen. The first visit sets the mark, so nothing is new on it.
 * A failure reading the mark is not a failure of the page: nothing is counted as new then.
 */
async function loadLastSeen(signal: AbortSignal): Promise<string | null> {
  try {
    const seen = await readWorkspaceSeen(signal)
    return seen ?? (await markWorkspaceSeen(signal))
  } catch (err) {
    if (signal.aborted) throw err
    return null
  }
}

/** Marks the organisation's tenders as seen now, so the tenders published so far are no longer new. */
export function markNewTendersSeen(): void {
  markWorkspaceSeen().then(
    (at) => anbudWorkspace.setLastSeen(at),
    () => anbudWorkspace.setPersistFailed(true),
  )
}

/**
 * Saves the profile the page last searched with, and the search step it was searched at, as confirmed. A field whose
 * value the user changed since it was loaded or last saved is marked as theirs; the others keep where they came from.
 */
export function saveProfile(): void {
  const profile = anbudState.submitted()
  if (!profile) return
  profileChanged = true
  const current = snapshot(profile)
  const sources: Record<string, string> = { ...anbudWorkspace.profileSources() }
  for (const field of trackedFields) {
    if (current[field] !== baseline[field]) sources[field] = 'user'
  }
  baseline = current
  anbudWorkspace.setProfileSources(sources)
  anbudWorkspace.setProfileSuggestion(null)
  const stored: StoredCompanyProfile = { ...profile, strictness: anbudState.strictness(), sources, confirmed: true }
  queued(() => writeWorkspaceProfile(stored)).then(
    () => anbudWorkspace.setPersistFailed(false),
    () => anbudWorkspace.setPersistFailed(true),
  )
}

/** Moves a tender to a pipeline stage. The page shows the move at once; a move the server refuses is put back. */
export function setPipelineStage(noticeId: string, stage: string): void {
  const before = anbudWorkspace.stages()[noticeId] ?? 'inbox'
  stagesChanged.set(noticeId, stage)
  anbudWorkspace.setStages((prev) => ({ ...prev, [noticeId]: stage }))
  queued(() => writeWorkspaceStage(noticeId, stage)).then(
    () => { anbudWorkspace.setPersistFailed(false); if (typeof window !== 'undefined') window.dispatchEvent(new Event(PIPELINE_UPDATED_EVENT)) },
    () => {
      stagesChanged.set(noticeId, before)
      anbudWorkspace.setStages((prev) => ({ ...prev, [noticeId]: before }))
      anbudWorkspace.setPersistFailed(true)
    },
  )
}
