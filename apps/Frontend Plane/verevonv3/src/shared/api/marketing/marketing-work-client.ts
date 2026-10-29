import { sanitizeGroupRule,type GroupRule } from '../visibility/research-client'
import { normalizePulseSite } from './pulse-site-stack.ts'
import { pulsePeriodDates } from './pulse-timeline.ts'
import type { PulseQuarter } from './pulse-client.ts'
import { isWebsiteId } from './website-client.ts'
import { pageSpeedCheckIds, type PageSpeedCheckId, type PageSpeedCheckStatus, type PageSpeedStrategy } from '../../actions/marketing-pagespeed.ts'
import { isAuditId, isAuditFindingId, isAuditRule, isAuditRunId, isAuditFingerprint, isAuditCheckStatus, type AuditRule, type AuditCheckStatus } from './audit-client.ts'
import { isPublicSitePage } from './discovery-client.ts'

export type MarketingTaskStatus = 'todo' | 'in_progress' | 'done' | 'dismissed'
export type MarketingOpportunityType = 'snippet_review' | 'title_meta' | 'title_test' | 'content_expansion'
export type MarketingAuditEvidence = { kind: 'site_audit_finding'; key: AuditRule; observedAt: string; auditId: string; findingId: string; pageUrl: string; severity: 'warning' | 'error' | 'review'; runId: string; fingerprint: string; relatedUrls: string[] }
export type MarketingReviewEvidence={kind:'search_group_review';key:string;observedAt:string;period:{startDate:string;endDate:string};clicks:number;impressions:number;ctr:number;position:number;review:{rule:GroupRule}}
export type MarketingEvidence = MarketingReviewEvidence | { kind: 'gsc_opportunity'; key: string; opportunityType: MarketingOpportunityType; period: { startDate: string; endDate: string }; clicks: number; impressions: number; ctr: number; position: number; observedAt: string } | { kind: 'pagespeed_check'; key: PageSpeedCheckId; observedAt: string; strategy: PageSpeedStrategy; checkStatus: 'failed' | 'manual' | 'error'; checkScore: number | null; lighthouseVersion: string } | MarketingAuditEvidence
export type MarketingAuditVerification = { status: AuditCheckStatus; observedAt: string; recordedAt: string; auditId: string; findingId: string; pageUrl: string; ruleId: AuditRule; runId: string; fingerprint: string }
export type MarketingReviewVerification={status:AuditCheckStatus;score:null;lighthouseVersion:'';observedAt:string;recordedAt:string;review:{ruleId:string;key:string;present:boolean;matches:boolean}}
export type MarketingVerification = MarketingReviewVerification | { status: PageSpeedCheckStatus; score: number | null; observedAt: string; lighthouseVersion: string; recordedAt: string } | MarketingAuditVerification
export type TaskPlanning = {assigneeId:string;dueDate:string;priority:'low'|'normal'|'high'|'urgent'}
function validPlanning(v:unknown):v is TaskPlanning{return isRecord(v)&&typeof v.assigneeId==='string'&&/^[A-Za-z0-9._:-]{0,200}$/.test(v.assigneeId)&&typeof v.dueDate==='string'&&(v.dueDate===''||validDay(v.dueDate))&&['low','normal','high','urgent'].includes(String(v.priority))}
const projectPlanning=(v:TaskPlanning):TaskPlanning=>({assigneeId:v.assigneeId,dueDate:v.dueDate,priority:v.priority})
export type MarketingTask = {
 planning?:TaskPlanning;
  id: string; websiteId: string; site: string; revision: number; status: MarketingTaskStatus
  createdAt: string; updatedAt: string
  evidence: MarketingEvidence; verification?: MarketingVerification
}
export type MarketingActivity = { id: string; taskId: string; kind: 'created' | 'status_changed' | 'verified' | 'planning_changed'; planning?:TaskPlanning; status: MarketingTaskStatus; occurredAt: string; verification?: MarketingVerification }
export type MarketingWorkSnapshot = {
  websiteId: string; site: string; tasks: MarketingTask[]; activity: MarketingActivity[]
  summary: { open: number; inProgress: number; done: number; dismissed: number; total: number; completionPercent: number | null }
}
type WorkContext = { websiteId: string; site: string }
type SaveOpportunityInput = WorkContext & { websiteRevision: number; quarter: PulseQuarter; query: string }
type UpdateTaskInput = WorkContext & { taskId: string; websiteRevision: number; revision: number; status: MarketingTaskStatus }
type TechnicalInput = WorkContext & { websiteRevision: number; strategy: PageSpeedStrategy; checkId: PageSpeedCheckId; fetchTime: string }
type VerifyInput = WorkContext & { taskId: string; websiteRevision: number; revision: number }
type AuditFindingInput = WorkContext & { websiteRevision: number; auditId: string; findingId: string }

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
const isInt = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
const isRevision = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= 1
const statuses: readonly MarketingTaskStatus[] = ['todo', 'in_progress', 'done', 'dismissed']
const opportunityTypes: readonly MarketingOpportunityType[] = ['snippet_review', 'title_meta', 'title_test', 'content_expansion']
const taskID = /^task_[a-f0-9]{32}$/
const eventID = /^evt_[a-f0-9]{32}$/
const validDay = (value: unknown): value is string => typeof value === 'string' && /^\d{4}-\d\d-\d\d$/.test(value) && pulsePeriodDates({ startDate: value, endDate: value }).length === 1
const validStamp = (value: unknown): value is string => typeof value === 'string' && value.length <= 40 && /^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.test(value) && validDay(value.slice(0, 10)) && Number.isFinite(Date.parse(value))
// Keep provider nanoseconds when comparing RFC3339 receipts; Date alone truncates them.
const stampNanos = (value: string): bigint => BigInt(Date.parse(value)) * 1_000_000n + BigInt((value.match(/\.(\d+)/)?.[1] ?? '').padEnd(9, '0').slice(3))
const validVersion = (value: unknown): value is string => typeof value === 'string' && /^[a-zA-Z0-9.+-]{1,64}$/.test(value)
const scoreMatches = (status: unknown, score: unknown): boolean => typeof status === 'string' && (status === 'passed' ? score === 1 : status === 'failed' ? score === 0 : ['manual', 'error', 'informative', 'not_reported', 'not_applicable'].includes(status) && score === null)
function validVerification(value: unknown): value is MarketingVerification {
  if(isRecord(value)&&isRecord(value.review)){const r=value.review;return isAuditCheckStatus(value.status)&&value.score===null&&value.lighthouseVersion===''&&typeof r.ruleId==='string'&&typeof r.key==='string'&&typeof r.present==='boolean'&&typeof r.matches==='boolean'&&validStamp(value.observedAt)&&validStamp(value.recordedAt)&&(!r.present?!r.matches&&value.status==='inconclusive':r.matches?value.status==='review':value.status==='passed')}
  if (isRecord(value) && 'auditId' in value) return isAuditCheckStatus(value.status) && isAuditId(value.auditId) && (value.findingId === '' || isAuditFindingId(value.findingId)) && (!['failed', 'review'].includes(value.status) || value.findingId !== '') && typeof value.pageUrl === 'string' && isAuditRule(value.ruleId) && isAuditRunId(value.runId) && isAuditFingerprint(value.fingerprint) && validStamp(value.observedAt) && validStamp(value.recordedAt) && !('score' in value) && !('lighthouseVersion' in value)
  return isRecord(value) && scoreMatches(value.status, value.score) && validStamp(value.observedAt) && validStamp(value.recordedAt) && validVersion(value.lighthouseVersion)
}
function projectVerification(value: MarketingVerification): MarketingVerification {
  if('review' in value)return {status:value.status,score:null,lighthouseVersion:'',observedAt:value.observedAt,recordedAt:value.recordedAt,review:{...value.review}}
  if ('auditId' in value) return { status: value.status, observedAt: value.observedAt, recordedAt: value.recordedAt, auditId: value.auditId, findingId: value.findingId, pageUrl: value.pageUrl, ruleId: value.ruleId, runId: value.runId, fingerprint: value.fingerprint }
  return { status: value.status, score: value.score, observedAt: value.observedAt, lighthouseVersion: value.lighthouseVersion, recordedAt: value.recordedAt }
}
function projectEvidence(value: MarketingEvidence): MarketingEvidence {
  if(value.kind==='search_group_review')return {kind:value.kind,key:value.key,observedAt:value.observedAt,period:{...value.period},clicks:value.clicks,impressions:value.impressions,ctr:value.ctr,position:value.position,review:{rule:sanitizeGroupRule(value.review.rule)}}
  if (value.kind === 'site_audit_finding') return { kind: value.kind, key: value.key, observedAt: value.observedAt, auditId: value.auditId, findingId: value.findingId, pageUrl: value.pageUrl, severity: value.severity, runId: value.runId, fingerprint: value.fingerprint, relatedUrls: [...value.relatedUrls] }
  return value.kind === 'pagespeed_check'
    ? { kind: value.kind, key: value.key, observedAt: value.observedAt, strategy: value.strategy, checkStatus: value.checkStatus, checkScore: value.checkScore, lighthouseVersion: value.lighthouseVersion }
    : { kind: value.kind, key: value.key, opportunityType: value.opportunityType, period: { startDate: value.period.startDate, endDate: value.period.endDate }, clicks: value.clicks, impressions: value.impressions, ctr: value.ctr, position: value.position, observedAt: value.observedAt }
}

