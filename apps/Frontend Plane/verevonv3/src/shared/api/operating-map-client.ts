import { requestJson } from '@/shared/api/http'
import { readSseStream } from '@/shared/api/sse'

export type OperatingMapPhaseName = 'Assist' | 'Ground' | 'Act'

export type OperatingMapRoot = {
  id: string
  orgId: string
  status: string
  currentVersionId?: string
  generatedFrom: Record<string, unknown>
  createdAt: string
  updatedAt: string
}

export type OperatingMapDepartment = {
  id: string
  name: string
  confidence: number
  evidenceRefs: string[]
}

export type OperatingMapWorkflow = {
  id: string
  departmentId: string
  name: string
  phase: OperatingMapPhaseName | string
  risk: string
  evidenceRefs: string[]
}

export type OperatingMapAgentBlueprint = {
  id: string
  name: string
  role: 'service' | 'sales' | 'ecommerce' | 'chatbot' | 'workflow' | string
  sourceWorkflowId: string
  requiresApproval: boolean
}

export type OperatingMapRolloutPhase = {
  id: string
  name: OperatingMapPhaseName | string
  description: string
}

export type OperatingMapRiskOverlay = {
  id: string
  label: string
  severity: string
}

export type OperatingMapLearningModule = {
  id: string
  title: string
  audience: string
}

export type OperatingMapRoiNote = {
  id: string
  label: string
  measurement: string
}

export type OperatingMapVersion = {
  id: string
  mapId: string
  orgId: string
  departments: OperatingMapDepartment[]
  workflows: OperatingMapWorkflow[]
  agentBlueprints: OperatingMapAgentBlueprint[]
  rolloutPhases: OperatingMapRolloutPhase[]
  riskOverlays: OperatingMapRiskOverlay[]
  learningModules: OperatingMapLearningModule[]
  roiNotes: OperatingMapRoiNote[]
  evidenceRefs: string[]
  confidence: number
  createdByRunId?: string
  createdAt?: string
  summary?: string
}

export type OperatingMapProposal = {
  id: string
  mapId: string
  orgId: string
  proposedVersion: OperatingMapVersion
  evidenceRefs: string[]
  generatedByRunId?: string
  status: 'pending' | 'accepted' | 'rejected' | string
  reviewedBy?: string
  createdAt: string
  reviewedAt?: string
}

export type OperatingMapBlueprintSuggestion = {
  id: string
  mapId: string
  versionId: string
  orgId: string
  blueprintId: string
  role: string
  sourceWorkflowId?: string
  name: string
  status: string
  requestedBy?: string
  payload: Record<string, unknown>
  createdAt: string
  updatedAt: string
}

export type OperatingMapSnapshot = {
  map: OperatingMapRoot | null
  currentVersion: OperatingMapVersion | null
  proposals: OperatingMapProposal[]
  blueprintSuggestions: OperatingMapBlueprintSuggestion[]
}

export type OperatingMapGenerateResult = {
  proposal: OperatingMapProposal
  runId?: string
  status: string
}

export type OperatingMapReviewResult = {
  proposal: OperatingMapProposal
  newVersion: OperatingMapVersion | null
}

export type OperatingMapRunEvent = {
  detail: string
  event?: string
  runId?: string
  status?: string
}

export type OperatingMapRunHandlers = {
  onDone?: () => void
  // Raw error (ApiError or Error) — never a pre-formatted English string. This
  // module cannot call useI18n() itself (it's a plain client, not a Solid
  // component), so callers translate via translateApiError(err, i18n.tr, ...)
  // once they receive it, using their own locale-aware fallback copy.
  onError?: (err: unknown) => void
  onEvent?: (event: OperatingMapRunEvent) => void
}

function orgHeaders(orgId: string): Record<string, string> {
  return orgId ? { 'x-verevon-org-id': orgId } : {}
}

