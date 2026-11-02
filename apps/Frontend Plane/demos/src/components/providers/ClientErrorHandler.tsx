'use client';

import { useEffect } from 'react';

export function ClientErrorHandler() {
  useEffect(() => {
    // Handle webpack module factory errors globally
    const handleGlobalError = (event: ErrorEvent) => {
      const error = event.error;
      
      // Check if this is a webpack module factory error
      if (
        error?.message?.includes("Cannot read properties of undefined (reading 'call')") ||
        error?.stack?.includes('webpack') ||
        error?.stack?.includes('__webpack_require__')
      ) {
        console.warn('Webpack module factory error detected, preventing crash:', error.message);
        
        // Prevent the error from crashing the app
        event.preventDefault();
        
        // Attempt to recover by reloading the page after a short delay
        setTimeout(() => {
          console.log('Attempting page reload to recover from webpack error...');
          window.location.reload();
        }, 1000);
        
        return false;
      }
    };

    // Handle unhandled promise rejections that might be webpack-related
    const handleUnhandledRejection = (event: PromiseRejectionEvent) => {
      const reason = event.reason;
      
      if (
        reason?.message?.includes("Cannot read properties of undefined (reading 'call')") ||
        reason?.stack?.includes('webpack') ||
        reason?.stack?.includes('__webpack_require__')
      ) {
        console.warn('Webpack promise rejection detected, preventing crash:', reason.message);
        
        // Prevent the unhandled rejection from crashing the app
        event.preventDefault();
        
        return false;
      }
    };

    // Add global error handlers
    window.addEventListener('error', handleGlobalError);
    window.addEventListener('unhandledrejection', handleUnhandledRejection);

    // Cleanup on unmount
    return () => {
      window.removeEventListener('error', handleGlobalError);
      window.removeEventListener('unhandledrejection', handleUnhandledRejection);
    };
  }, []);

  return null; // This component doesn't render anything
}
