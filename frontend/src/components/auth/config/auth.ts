export const authConfig = {
  defaultRedirectTo: '/',
  passkeyEnabled: true,
  socialProvidersEnabled: true,
  enterpriseFeatures: true,
  organizationManagement: true,
  emailVerificationRequired: true,
  backendUrl: 'http://localhost:3011',
  frontendUrl: 'http://localhost:3000',
} as const;

export const socialProviderNames = ['Microsoft', 'Google', 'Okta', 'Vipps'] as const;

export const activeSocialProviders = ['Microsoft', 'Google'] as const;
// Test comment
