'use client';

import { Suspense, useEffect } from 'react';
import { useSearchParams, useRouter } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { LoaderOne } from '../../../components/auth/ui/loader';

interface VerifyEmailResult {
  message: string;
  redirectUrl: string;
}

function getRedirectUrl(callbackURL: string | null) {
  return callbackURL && callbackURL.startsWith('/') ? callbackURL : '/dashboard';
}

function VerifyEmailContent() {
  const searchParams = useSearchParams();
  const router = useRouter();
  const token = searchParams.get('token');
  const callbackURL = searchParams.get('callbackURL');
  const redirectUrl = getRedirectUrl(callbackURL);

  const verificationQuery = useQuery<VerifyEmailResult>({
    queryKey: ['verify-email', token, redirectUrl],
    enabled: Boolean(token),
    retry: false,
    queryFn: async () => {
      const response = await fetch('/api/auth/verifyEmail', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          token,
          callbackURL: redirectUrl,
        }),
        credentials: 'include',
      });

      if (!response.ok) {
        const errorText = await response.text();
        throw new Error(`Verifisering feilet: ${errorText}`);
      }

      return {
        message: 'E-post er verifisert! Redirecter til dashboard...',
        redirectUrl,
      };
    },
  });

  useEffect(() => {
    if (!verificationQuery.isSuccess) {
      return;
    }

    const timeoutId = window.setTimeout(() => {
      router.push(verificationQuery.data.redirectUrl);
    }, 2000);

    return () => window.clearTimeout(timeoutId);
  }, [router, verificationQuery.data, verificationQuery.isSuccess]);

  const status: 'loading' | 'success' | 'error' = !token
    ? 'error'
    : verificationQuery.isPending
      ? 'loading'
      : verificationQuery.isSuccess
        ? 'success'
        : 'error';

  const message = !token
    ? 'Ingen verification token funnet i URL.'
    : verificationQuery.isSuccess
      ? verificationQuery.data.message
      : verificationQuery.error instanceof Error
        ? verificationQuery.error.message
        : 'En feil oppstod under verifisering av e-post.';

  return (
    <div className="min-h-screen bg-background flex items-center justify-center p-4">
      <div className="max-w-md w-full bg-card rounded-lg border border-border p-6 text-center">
        {status === 'loading' && (
          <>
            <div className="flex justify-center mb-4">
              <LoaderOne />
            </div>
            <h1
              className="mb-2 text-2xl font-normal leading-[1.05] tracking-normal"
              style={{ fontFamily: 'var(--font-geist-sans), Arial, sans-serif' }}
            >
              Verifiserer e-post
            </h1>
            <p className="text-muted-foreground">Vennligst vent...</p>
          </>
        )}
        
        {status === 'success' && (
          <>
            <div className="w-12 h-12 bg-green-100 rounded-full flex items-center justify-center mx-auto mb-4">
              <svg className="w-6 h-6 text-green-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
              </svg>
            </div>
            <h1
              className="mb-2 text-2xl font-normal leading-[1.05] tracking-normal text-green-600"
              style={{ fontFamily: 'var(--font-geist-sans), Arial, sans-serif' }}
            >
              E-post verifisert!
            </h1>
            <p className="text-muted-foreground">{message}</p>
          </>
        )}
        
        {status === 'error' && (
          <>
            <div className="w-12 h-12 bg-red-100 rounded-full flex items-center justify-center mx-auto mb-4">
              <svg className="w-6 h-6 text-red-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
              </svg>
            </div>
            <h1
              className="mb-2 text-2xl font-normal leading-[1.05] tracking-normal text-red-600"
              style={{ fontFamily: 'var(--font-geist-sans), Arial, sans-serif' }}
            >
              Verifisering feilet
            </h1>
            <p className="text-muted-foreground mb-4">{message}</p>
            <button
              onClick={() => router.push('/login')}
              className="bg-primary text-primary-foreground px-4 py-2 rounded-md hover:bg-primary/90 transition-colors"
            >
              Tilbake til innlogging
            </button>
          </>
        )}
      </div>
    </div>
  );
}

export default function VerifyEmailPage() {
  return (
    <Suspense fallback={<div className="min-h-screen bg-background flex items-center justify-center"><LoaderOne /></div>}>
      <VerifyEmailContent />
    </Suspense>
  );
}
