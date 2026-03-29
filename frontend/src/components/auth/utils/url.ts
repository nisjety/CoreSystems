export function normalizeRedirectURL(redirectTo: string): string {
  // Ensure redirect URL starts with /
  if (!redirectTo.startsWith('/')) {
    return `/${redirectTo}`;
  }
  return redirectTo;
}

export function isValidRedirectURL(url: string): boolean {
  // Basic validation for redirect URLs
  try {
    const parsed = new URL(url, window.location.origin);
    return parsed.origin === window.location.origin;
  } catch {
    return url.startsWith('/');
  }
}
