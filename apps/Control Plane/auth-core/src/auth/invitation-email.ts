export function canonicalPublicOrigin(
  value: string,
  productionLike: boolean,
): string {
  const message = 'public URL must be a canonical HTTPS origin';
  let url: URL;
  try {
    url = new URL(value.trim());
  } catch {
    throw new Error(message);
  }

  const loopback = new Set(['localhost', '127.0.0.1', '[::1]']);
  const secure = url.protocol === 'https:';
  const localDevelopmentHttp =
    !productionLike && url.protocol === 'http:' && loopback.has(url.hostname);
  if (
    (!secure && !localDevelopmentHttp) ||
    url.username !== '' ||
    url.password !== '' ||
    url.pathname !== '/' ||
    url.search !== '' ||
    url.hash !== ''
  ) {
    throw new Error(message);
  }

  return url.origin;
}

export function buildInvitationLink(
  frontendUrl: string,
  invitationId: string,
): string {
  const origin = canonicalPublicOrigin(
    frontendUrl,
    process.env.NODE_ENV === 'production',
  );
  return new URL(
    `/accept-invitation/${encodeURIComponent(invitationId)}`,
    origin,
  ).toString();
}

export function escapeInvitationHtml(value: string): string {
  return value.replace(
    /[&<>"']/g,
    (character) =>
      ({
        '&': '&amp;',
        '<': '&lt;',
        '>': '&gt;',
        '"': '&quot;',
        "'": '&#39;',
      })[character] ?? character,
  );
}
