import { describe, expect, it } from 'vitest'
import { CONTROL_PLANE_DSAR_DISCLOSURE } from '@/shared/api/privacy-client'

// The Privacy & data UI renders this disclosure as the authoritative scope
// statement. It must match user-core's gdpr.go BuildDSARExport notes VERBATIM so
// the UI never over- or under-states what the export/erase covers. If gdpr.go
// changes, this test fails until both are reconciled.
describe('CONTROL_PLANE_DSAR_DISCLOSURE', () => {
  it('matches user-core gdpr.go DSAR notes verbatim', () => {
    expect([...CONTROL_PLANE_DSAR_DISCLOSURE]).toEqual([
      'Control Plane export: profile + org memberships + API key metadata.',
      'Audit events for this subject are retained by audit-core (velion.audit.v1.control.*).',
      'Model Plane run history / conversations and Data Plane documents are purged/exported via the velion.gdpr.erasure.requested fan-out (follow-up subscribers).',
    ])
  })
})
