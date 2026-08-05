import { describe, expect, it } from 'vitest'
import { parseInboxResolutionPlan, parseInboxTriageProposal } from './inbox-ai-triage'

describe('parseInboxTriageProposal', () => {
  it('accepts a bounded, reviewable triage proposal from a fenced model response', () => {
    expect(parseInboxTriageProposal(`\`\`\`json
      {"confidence":0.84,"reason":"The customer reports a missing delivery and needs carrier follow-up.","suggestedFields":{"category":"delivery","intent":"missing_delivery","work_type":"incident","priority":"high","severity":"medium"},"incident":{"title":"Delivery disruption","customer_impact":"Customers report missing deliveries."}}
    \`\`\``)).toEqual({
      confidence: 0.84,
      reason: 'The customer reports a missing delivery and needs carrier follow-up.',
      suggestedFields: {
        category: 'delivery',
        intent: 'missing_delivery',
        work_type: 'incident',
        priority: 'high',
        severity: 'medium',
      },
      incident: {
        title: 'Delivery disruption',
        customer_impact: 'Customers report missing deliveries.',
      },
      problem: undefined,
    })
  })

  it('rejects prose, unsupported priority values, and empty field proposals', () => {
    expect(parseInboxTriageProposal('Route this urgently to delivery support.')).toBeNull()
    expect(parseInboxTriageProposal('{"confidence":0.9,"reason":"Urgent","suggestedFields":{"priority":"immediately"}}')).toBeNull()
    expect(parseInboxTriageProposal('{"confidence":0.9,"reason":"Unknown type","suggestedFields":{"work_type":"project"}}')).toBeNull()
    expect(parseInboxTriageProposal('{"confidence":0.9,"reason":"Needs review","suggestedFields":{}}')).toBeNull()
    expect(parseInboxTriageProposal('{"confidence":0.9,"reason":"Needs review","suggestedFields":{"work_type":"incident"}}')).toBeNull()
    expect(parseInboxTriageProposal('{"confidence":0.9,"reason":"Needs review","suggestedFields":{"priority":"high"},"incident":{"title":"x","customer_impact":"y"}}')).toBeNull()
    expect(parseInboxTriageProposal('{"confidence":0.9,"reason":"Needs review","suggestedFields":{"priority":"high"},"problem":{"title":"x","summary":"y"}}')).toBeNull()
		expect(parseInboxTriageProposal('{"confidence":0.9,"reason":"Terminal closure needs a human decision","suggestedFields":{"status":"resolved"}}')).toBeNull()
  })

	it('accepts only active-work ticket status proposals', () => {
		expect(parseInboxTriageProposal('{"confidence":0.9,"reason":"The customer has been asked for the order number.","suggestedFields":{"status":"waiting_customer"}}')?.suggestedFields.status).toBe('waiting_customer')
	})

  it('preserves a bounded Problem candidate only beside incident triage', () => {
    const proposal = parseInboxTriageProposal('{"confidence":0.9,"reason":"Repeated checkout failures share a timeout.","suggestedFields":{"work_type":"incident","severity":"critical"},"incident":{"title":"Checkout failure","customer_impact":"Customers cannot checkout."},"problem":{"title":"Checkout dependency instability","summary":"Several checkout failures share a timeout.","root_cause":"Gateway timeout observed."}}')
    expect(proposal?.problem).toEqual({
      title: 'Checkout dependency instability',
      summary: 'Several checkout failures share a timeout.',
      root_cause: 'Gateway timeout observed.',
    })
  })

  it('accepts routing only when the proposed team exactly matches the canonical directory', () => {
    const text = '{"confidence":0.88,"reason":"Billing should own this payment issue.","suggestedFields":{"category":"payment","team_id":"team_billing","team_name":"Billing"}}'
    const ticketTeams = [{ id: 'team_billing', name: 'Billing' }]

    expect(parseInboxTriageProposal(text, { ticketTeams })?.suggestedFields).toMatchObject({
      team_id: 'team_billing',
      team_name: 'Billing',
    })
    expect(parseInboxTriageProposal(text, { ticketTeams: [{ id: 'team_billing', name: 'Payments' }] })).toBeNull()
    expect(parseInboxTriageProposal('{"confidence":0.88,"reason":"Route it","suggestedFields":{"team_id":"team_billing"}}', { ticketTeams })).toBeNull()
  })
})

describe('parseInboxResolutionPlan', () => {
  it('accepts bounded reply, internal-note, and reviewable-triage suggestions without treating them as actions', () => {
    const plan = parseInboxResolutionPlan(`\`\`\`json
      {
        "summary":"The carrier scan is overdue and the customer needs an update.",
        "reply":"Thanks for letting us know. We are checking the carrier scan and will update you shortly.",
        "internal_note":"Check the carrier exception before promising a delivery date.",
        "triage":{"confidence":0.82,"reason":"The overdue carrier scan supports a delivery investigation.","suggestedFields":{"category":"delivery","intent":"missing_delivery","priority":"high","severity":"medium"}}
      }
    \`\`\``)

    expect(plan).toEqual({
      summary: 'The carrier scan is overdue and the customer needs an update.',
      reply: 'Thanks for letting us know. We are checking the carrier scan and will update you shortly.',
      internalNote: 'Check the carrier exception before promising a delivery date.',
      triage: {
        confidence: 0.82,
        reason: 'The overdue carrier scan supports a delivery investigation.',
        suggestedFields: {
          category: 'delivery',
          intent: 'missing_delivery',
          priority: 'high',
          severity: 'medium',
        },
      },
    })
  })

  it('rejects unknown fields, empty plans, and invalid nested triage', () => {
    expect(parseInboxResolutionPlan('{"summary":"x","unknown":true}')).toBeNull()
    expect(parseInboxResolutionPlan('{"summary":""}')).toBeNull()
    expect(parseInboxResolutionPlan('{"summary":"x","triage":{"confidence":2,"reason":"x","suggestedFields":{"priority":"high"}}}')).toBeNull()
  })
})