export async function loadOperatingMap(orgId: string, signal?: AbortSignal): Promise<OperatingMapSnapshot> {
  const raw = await requestJson<unknown>('/api/v1/knowledge/operating-map', {
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeSnapshot(raw)
}

export async function generateOperatingMap(orgId: string, signal?: AbortSignal): Promise<OperatingMapGenerateResult> {
  const raw = await requestJson<unknown>('/api/v1/knowledge/operating-map/generate', {
    method: 'POST',
    body: JSON.stringify({
      generatedFrom: {
        source: 'knowledge-workspace',
        capability: 'operating_map.generate',
      },
    }),
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeGenerateResult(raw)
}

export async function reviewOperatingMapProposal(
  orgId: string,
  proposalId: string,
  decision: 'accept' | 'reject',
  signal?: AbortSignal,
): Promise<OperatingMapReviewResult> {
  const raw = await requestJson<unknown>(
    `/api/v1/knowledge/operating-map/proposals/${encodeURIComponent(proposalId)}/review`,
    {
      method: 'POST',
      body: JSON.stringify({ decision }),
      headers: orgHeaders(orgId),
      signal,
    },
  )
  return normalizeReviewResult(raw)
}

export async function streamOperatingMapRunEvents(
  orgId: string,
  runId: string,
  handlers: OperatingMapRunHandlers,
  signal?: AbortSignal,
): Promise<void> {
  await readSseStream(
    `/api/v1/knowledge/operating-map/runs/${encodeURIComponent(runId)}/events`,
    { headers: orgHeaders(orgId), signal },
    (event) => {
      const parsed = normalizeRunEvent(event.event, event.data)
      handlers.onEvent?.(parsed)
      if (parsed.status === 'completed') handlers.onDone?.()
    },
    (err) => handlers.onError?.(err),
    () => handlers.onDone?.(),
  )
}

function normalizeSnapshot(raw: unknown): OperatingMapSnapshot {
  const record = asRecord(raw) ?? {}
  return {
    map: normalizeRoot(record.map),
    currentVersion: normalizeVersion(record.current_version ?? record.currentVersion),
    proposals: arrayField(record, 'proposals').map(normalizeProposal).filter(isPresent),
    blueprintSuggestions: arrayField(record, 'blueprint_suggestions').map(normalizeBlueprintSuggestion).filter(isPresent),
  }
}

function normalizeGenerateResult(raw: unknown): OperatingMapGenerateResult {
  const record = asRecord(raw) ?? {}
  const proposal = normalizeProposal(record.proposal)
  if (!proposal) throw new Error('Operating Map generation did not return a proposal.')
  return {
    proposal,
    runId: stringField(record, 'run_id') ?? stringField(record, 'runId') ?? proposal.generatedByRunId,
    status: stringField(record, 'status') ?? proposal.status,
  }
}

function normalizeReviewResult(raw: unknown): OperatingMapReviewResult {
  const record = asRecord(raw) ?? {}
  const proposal = normalizeProposal(record.proposal)
  if (!proposal) throw new Error('Operating Map review did not return a proposal.')
  return {
    proposal,
    newVersion: normalizeVersion(record.new_version ?? record.newVersion),
  }
}

function normalizeRoot(raw: unknown): OperatingMapRoot | null {
  const record = asRecord(raw)
  if (!record) return null
  const id = stringField(record, 'operating_map_id') ?? stringField(record, 'id')
  const orgId = stringField(record, 'org_id') ?? stringField(record, 'orgId')
  if (!id || !orgId) return null
  return {
    id,
    orgId,
    status: stringField(record, 'status') ?? 'draft',
    ...(stringField(record, 'current_version_id') ? { currentVersionId: stringField(record, 'current_version_id') } : {}),
    generatedFrom: asRecord(record.generated_from) ?? {},
    createdAt: stringField(record, 'created_at') ?? '',
    updatedAt: stringField(record, 'updated_at') ?? '',
  }
}

function normalizeProposal(raw: unknown): OperatingMapProposal | null {
  const record = asRecord(raw)
  if (!record) return null
  const id = stringField(record, 'proposal_id') ?? stringField(record, 'id')
  const mapId = stringField(record, 'operating_map_id') ?? stringField(record, 'mapId')
  const orgId = stringField(record, 'org_id') ?? stringField(record, 'orgId')
  if (!id || !mapId || !orgId) return null
  return {
    id,
    mapId,
    orgId,
    proposedVersion: normalizeVersion(record.proposed_version, { mapId, orgId }) ?? emptyVersion(mapId, orgId),
    evidenceRefs: stringArray(record.evidence_refs),
    ...(stringField(record, 'generated_by_run_id') ? { generatedByRunId: stringField(record, 'generated_by_run_id') } : {}),
    status: stringField(record, 'proposal_status') ?? 'pending',
    ...(stringField(record, 'reviewed_by') ? { reviewedBy: stringField(record, 'reviewed_by') } : {}),
    createdAt: stringField(record, 'created_at') ?? '',
    ...(stringField(record, 'reviewed_at') ? { reviewedAt: stringField(record, 'reviewed_at') } : {}),
  }
}

function normalizeBlueprintSuggestion(raw: unknown): OperatingMapBlueprintSuggestion | null {
  const record = asRecord(raw)
  if (!record) return null
  const id = stringField(record, 'suggestion_id') ?? stringField(record, 'id')
  const mapId = stringField(record, 'operating_map_id') ?? stringField(record, 'mapId')
  const versionId = stringField(record, 'version_id') ?? stringField(record, 'versionId')
  const orgId = stringField(record, 'org_id') ?? stringField(record, 'orgId')
  const blueprintId = stringField(record, 'blueprint_id') ?? stringField(record, 'blueprintId')
  if (!id || !mapId || !versionId || !orgId || !blueprintId) return null
  return {
    id,
    mapId,
    versionId,
    orgId,
    blueprintId,
    role: stringField(record, 'role') ?? 'workflow',
    ...(stringField(record, 'source_workflow_id') ? { sourceWorkflowId: stringField(record, 'source_workflow_id') } : {}),
    name: stringField(record, 'name') ?? 'Agent blueprint',
    status: stringField(record, 'suggestion_status') ?? 'suggested',
    ...(stringField(record, 'requested_by') ? { requestedBy: stringField(record, 'requested_by') } : {}),
    payload: asRecord(record.payload) ?? {},
    createdAt: stringField(record, 'created_at') ?? '',
    updatedAt: stringField(record, 'updated_at') ?? '',
  }
}

function normalizeVersion(raw: unknown, fallback?: { mapId: string; orgId: string }): OperatingMapVersion | null {
  const record = asRecord(raw)
  if (!record) return null
  const mapId = stringField(record, 'operating_map_id') ?? fallback?.mapId ?? ''
  const orgId = stringField(record, 'org_id') ?? fallback?.orgId ?? ''
  return {
    id: stringField(record, 'version_id') ?? stringField(record, 'id') ?? '',
    mapId,
    orgId,
    departments: arrayField(record, 'departments').map(normalizeDepartment),
    workflows: arrayField(record, 'workflows').map(normalizeWorkflow),
    agentBlueprints: arrayField(record, 'agent_blueprints').map(normalizeAgentBlueprint),
    rolloutPhases: arrayField(record, 'rollout_phases').map(normalizeRolloutPhase),
    riskOverlays: arrayField(record, 'risk_overlays').map(normalizeRiskOverlay),
    learningModules: arrayField(record, 'learning_modules').map(normalizeLearningModule),
    roiNotes: arrayField(record, 'roi_notes').map(normalizeRoiNote),
    evidenceRefs: stringArray(record.evidence_refs),
    confidence: numberField(record, 'confidence') ?? 0.5,
    ...(stringField(record, 'created_by_run_id') ? { createdByRunId: stringField(record, 'created_by_run_id') } : {}),
    ...(stringField(record, 'created_at') ? { createdAt: stringField(record, 'created_at') } : {}),
    ...(stringField(record, 'summary') ? { summary: stringField(record, 'summary') } : {}),
  }
}

function normalizeDepartment(raw: unknown): OperatingMapDepartment {
  const record = asRecord(raw) ?? {}
  return {
    id: stringField(record, 'id') ?? slug(stringField(record, 'name') ?? 'department'),
    name: stringField(record, 'name') ?? 'Department',
    confidence: numberField(record, 'confidence') ?? 0.5,
    evidenceRefs: stringArray(record.evidence_refs),
  }
}

function normalizeWorkflow(raw: unknown): OperatingMapWorkflow {
  const record = asRecord(raw) ?? {}
  return {
    id: stringField(record, 'id') ?? slug(stringField(record, 'name') ?? 'workflow'),
    departmentId: stringField(record, 'department_id') ?? stringField(record, 'departmentId') ?? '',
    name: stringField(record, 'name') ?? 'Workflow',
    phase: stringField(record, 'phase') ?? 'Assist',
    risk: stringField(record, 'risk') ?? 'medium',
    evidenceRefs: stringArray(record.evidence_refs),
  }
}

function normalizeAgentBlueprint(raw: unknown): OperatingMapAgentBlueprint {
  const record = asRecord(raw) ?? {}
  return {
    id: stringField(record, 'id') ?? slug(stringField(record, 'name') ?? 'agent-blueprint'),
    name: stringField(record, 'name') ?? 'Agent blueprint',
    role: stringField(record, 'role') ?? 'workflow',
    sourceWorkflowId: stringField(record, 'source_workflow_id') ?? stringField(record, 'sourceWorkflowId') ?? '',
    requiresApproval: booleanField(record, 'requires_approval') ?? true,
  }
}

function normalizeRolloutPhase(raw: unknown): OperatingMapRolloutPhase {
  const record = asRecord(raw) ?? {}
  return {
    id: stringField(record, 'id') ?? slug(stringField(record, 'name') ?? 'phase'),
    name: stringField(record, 'name') ?? 'Assist',
    description: stringField(record, 'description') ?? '',
  }
}

function normalizeRiskOverlay(raw: unknown): OperatingMapRiskOverlay {
  const record = asRecord(raw) ?? {}
  return {
    id: stringField(record, 'id') ?? slug(stringField(record, 'label') ?? 'risk'),
    label: stringField(record, 'label') ?? 'Risk review',
    severity: stringField(record, 'severity') ?? 'medium',
  }
}

function normalizeLearningModule(raw: unknown): OperatingMapLearningModule {
  const record = asRecord(raw) ?? {}
  return {
    id: stringField(record, 'id') ?? slug(stringField(record, 'title') ?? 'learning-module'),
    title: stringField(record, 'title') ?? 'Learning module',
    audience: stringField(record, 'audience') ?? 'team',
  }
}

function normalizeRoiNote(raw: unknown): OperatingMapRoiNote {
  const record = asRecord(raw) ?? {}
  return {
    id: stringField(record, 'id') ?? slug(stringField(record, 'label') ?? 'roi-note'),
    label: stringField(record, 'label') ?? 'ROI note',
    measurement: stringField(record, 'measurement') ?? '',
  }
}

function normalizeRunEvent(event: string | undefined, data: string): OperatingMapRunEvent {
  if (!data) return { detail: event ?? 'Operating Map updated.', event }
  try {
    const record = asRecord(JSON.parse(data))
    if (!record) return { detail: data, event }
    return {
      detail: stringField(record, 'detail') ?? stringField(record, 'message') ?? stringField(record, 'status') ?? data,
      event,
      ...(stringField(record, 'runId') ? { runId: stringField(record, 'runId') } : {}),
      ...(stringField(record, 'status') ? { status: stringField(record, 'status') } : {}),
    }
  } catch {
    return { detail: data, event }
  }
}

function emptyVersion(mapId: string, orgId: string): OperatingMapVersion {
  return {
    id: '',
    mapId,
    orgId,
    departments: [],
    workflows: [],
    agentBlueprints: [],
    rolloutPhases: [],
    riskOverlays: [],
    learningModules: [],
    roiNotes: [],
    evidenceRefs: [],
    confidence: 0,
  }
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null
}

function arrayField(record: Record<string, unknown>, key: string): unknown[] {
  const value = record[key]
  return Array.isArray(value) ? value : []
}

function stringField(record: Record<string, unknown>, key: string): string | undefined {
  const value = record[key]
  return typeof value === 'string' && value.trim().length > 0 ? value : undefined
}

function numberField(record: Record<string, unknown>, key: string): number | undefined {
  const value = record[key]
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

function booleanField(record: Record<string, unknown>, key: string): boolean | undefined {
  const value = record[key]
  return typeof value === 'boolean' ? value : undefined
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []
}

function slug(value: string): string {
  return value.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'item'
}

function isPresent<T>(value: T | null): value is T {
  return value !== null
}
