'use client';

import { useEffect } from 'react';

export function ClientErrorHandler() {
  useEffect(() => {
    // Suppress chrome extension console errors globally
    const originalError = console.error;
    console.error = (...args: any[]) => {
      const message = args[0]?.toString() || '';
      
      // Suppress chrome extension communication errors
      if (
        message.includes('Could not establish connection') ||
        message.includes('Receiving end does not exist') ||
        message.includes('Unchecked runtime.lastError')
      ) {
        return; // Silently suppress
      }
      
      // Call original console.error for other messages
      originalError.apply(console, args);
    };

    // Handle webpack module factory errors globally
    const handleGlobalError = (event: ErrorEvent) => {
      const error = event.error;
      
      // Suppress chrome extension communication errors
      if (
        error?.message?.includes('Could not establish connection') ||
        error?.message?.includes('Receiving end does not exist')
      ) {
        return false; // Silently suppress
      }
      
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
      
      // Suppress chrome extension communication errors
      if (
        reason?.message?.includes('Could not establish connection') ||
        reason?.message?.includes('Receiving end does not exist')
      ) {
        return false; // Silently suppress
      }
      
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

    // Handle extension messages gracefully
    const handleMessage = (event: MessageEvent) => {
      // Silently ignore extension messages - they may fail if extension is not fully loaded
      // This prevents "Unchecked runtime.lastError" messages in the console
      if (event.data && typeof event.data === 'object') {
        // Just don't throw, let the message pass through silently
      }
    };

    // Add global error handlers
    window.addEventListener('error', handleGlobalError);
    window.addEventListener('unhandledrejection', handleUnhandledRejection);
    window.addEventListener('message', handleMessage, true); // Use capture phase

    // Cleanup on unmount
    return () => {
      window.removeEventListener('error', handleGlobalError);
      window.removeEventListener('unhandledrejection', handleUnhandledRejection);
      window.removeEventListener('message', handleMessage, true);
      console.error = originalError; // Restore original console.error
    };
  }, []);

  return null; // This component doesn't render anything
}