const auditFields = ['auditId', 'findingId', 'pageUrl', 'severity', 'runId', 'fingerprint', 'relatedUrls']
function verificationMatches(value: unknown, evidence: MarketingEvidence): value is MarketingVerification {
 if(evidence.kind==='search_group_review')return validVerification(value)&&'review' in value&&value.review.ruleId===evidence.review.rule.id&&value.review.key===evidence.key&&stampNanos(value.observedAt)>stampNanos(evidence.observedAt)
 if(isRecord(value)&&value.review!==undefined)return false
  if (!validVerification(value) || stampNanos(value.observedAt) <= stampNanos(evidence.observedAt) || stampNanos(value.recordedAt) < stampNanos(value.observedAt)) return false
  if (evidence.kind === 'site_audit_finding') return 'auditId' in value && value.auditId !== evidence.auditId && value.pageUrl === evidence.pageUrl && value.ruleId === evidence.key
  return evidence.kind === 'pagespeed_check' && !('auditId' in value)
}
function validAuditTaskEvidence(value: Record<string, unknown>, site: string): boolean {
  return isAuditRule(value.key) && validStamp(value.observedAt) && isAuditId(value.auditId) && isAuditFindingId(value.findingId) && isPublicSitePage(site, value.pageUrl) && typeof value.severity === 'string' && ['error', 'warning', 'review'].includes(value.severity) && isAuditRunId(value.runId) && isAuditFingerprint(value.fingerprint) && Array.isArray(value.relatedUrls) && value.relatedUrls.length <= 100 && value.relatedUrls.every((url) => isPublicSitePage(site, url)) && new Set(value.relatedUrls).size === value.relatedUrls.length && !['opportunityType', 'period', 'clicks', 'impressions', 'ctr', 'position', 'strategy', 'checkStatus', 'checkScore', 'lighthouseVersion'].some((key) => key in value)
}

