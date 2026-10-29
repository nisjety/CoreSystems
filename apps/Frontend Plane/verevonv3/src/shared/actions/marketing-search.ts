/** Owner HTTP contract shared by Studio and future authenticated model adapters.
 * This declaration does not register a Model Plane tool or grant provider access. */
export const marketingSearchComparisonContract = {
  id: 'marketing.search.compare', ownerPlane: 'engagement', ownerCore: 'visibility-core',
  method: 'GET', path: '/api/v1/visibility/search/compare', risk: 'low',
  allowedActorTypes: ['human', 'model'], readOnly: true,
} as const
