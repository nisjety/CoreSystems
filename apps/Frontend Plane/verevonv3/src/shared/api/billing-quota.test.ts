import { afterEach, describe, expect, it, vi } from 'vitest'
import { billingQuotaActionSchema } from '@/shared/actions/billing-quota'
import { getBillingQuota, quotaReadoutMetric, quotaStatusSchema } from './billing-quota'

// billing-core's wire shape, including the keys the readout does not read.
const wire = {
  org_id: 'org_1',
  metric: 'ai.metered.input_tokens',
  limit: 1000,
  used: 850,
  remaining: 150,
  is_exceeded: false,
  utilization: 0.85,
}

afterEach(() => {
  vi.unstubAllEnvs()
  vi.unstubAllGlobals()
})

describe('billing quota response contract', () => {
  it('reads the share and the amounts, and drops the keys the readout does not show', () => {
    expect(quotaStatusSchema.parse(wire)).toEqual({
      limit: 1000,
      used: 850,
      remaining: 150,
      is_exceeded: false,
      utilization: 0.85,
    })
  })

  it('refuses a share outside 0 to 1, a negative amount and a flag that is not a boolean', () => {
    expect(quotaStatusSchema.safeParse({ ...wire, utilization: 1.2 }).success).toBe(false)
    expect(quotaStatusSchema.safeParse({ ...wire, remaining: -1 }).success).toBe(false)
    expect(quotaStatusSchema.safeParse({ ...wire, is_exceeded: 'yes' }).success).toBe(false)
  })
})

describe('billing quota action contract', () => {
  it('is an organisation-scoped read of one metric, and never takes an organisation id', () => {
    const parsed = billingQuotaActionSchema.parse({
      type: 'billing.quota.read',
      metric: ' ai.metered.input_tokens ',
      scope: 'organization',
    })
    expect(parsed.metric).toBe('ai.metered.input_tokens')
    expect(billingQuotaActionSchema.safeParse({ type: 'billing.quota.read', metric: 'ai.metered.input_tokens', scope: 'user' }).success).toBe(false)
    expect(billingQuotaActionSchema.safeParse({ type: 'billing.quota.read', metric: '   ', scope: 'organization' }).success).toBe(false)
    expect(
      billingQuotaActionSchema.safeParse({
        type: 'billing.quota.read',
        metric: 'ai.metered.input_tokens',
        scope: 'organization',
        orgId: 'org_2',
      }).success,
    ).toBe(false)
  })
})

describe('quota readout switch', () => {
  it('names a metric only when VITE_QUOTA_READOUT is the literal true', () => {
    vi.stubEnv('VITE_QUOTA_READOUT_METRIC', 'ai.metered.input_tokens')
    for (const value of ['', 'false', 'TRUE', '1']) {
      vi.stubEnv('VITE_QUOTA_READOUT', value)
      expect(quotaReadoutMetric()).toBeUndefined()
    }
    vi.stubEnv('VITE_QUOTA_READOUT', 'true')
    expect(quotaReadoutMetric()).toBe('ai.metered.input_tokens')
  })

  it('names nothing when the metric is blank', () => {
    vi.stubEnv('VITE_QUOTA_READOUT', 'true')
    vi.stubEnv('VITE_QUOTA_READOUT_METRIC', '   ')
    expect(quotaReadoutMetric()).toBeUndefined()
  })
})

describe('getBillingQuota', () => {
  it('reads the BFF quota route for the metric and returns the parsed status', async () => {
    const fetchMock = vi.fn<(input: string, init?: RequestInit) => Promise<Response>>(
      async () => new Response(JSON.stringify(wire), { headers: { 'content-type': 'application/json' } }),
    )
    vi.stubGlobal('fetch', fetchMock)

    await expect(getBillingQuota('ai.metered.input_tokens')).resolves.toMatchObject({ remaining: 150, utilization: 0.85 })
    expect(fetchMock.mock.calls[0]?.[0]).toMatch(/\/api\/v1\/billing\/quotas\/ai\.metered\.input_tokens$/)
  })

  it('refuses a blank metric before it makes any request', async () => {
    const fetchMock = vi.fn<(input: string, init?: RequestInit) => Promise<Response>>()
    vi.stubGlobal('fetch', fetchMock)

    await expect(getBillingQuota('  ')).rejects.toThrow()
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