function validTask(value: unknown, websiteId: string, site: string): value is MarketingTask {
  if (!isRecord(value) || typeof value.id !== 'string' || !taskID.test(value.id) || value.websiteId !== websiteId || value.site !== site || !isRevision(value.revision) || !statuses.includes(value.status as MarketingTaskStatus) || !validStamp(value.createdAt) || !validStamp(value.updatedAt) || !isRecord(value.evidence) || value.planning!==undefined&&!validPlanning(value.planning)) return false
  const evidence = value.evidence
  if(evidence.kind==='search_group_review'){try{if(!isRecord(evidence.review))return false;const rule=sanitizeGroupRule(evidence.review.rule);return typeof evidence.key==='string'&&evidence.key.length>0&&evidence.key.length<=2048&&!/[\x00-\x1f\x7f]/.test(evidence.key)&&(rule.entity!=='page'||isPublicSitePage(site,evidence.key))&&validStamp(evidence.observedAt)&&isRecord(evidence.period)&&validDay(evidence.period.startDate)&&validDay(evidence.period.endDate)&&isInt(evidence.clicks)&&isInt(evidence.impressions)&&evidence.clicks<=evidence.impressions&&typeof evidence.ctr==='number'&&Number.isFinite(evidence.ctr)&&Math.abs(evidence.ctr-(evidence.impressions?evidence.clicks/evidence.impressions:0))<.000001&&typeof evidence.position==='number'&&Number.isFinite(evidence.position)&&evidence.position>=0&&evidence.position<=1e6&&(value.verification===undefined||validVerification(value.verification)&&'review' in value.verification&&value.verification.review.key===evidence.key&&value.verification.review.ruleId===rule.id&&stampNanos(value.verification.observedAt)>stampNanos(evidence.observedAt))}catch{return false}}
  if('review' in evidence)return false
  if (evidence.kind === 'site_audit_finding') return validAuditTaskEvidence(evidence, site) && stampNanos(evidence.observedAt as string) <= stampNanos(value.createdAt) && (value.verification === undefined || verificationMatches(value.verification, evidence as MarketingAuditEvidence))
  if (auditFields.some((key) => key in evidence)) return false
  if (value.verification !== undefined && (evidence.kind !== 'pagespeed_check' || !validVerification(value.verification) || 'auditId' in value.verification)) return false
  if (evidence.kind === 'pagespeed_check') {
    if (['opportunityType', 'period', 'clicks', 'impressions', 'ctr', 'position'].some((key) => key in evidence)) return false
    if (!pageSpeedCheckIds.includes(evidence.key as PageSpeedCheckId) || typeof evidence.strategy !== 'string' || !['mobile', 'desktop'].includes(evidence.strategy) || typeof evidence.checkStatus !== 'string' || !['failed', 'manual', 'error'].includes(evidence.checkStatus) || !scoreMatches(evidence.checkStatus, evidence.checkScore) || !validStamp(evidence.observedAt) || !validVersion(evidence.lighthouseVersion)) return false
    return value.verification === undefined || validVerification(value.verification) && stampNanos(value.verification.observedAt) > stampNanos(evidence.observedAt) && stampNanos(value.verification.recordedAt) >= stampNanos(value.verification.observedAt)
  }
  if (['strategy', 'checkStatus', 'checkScore', 'lighthouseVersion'].some((key) => key in evidence)) return false
  return evidence.kind === 'gsc_opportunity' && typeof evidence.key === 'string' && evidence.key.trim().length > 0 && evidence.key.length <= 512 && !/[\x00-\x1f\x7f]/.test(evidence.key) && opportunityTypes.includes(evidence.opportunityType as MarketingOpportunityType) &&
    isRecord(evidence.period) && validDay(evidence.period.startDate) && validDay(evidence.period.endDate) && evidence.period.startDate <= evidence.period.endDate &&
    isInt(evidence.clicks) && isInt(evidence.impressions) && evidence.clicks <= evidence.impressions && typeof evidence.ctr === 'number' && Number.isFinite(evidence.ctr) && evidence.ctr >= 0 && evidence.ctr <= 1 && Math.abs(evidence.ctr - (evidence.impressions ? evidence.clicks / evidence.impressions : 0)) <= 0.000001 &&
    typeof evidence.position === 'number' && Number.isFinite(evidence.position) && evidence.position > 0 && evidence.position <= 20 && validStamp(evidence.observedAt)
}

