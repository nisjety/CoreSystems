import { createSignal } from 'solid-js'
import type { TenderMatch, TenderNoticeDetail, TenderMatches, TenderProfile, TenderStrictness } from '@/shared/api/anbud-client'
import { emptyProfileDraft, type ProfileDraft } from '@/features/anbud/lib/profile'

/**
 * What the tender pages share: the profile as typed, the slider's step, and the profile last searched. The
 * organisation's profile and pipeline are loaded from and saved to its workspace on the server (workspace-sync.ts);
 * nothing is kept in the browser's storage.
 */
const [draft, setDraft] = createSignal<ProfileDraft>(emptyProfileDraft)
const [strictness, setStrictness] = createSignal<TenderStrictness>('balanced')
const [submitted, setSubmitted] = createSignal<TenderProfile | null>(null)

export type AnbudView = 'pipeline' | 'all' | 'radar'
const [view, setView] = createSignal<AnbudView>('all')
const [stage, setStage] = createSignal('inbox')
const [stages, setStages] = createSignal<Record<string, string>>({})
const [matches, setMatches] = createSignal<TenderMatches | null>(null)
const [profileOpen, setProfileOpen] = createSignal(false)
const [loadFailed, setLoadFailed] = createSignal(false)
const [persistFailed, setPersistFailed] = createSignal(false)
/** How much of a suggested profile came from the company's own wins, when the page holds one. */
export type ProfileSuggestion = { wins: number; cpv: number; nuts: number }
const [profileSuggestion, setProfileSuggestion] = createSignal<ProfileSuggestion | null>(null)
/** Where each field of the saved profile came from, as the server keeps it. */
const [profileSources, setProfileSources] = createSignal<Record<string, string>>({})
/** When the organisation last marked its tenders as seen: what was published after it is new. Null until known. */
const [lastSeen, setLastSeen] = createSignal<string | null>(null)

export type TenderFilters = { noticeType: string; verdict: string; deadline: string; status: string; buyer: string; region: string; cpv: string }
export const emptyTenderFilters: TenderFilters = { noticeType: 'all', verdict: 'all', deadline: 'all', status: 'all', buyer: '', region: '', cpv: '' }
const [filters, setFilters] = createSignal<TenderFilters>({ ...emptyTenderFilters })
const [radarReach, setRadarReach] = createSignal('all')
const [radarBuyer, setRadarBuyer] = createSignal('')
const [facets, setFacets] = createSignal<Record<string, Record<string, number>>>({})
const [facetsBusy, setFacetsBusy] = createSignal(false)
export const anbudFilters = { facets, setFacets, facetsBusy, setFacetsBusy, filters, setFilters, radarReach, setRadarReach, radarBuyer, setRadarBuyer,
  reset: () => setFilters({ ...emptyTenderFilters }),
}

const [detailId, setDetailId] = createSignal<string | null>(null)
const [detailSummary, setDetailSummary] = createSignal<TenderMatch | null>(null)
const [detailData, setDetailData] = createSignal<TenderNoticeDetail | null>(null)
export type TenderDetailSection = 'brief' | 'documents' | 'requirements' | 'lots' | 'evaluation' | 'awards' | 'changes' | 'source' | 'assessment'
const [detailSection, setDetailSection] = createSignal<TenderDetailSection>('brief')
export const anbudDetail = {
  id: detailId, summary: detailSummary, data: detailData, setData: setDetailData, section: detailSection, setSection: setDetailSection,
  open: (notice: TenderMatch) => { setDetailId(notice.noticeId); setDetailSummary(notice); setDetailData(null); setDetailSection('brief') },
  close: () => { setDetailId(null); setDetailSummary(null); setDetailData(null); setDetailSection('brief') },
}

export const anbudWorkspace = {
  view, setView, stage, setStage, stages, setStages, matches, setMatches, profileOpen, setProfileOpen,
  loadFailed, setLoadFailed, persistFailed, setPersistFailed, profileSuggestion, setProfileSuggestion, profileSources, setProfileSources, lastSeen, setLastSeen,
}

export const anbudState = { draft, setDraft, strictness, setStrictness, submitted, setSubmitted }

/** Clears the shared state. Tests use it between cases. */
export function resetAnbudState(): void {
  anbudDetail.close()
  setDraft(emptyProfileDraft)
  setStrictness('balanced')
  setSubmitted(null)
  anbudFilters.reset()
  setFacets({})
  setFacetsBusy(false)
  setRadarReach('all')
  setRadarBuyer('')
  setView('all')
  setStage('inbox')
  setStages({})
  setMatches(null)
  setProfileOpen(false)
  setLoadFailed(false)
  setPersistFailed(false)
  setProfileSuggestion(null)
  setProfileSources({})
  setLastSeen(null)
}
