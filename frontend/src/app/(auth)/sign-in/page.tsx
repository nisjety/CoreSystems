import { headers } from 'next/headers';
import { redirect } from 'next/navigation';
import AuthPage from "../../../components/auth/AuthPage";
import { getCanonicalLocalRedirectPath } from '@/components/auth/lib/local-dev-origin';

type SignInPageProps = {
  searchParams?: Promise<{ redirect?: string }>;
};

export default async function SignInPage({ searchParams }: SignInPageProps) {
  const resolvedSearchParams = searchParams ? await searchParams : undefined;
  const requestHeaders = await headers();
  const canonicalLocalRedirect = getCanonicalLocalRedirectPath({
    hostHeader: requestHeaders.get('x-forwarded-host') ?? requestHeaders.get('host'),
    protocolHeader: requestHeaders.get('x-forwarded-proto'),
    pathname: '/sign-in',
    search: resolvedSearchParams?.redirect
      ? `redirect=${encodeURIComponent(resolvedSearchParams.redirect)}`
      : undefined,
  });

  if (canonicalLocalRedirect) {
    redirect(canonicalLocalRedirect);
  }

  const redirectTo = resolvedSearchParams?.redirect || '/dashboard';

  return <AuthPage redirectTo={redirectTo} />;
}
