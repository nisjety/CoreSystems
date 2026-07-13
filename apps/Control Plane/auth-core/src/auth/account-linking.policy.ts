export function normalizeIdentityEmail(email: string): string {
  return email.trim().toLowerCase();
}

export function canImplicitlyLinkProviderIdentity(input: {
  provider: string;
  providerEmail: string;
  providerEmailVerified: boolean;
  canonicalEmail: string;
}): boolean {
  if (input.provider.trim() === '') {
    return false;
  }
  if (!input.providerEmailVerified) {
    return false;
  }
  return (
    normalizeIdentityEmail(input.providerEmail) ===
    normalizeIdentityEmail(input.canonicalEmail)
  );
}
