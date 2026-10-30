/** First-party aggregate receipts only. No contact fields or collection writes. */
export const marketingOutcomesContract = { id: 'marketing.outcomes.read', method: 'GET', path: '/api/v1/marketing/outcomes', readOnly: true, allowedActorTypes: ['human'] } as const
