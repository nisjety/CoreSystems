'use client';

import { useEffect, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { toast } from 'sonner';
import { useAuthTranslation } from '../../../../components/auth/lib/i18n/hooks';
import { authORPCClient } from '../../../../components/auth/lib/orpc/client';
import { LoaderOne } from '../../../../components/auth/ui/loader';
import { onboardingService } from '@/components/onboarding/services/onboarding-service';

interface CallbackState {
  isLoading: boolean;
  error: string | null;
  success: boolean;
}

async function waitForProfile() {
  let lastError: unknown;

  for (let attempt = 0; attempt < 6; attempt += 1) {
    try {
      return await authORPCClient.getProfile();
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => window.setTimeout(resolve, 400 * (attempt + 1)));
    }
  }

  throw lastError instanceof Error ? lastError : new Error('No authenticated user');
}

export default function AuthCallbackClient() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { authT } = useAuthTranslation();
  const [state, setState] = useState<CallbackState>({
    isLoading: true,
    error: null,
    success: false,
  });

  useEffect(() => {
    const handleCallback = async () => {
      try {
        setState({ isLoading: true, error: null, success: false });

        const redirectTo =
          searchParams.get('redirectTo') ||
          searchParams.get('redirect') ||
          '/dashboard';
        const error = searchParams.get('error');
        const errorDescription = searchParams.get('error_description');

        if (error) {
          console.error('OAuth Error:', error, errorDescription);

          let errorMessage = authT.callback('error');
          if (error === 'access_denied') {
            errorMessage = authT.callbackOAuth('denied');
          } else if (error === 'cancelled') {
            errorMessage = authT.callbackOAuth('cancelled');
          } else if (error === 'timeout') {
            errorMessage = authT.callbackOAuth('timeout');
          }

          setState({ isLoading: false, error: errorMessage, success: false });
          toast.error(`${authT.callbackOAuth('error')}: ${errorDescription || errorMessage}`);

          setTimeout(() => {
            router.push('/sign-in');
          }, 3000);
          return;
        }

        const userProfile = await waitForProfile();

        if (userProfile) {
          setState({ isLoading: false, error: null, success: true });
          toast.success(authT.callback('success'));

          const needsOnboarding = await onboardingService.needsOnboarding();

          let finalRedirect = redirectTo === '/' ? '/dashboard' : redirectTo;
          if (needsOnboarding) {
            await onboardingService.startOnboarding();
            finalRedirect = '/onboarding/profile';
            toast.info('Welcome! Let\'s set up your account');
          }

          console.log('🔄 Redirecting to:', finalRedirect);

          setTimeout(() => {
            router.push(finalRedirect);
          }, 1500);
        } else {
          throw new Error('No user session found after authentication');
        }
      } catch (error) {
        console.error('❌ Callback error:', error);
        const errorMessage = authT.callback('error');
        setState({ isLoading: false, error: errorMessage, success: false });
        toast.error(errorMessage);

        setTimeout(() => {
          router.push('/sign-in');
        }, 3000);
      }
    };

    void handleCallback();
  }, [router, searchParams, authT]);

  if (state.error) {
    return (
      <div className="fixed inset-0 flex items-center justify-center p-4 z-50">
        <div className="bg-white/80 dark:bg-gray-900/80 backdrop-blur-md border border-white/20 rounded-2xl shadow-2xl max-w-md w-full p-8 text-center">
          <div className="w-16 h-16 mx-auto mb-6 text-red-500">
            <svg fill="none" stroke="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L3.732 16.5c-.77.833.192 2.5 1.732 2.5z" />
            </svg>
          </div>
          <h2 className="text-xl font-semibold text-red-900 dark:text-red-100 mb-3">
            {authT.callback('error')}
          </h2>
          <p className="text-red-700 dark:text-red-200 mb-4">
            {state.error}
          </p>
          <p className="text-sm text-red-600 dark:text-red-300">
            {authT.callback('redirectingToSignIn')}
          </p>
        </div>
      </div>
    );
  }

  if (state.success) {
    return (
      <div className="fixed inset-0 flex items-center justify-center p-4 z-50">
        <div className="bg-white/80 dark:bg-gray-900/80 backdrop-blur-md border border-white/20 rounded-2xl shadow-2xl max-w-md w-full p-8 text-center">
          <div className="w-16 h-16 mx-auto mb-6 text-green-500">
            <svg fill="none" stroke="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
            </svg>
          </div>
          <h2 className="text-xl font-semibold text-green-900 dark:text-green-100 mb-3">
            {authT.callback('success')}
          </h2>
          <p className="text-green-700 dark:text-green-200">
            {authT.callback('redirecting')}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="fixed inset-0 flex items-center justify-center p-4 z-50">
      <div className="bg-white/80 dark:bg-gray-900/80 backdrop-blur-md border border-white/20 rounded-2xl shadow-2xl max-w-md w-full p-8 text-center">
        <div className="flex justify-center mb-6">
          <LoaderOne />
        </div>
        <h2 className="text-xl font-semibold text-gray-900 dark:text-white mb-3">
          Completing sign in
        </h2>
        <p className="text-gray-600 dark:text-gray-400">
          Please wait while we complete your authentication...
        </p>
      </div>
    </div>
  );
}