export function isMarketingWorkSnapshot(value: unknown, expected?: WorkContext): value is MarketingWorkSnapshot {
  if (!isRecord(value) || typeof value.websiteId !== 'string' || !isWebsiteId(value.websiteId) || typeof value.site !== 'string') return false
  try { if (normalizePulseSite(value.site) !== value.site) return false } catch { return false }
  if (expected && (value.websiteId !== expected.websiteId || value.site !== expected.site)) return false
  if (!Array.isArray(value.tasks) || value.tasks.length > 500 || !value.tasks.every((task) => validTask(task, value.websiteId as string, value.site as string))) return false
  const tasks = value.tasks as MarketingTask[]
  const taskIds = new Set(tasks.map((task) => task.id))
  if (taskIds.size !== tasks.length || !Array.isArray(value.activity) || value.activity.length > 1000) return false
  const activityIDs = new Set<string>()
  for (const item of value.activity) {
    if (!isRecord(item) || typeof item.id !== 'string' || !eventID.test(item.id) || activityIDs.has(item.id) || typeof item.taskId !== 'string' || !taskIds.has(item.taskId) || typeof item.kind !== 'string' || !['created', 'status_changed', 'verified', 'planning_changed'].includes(item.kind) || !statuses.includes(item.status as MarketingTaskStatus) || !validStamp(item.occurredAt)) return false
    if (item.kind === 'verified' ? !verificationMatches(item.verification, tasks.find((task) => task.id === item.taskId)!.evidence) : item.verification !== undefined) return false
    if((item.kind==='planning_changed')!==(item.planning!==undefined)||item.planning!==undefined&&!validPlanning(item.planning))return false
    activityIDs.add(item.id)
  }
  if (!isRecord(value.summary)) return false
  const summary = value.summary
  if (!['open', 'inProgress', 'done', 'dismissed', 'total'].every((key) => isInt(summary[key]))) return false
  const done = tasks.filter((task) => task.status === 'done').length
  const dismissed = tasks.filter((task) => task.status === 'dismissed').length
  const inProgress = tasks.filter((task) => task.status === 'in_progress').length
  const open = tasks.filter((task) => task.status === 'todo' || task.status === 'in_progress').length
  const total = tasks.length
  const completion = total === dismissed ? null : Math.round(done / (total - dismissed) * 1000) / 10
  return summary.open === open && summary.inProgress === inProgress && summary.done === done && summary.dismissed === dismissed && summary.total === total && (completion === null ? summary.completionPercent === null : summary.completionPercent === completion)
}

