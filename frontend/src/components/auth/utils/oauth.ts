export function createAbsoluteCallbackURL(redirectTo: string): string {
  if (typeof window === 'undefined') return redirectTo;
  return `${window.location.origin}${redirectTo}`;
}

export function buildOAuthURL(provider: string, callbackURL: string): string {
  const encodedCallback = encodeURIComponent(callbackURL);
  
  if (provider === 'google' || provider === 'microsoft') {
    // Built-in social providers use /sign-in/[provider]
    return `http://localhost:3011/api/v2/auth/signIn/${provider}?callbackURL=${encodedCallback}`;
  } else {
    // Custom OAuth providers (Vipps, Okta) use oRPC oauth/initiate endpoint
    return `http://localhost:3011/api/v2/auth/oauth/initiate?provider=${provider}&redirectTo=${encodedCallback}`;
  }
}

export function handleOAuthRedirect(url: string): void {
  window.location.href = url;
}
