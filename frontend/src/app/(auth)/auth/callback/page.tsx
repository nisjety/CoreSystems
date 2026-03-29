import { headers } from 'next/headers';
import { redirect } from 'next/navigation';
import AuthCallbackClient from './AuthCallbackClient';
import { getCanonicalLocalRedirectPath } from '@/components/auth/lib/local-dev-origin';

type AuthCallbackPageProps = {
  searchParams?: Promise<Record<string, string | string[] | undefined>>;
};

function toSearchString(
  searchParams: Record<string, string | string[] | undefined> | undefined,
) {
  if (!searchParams) {
    return undefined;
  }

  const nextSearchParams = new URLSearchParams();
  for (const [key, value] of Object.entries(searchParams)) {
    if (typeof value === 'string') {
      nextSearchParams.set(key, value);
      continue;
    }

    if (Array.isArray(value)) {
      for (const item of value) {
        nextSearchParams.append(key, item);
      }
    }
  }

  const serialized = nextSearchParams.toString();
  return serialized || undefined;
}

export default async function AuthCallbackPage({ searchParams }: AuthCallbackPageProps) {
  const resolvedSearchParams = searchParams ? await searchParams : undefined;
  const requestHeaders = await headers();
  const canonicalLocalRedirect = getCanonicalLocalRedirectPath({
    hostHeader: requestHeaders.get('x-forwarded-host') ?? requestHeaders.get('host'),
    protocolHeader: requestHeaders.get('x-forwarded-proto'),
    pathname: '/auth/callback',
    search: toSearchString(resolvedSearchParams),
  });

  if (canonicalLocalRedirect) {
    redirect(canonicalLocalRedirect);
  }

  return <AuthCallbackClient />;
}
