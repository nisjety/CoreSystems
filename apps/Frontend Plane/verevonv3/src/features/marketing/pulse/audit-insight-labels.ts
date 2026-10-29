import type { AuditChangeKind } from '@/shared/api/marketing/audit-history-boundary'
import type { AuditInsightAlert } from '@/shared/api/marketing/audit-insights-client'

type Translate = (nb: string, en: string) => string
export const insightChangeLabel = (kind: AuditChangeKind, tr: Translate) => ({ new: tr('Nye', 'New'), unchanged: tr('Uendrede', 'Unchanged'), resolved: tr('Observert løst', 'Observed resolved'), regressed: tr('Tilbakefall', 'Regressed'), unknown: tr('Uavklart', 'Unknown') })[kind]
export const insightAlertLabel = (kind: AuditInsightAlert['kind'], tr: Translate) => ({ new_finding: tr('Nytt funn', 'New finding'), regressed_finding: tr('Funn har kommet tilbake', 'Finding regressed'), audit_failed: tr('Revisjonen mislyktes', 'Audit failed'), coverage_gap: tr('Utilstrekkelig sammenlignbar dekning', 'Insufficient comparable coverage') })[kind]
