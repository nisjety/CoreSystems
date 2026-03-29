const LOCAL_DEV_HOSTS = new Set(['localhost', '127.0.0.1']);

function getCanonicalLocalOrigin() {
  return (
    process.env.NEXT_PUBLIC_APP_URL ||
    process.env.FRONTEND_URL ||
    'http://localhost:3000'
  );
}

export function getCanonicalLocalRedirectPath(options: {
  hostHeader?: string | null;
  protocolHeader?: string | null;
  pathname: string;
  search?: string;
}) {
  if (process.env.NODE_ENV !== 'development') {
    return null;
  }

  const hostHeader = options.hostHeader?.trim();
  if (!hostHeader) {
    return null;
  }

  const protocol = options.protocolHeader?.trim() || 'http';
  const currentUrl = new URL(`${protocol}://${hostHeader}`);
  const canonicalUrl = new URL(getCanonicalLocalOrigin());

  if (!LOCAL_DEV_HOSTS.has(currentUrl.hostname) || !LOCAL_DEV_HOSTS.has(canonicalUrl.hostname)) {
    return null;
  }

  if (currentUrl.origin === canonicalUrl.origin) {
    return null;
  }

  const targetUrl = new URL(options.pathname, canonicalUrl);
  if (options.search) {
    targetUrl.search = options.search;
  }

  return targetUrl.toString();
}