export function sanitizeMarketingWorkSnapshot(value: unknown, expected?: WorkContext): MarketingWorkSnapshot {
  if (!isMarketingWorkSnapshot(value, expected)) throw new Error('Invalid Marketing work response')
  return {
    websiteId: value.websiteId, site: value.site,
    tasks: value.tasks.map((task) => ({
      id: task.id, websiteId: task.websiteId, site: task.site, revision: task.revision, status: task.status, createdAt: task.createdAt, updatedAt: task.updatedAt,
      evidence: projectEvidence(task.evidence), ...(task.planning?{planning:projectPlanning(task.planning)}:{}), ...(task.verification ? { verification: projectVerification(task.verification) } : {}),
    })),
    activity: value.activity.map((item) => ({ id: item.id, taskId: item.taskId, kind: item.kind, status: item.status, occurredAt: item.occurredAt, ...(item.planning?{planning:projectPlanning(item.planning)}:{}), ...(item.verification ? { verification: projectVerification(item.verification) } : {}) })),
    summary: { open: value.summary.open, inProgress: value.summary.inProgress, done: value.summary.done, dismissed: value.summary.dismissed, total: value.summary.total, completionPercent: value.summary.completionPercent },
  }
}

function assertContext(websiteId: string, site: string) {
  if (!isWebsiteId(websiteId)) throw new Error('Invalid website identity')
  try { if (normalizePulseSite(site) !== site) throw new Error('Invalid work site') } catch { throw new Error('Invalid work site') }
}
function assertRevision(revision: number) { if (!isRevision(revision)) throw new Error('Invalid website revision') }
async function readSnapshot(response: Response, context: WorkContext) {
  if (!response.ok) throw new Error(response.status === 409 ? 'work_conflict' : 'Marketing work is unavailable')
  const payload: unknown = await response.json()
  return sanitizeMarketingWorkSnapshot(isRecord(payload) ? payload.data : undefined, context)
}
function jsonWrite(method: 'POST' | 'PATCH', path: string, body: object, signal?: AbortSignal) {
  return fetch(path, {
    method, credentials: 'include', headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify(body), signal,
  })
}

export async function loadMarketingWork(websiteId: string, site: string, signal?: AbortSignal): Promise<MarketingWorkSnapshot> {
  assertContext(websiteId, site)
  const query = new URLSearchParams({ websiteId, site })
  return readSnapshot(await fetch(`/api/v1/visibility/work?${query}`, { credentials: 'include', headers: { Accept: 'application/json' }, signal }), { websiteId, site })
}

export async function saveMarketingOpportunity(input: SaveOpportunityInput, signal?: AbortSignal): Promise<MarketingWorkSnapshot> {
  assertContext(input.websiteId, input.site); assertRevision(input.websiteRevision)
  if ((!/^Q[1-4]$/.test(input.quarter) && input.quarter !== 'FY') || !input.query.trim() || input.query.length > 512) throw new Error('Invalid search opportunity')
  return readSnapshot(await jsonWrite('POST', '/api/v1/visibility/work', { websiteId: input.websiteId, site: input.site, websiteRevision: input.websiteRevision, quarter: input.quarter, query: input.query }, signal), input)
}

