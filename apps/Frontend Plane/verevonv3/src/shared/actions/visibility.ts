/** Visibility owns search/SEO/SEM workflows within Engagement. Legacy marketing
 * action IDs and API contracts remain stable during the service migration. */
export const visibilityWorkspaceContract = {
  id: 'visibility.workspace.open',
  owner: 'visibility-core',
  plane: 'engagement-plane',
  route: '/engagement/visibility',
  legacyRoute: '/studio/pulse',
  readOnly: true,
  allowedActorTypes: ['human'],
} as const
