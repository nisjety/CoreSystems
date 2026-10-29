/** Durable, evidence-backed work queue commands. These never mutate provider data. */
export const marketingWorkContracts = {
  list: { id: 'marketing.work.list', method: 'GET', path: '/api/v1/visibility/work', query: ['websiteId', 'site'], allowedActorTypes: ['human', 'model'], readOnly: true },
  saveOpportunity: { id: 'marketing.work.opportunity.save', method: 'POST', path: '/api/v1/visibility/work', allowedActorTypes: ['human'], requiresExplicitUserAction: true },
  saveTechnical: { id: 'marketing.work.technical.save', method: 'POST', path: '/api/v1/visibility/work/technical', allowedActorTypes: ['human'], requiresExplicitUserAction: true },
  verifyTechnical: { id: 'marketing.work.technical.verify', method: 'POST', path: '/api/v1/visibility/work/:taskId/verify', allowedActorTypes: ['human'], requiresExplicitUserAction: true },
  updateStatus: { id: 'marketing.work.task.status', method: 'PATCH', path: '/api/v1/visibility/work/:taskId', allowedActorTypes: ['human'], requiresExplicitUserAction: true },
} as const
