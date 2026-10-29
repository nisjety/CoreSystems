import 'server-only';

const DEFAULT_AUTH_SERVICE_URL = 'http://auth-core:3011';

export function getInternalApiKey(): string {
  const apiKey = process.env.INTERNAL_API_KEY?.trim();

  if (!apiKey) {
    throw new Error('INTERNAL_API_KEY must be configured');
  }

  return apiKey;
}

export function getAuthServiceUrl(): string {
  const serviceUrl = process.env.AUTH_SERVICE_URL?.trim();

  return serviceUrl || DEFAULT_AUTH_SERVICE_URL;
}