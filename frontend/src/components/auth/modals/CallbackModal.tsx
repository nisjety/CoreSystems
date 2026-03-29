'use client';

import { useEffect, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { toast } from 'sonner';
import { useAuthTranslation } from '../lib/i18n/hooks';
import { authORPCClient } from '../lib/orpc/client';
import { LoaderOne } from '../ui/loader';

interface CallbackState {
  isLoading: boolean;
  error: string | null;
  success: boolean;
}

interface CallbackModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export function CallbackModal({ isOpen, onClose }: CallbackModalProps) {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { authT } = useAuthTranslation();
  const [state, setState] = useState<CallbackState>({
    isLoading: true,
    error: null,
    success: false,
  });

  useEffect(() => {
    if (!isOpen) return;

    const handleCallback = async () => {
      try {
        setState({ isLoading: true, error: null, success: false });

        // Get redirect URL from search params
        const redirectTo = searchParams.get('redirectTo') || searchParams.get('redirect') || '/dashboard';
        const error = searchParams.get('error');
        const errorDescription = searchParams.get('error_description');

        // Check for OAuth errors
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
          
          // Close modal and show error state for a bit before returning to auth
          setTimeout(() => {
            onClose();
            // Clear URL params to return to clean auth page
            router.replace('/sign-in');
          }, 3000);
          return;
        }

        // Use oRPC client to get current user session
        const userProfile = await authORPCClient.getProfile();
        
        if (userProfile) {
          setState({ isLoading: false, error: null, success: true });
          toast.success(authT.callback('success'));

          // Redirect to the intended destination
          console.log('🔄 Redirecting to:', redirectTo);
          
          // Use setTimeout to ensure the success state is shown
          setTimeout(() => {
            onClose();
            router.push(redirectTo);
          }, 1500);
        } else {
          throw new Error('No user session found after authentication');
        }

      } catch (error) {
        console.error('❌ Callback error:', error);
        const errorMessage = authT.callback('error');
        setState({ isLoading: false, error: errorMessage, success: false });
        toast.error(errorMessage);
        
        // Close modal and return to auth page after error
        setTimeout(() => {
          onClose();
          router.replace('/sign-in');
        }, 3000);
      }
    };

    handleCallback();
  }, [isOpen, router, searchParams, authT, onClose]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-[300] flex items-center justify-center p-4">
      {/* Glass backdrop */}
      <div className="absolute inset-0 bg-black/20 backdrop-blur-sm" onClick={onClose} aria-hidden="true" />
      
      {/* Glass modal content */}
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Autentisering"
        className="relative bg-white/80 dark:bg-gray-900/80 backdrop-blur-md border border-white/20 rounded-2xl shadow-2xl max-w-md w-full p-8 text-center">
        {/* Error State */}
        {state.error && (
          <div>
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
        )}

        {/* Success State */}
        {state.success && (
          <div>
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
        )}

        {/* Loading State */}
        {state.isLoading && (
          <div>
            <div className="flex justify-center mb-6">
              <LoaderOne />
            </div>
            <h2 className="text-xl font-semibold text-gray-900 dark:text-white mb-3">
              {authT.callback('title')}
            </h2>
            <p className="text-gray-600 dark:text-gray-400">
              {authT.callback('processing')}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