export async function updateMarketingTask(input: UpdateTaskInput, signal?: AbortSignal): Promise<MarketingWorkSnapshot> {
  assertContext(input.websiteId, input.site); assertRevision(input.websiteRevision); assertRevision(input.revision)
  if (!taskID.test(input.taskId) || !statuses.includes(input.status)) throw new Error('Invalid task update')
  return readSnapshot(await jsonWrite('PATCH', `/api/v1/visibility/work/${input.taskId}`, { websiteId: input.websiteId, site: input.site, websiteRevision: input.websiteRevision, revision: input.revision, status: input.status }, signal), input)
}

export async function saveMarketingTechnical(input: TechnicalInput, signal?: AbortSignal): Promise<MarketingWorkSnapshot> {
  assertContext(input.websiteId, input.site); assertRevision(input.websiteRevision)
  if (!['mobile', 'desktop'].includes(input.strategy) || !pageSpeedCheckIds.includes(input.checkId) || !validStamp(input.fetchTime)) throw new Error('Invalid technical finding')
  return readSnapshot(await jsonWrite('POST', '/api/v1/visibility/work/technical', { websiteId: input.websiteId, site: input.site, websiteRevision: input.websiteRevision, strategy: input.strategy, checkId: input.checkId, fetchTime: input.fetchTime }, signal), input)
}

export async function saveMarketingAuditFinding(input: AuditFindingInput, signal?: AbortSignal): Promise<MarketingWorkSnapshot> {
  assertContext(input.websiteId, input.site); assertRevision(input.websiteRevision)
  if (!isAuditId(input.auditId) || !isAuditFindingId(input.findingId)) throw new Error('Invalid audit finding identity')
  return readSnapshot(await jsonWrite('POST', '/api/v1/visibility/work/audit', { websiteId: input.websiteId, site: input.site, websiteRevision: input.websiteRevision, auditId: input.auditId, findingId: input.findingId }, signal), input)
}

export async function verifyMarketingTask(input: VerifyInput, signal?: AbortSignal): Promise<MarketingWorkSnapshot> {
  assertContext(input.websiteId, input.site); assertRevision(input.websiteRevision); assertRevision(input.revision)
  if (!taskID.test(input.taskId)) throw new Error('Invalid task identity')
  return readSnapshot(await jsonWrite('POST', `/api/v1/visibility/work/${input.taskId}/verify`, { websiteId: input.websiteId, site: input.site, websiteRevision: input.websiteRevision, revision: input.revision }, signal), input)
}

export async function loadTaskAssignees(context:WorkContext,signal?:AbortSignal):Promise<{id:string;name:string}[]> {
 assertContext(context.websiteId,context.site);const response=await fetch(`/api/v1/visibility/work/assignees?${new URLSearchParams(context)}`,{credentials:'include',redirect:'error',signal,headers:{Accept:'application/json'}});if(!response.ok)throw Error('membership_unavailable');const v:unknown=await response.json();if(!isRecord(v)||!isRecord(v.data)||!Array.isArray(v.data.members)||v.data.members.length>1000)throw Error('Invalid members');return v.data.members.map(v=>{if(!isRecord(v)||typeof v.id!=='string'||!/^[A-Za-z0-9._:-]{1,200}$/.test(v.id)||typeof v.name!=='string'||v.name.length>200||/[\x00-\x1f\x7f]/.test(v.name))throw Error('Invalid member');return {id:v.id,name:v.name}})
}
export async function planMarketingTask(input:VerifyInput,planning:TaskPlanning,signal?:AbortSignal):Promise<MarketingWorkSnapshot>{assertContext(input.websiteId,input.site);assertRevision(input.websiteRevision);assertRevision(input.revision);if(!taskID.test(input.taskId)||!validPlanning(planning))throw Error('Invalid planning');const {taskId,...context}=input;return readSnapshot(await jsonWrite('PATCH',`/api/v1/visibility/work/${taskId}/planning`,{...context,...planning},signal),input)}

export async function saveSearchReview(input:WorkContext & {websiteRevision:number;ruleId:string;key:string},signal?:AbortSignal){assertContext(input.websiteId,input.site);assertRevision(input.websiteRevision);return readSnapshot(await jsonWrite('POST','/api/v1/visibility/work/review',input,signal),input)}
