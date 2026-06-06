const CONNECTOR_PROVIDER_KEYS: Record<string, string[]> = {
  github: ["github"],
  gdrive: ["google", "google-drive"],
  google: ["google", "google-drive"],
  "google-drive": ["google", "google-drive"],
  google_drive: ["google", "google-drive"],
  "google-workspace": ["google", "google-drive"],
  gmail: ["google", "google-drive"],
  microsoft: ["microsoft", "microsoft-graph"],
  microsoft365: ["microsoft", "microsoft-graph"],
  "microsoft-365": ["microsoft", "microsoft-graph"],
  m365: ["microsoft", "microsoft-graph"],
  notion: ["notion"],
  onedrive: ["microsoft", "microsoft-graph"],
  outlook: ["microsoft", "microsoft-graph"],
  sharepoint: ["microsoft", "microsoft-graph"],
  slack: ["slack"],
  shopify: ["shopify"],
  stripe: ["stripe"],
  teams: ["microsoft", "microsoft-graph"],
};

export function providerKeysForConnectionIdentifier(identifier: string): string[] {
  const normalized = identifier.trim().toLowerCase();
  if (!normalized) return [];
  return CONNECTOR_PROVIDER_KEYS[normalized] ?? [];
}

export function providerKeysForConnectionIdentifiers(identifiers: string[]): string[] {
  return Array.from(
    new Set(
      identifiers.flatMap((identifier) => providerKeysForConnectionIdentifier(identifier)),
    ),
  );
}
