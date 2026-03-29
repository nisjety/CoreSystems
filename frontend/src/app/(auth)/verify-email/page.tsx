'use client';

import { useEffect, useState } from 'react';
import { useSearchParams, useRouter } from 'next/navigation';
import { LoaderOne } from '../../../components/auth/ui/loader';

export default function VerifyEmailPage() {
  const searchParams = useSearchParams();
  const router = useRouter();
  const [status, setStatus] = useState<'loading' | 'success' | 'error'>('loading');
  const [message, setMessage] = useState('');

  useEffect(() => {
    const verifyEmail = async () => {
      try {
        const token = searchParams.get('token');
        const callbackURL = searchParams.get('callbackURL');
        
        if (!token) {
          setStatus('error');
          setMessage('Ingen verification token funnet i URL.');
          return;
        }

        console.log('🔍 Verifying email with token:', token);
        console.log('📍 Callback URL:', callbackURL);

        // Call the backend using the new oRPC endpoint for email verification
        const response = await fetch(`/api/auth/verifyEmail`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
          },
          body: JSON.stringify({
            token: token,
            callbackURL: callbackURL || '/dashboard'
          }),
          credentials: 'include',
        });

        if (response.ok) {
          setStatus('success');
          setMessage('E-post er verifisert! Redirecter til dashboard...');
          
          // Wait a moment then redirect to callback URL or dashboard
          setTimeout(() => {
            const redirectUrl = callbackURL && callbackURL.startsWith('/') 
              ? callbackURL 
              : '/dashboard';
            router.push(redirectUrl);
          }, 2000);
        } else {
          const errorText = await response.text();
          setStatus('error');
          setMessage(`Verifisering feilet: ${errorText}`);
        }
      } catch (error) {
        console.error('❌ Email verification error:', error);
        setStatus('error');
        setMessage('En feil oppstod under verifisering av e-post.');
      }
    };

    verifyEmail();
  }, [searchParams, router]);

  return (
    <div className="min-h-screen bg-background flex items-center justify-center p-4">
      <div className="max-w-md w-full bg-card rounded-lg border border-border p-6 text-center">
        {status === 'loading' && (
          <>
            <div className="flex justify-center mb-4">
              <LoaderOne />
            </div>
            <h1 className="text-xl font-semibold mb-2">Verifiserer e-post</h1>
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
            <h1 className="text-xl font-semibold text-green-600 mb-2">E-post verifisert!</h1>
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
            <h1 className="text-xl font-semibold text-red-600 mb-2">Verifisering feilet</h1>
            <p className="text-muted-foreground mb-4">{message}</p>
            <button
              onClick={() => router.push('/sign-in')}
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