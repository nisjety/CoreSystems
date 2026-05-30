'use client';

// Import polyfill immediately when this module loads (before React renders)
import '@/lib/utils/performance-polyfill';

import { useEffect } from 'react';

/**
 * Performance Polyfill Provider
 * Wraps performance.mark() and performance.measure() to handle
 * Next.js 16 Webpack timing issues gracefully.
 */
export function PerformancePolyfillProvider() {
  useEffect(() => {
    // The polyfill is already applied when this module is imported
    // This component exists to ensure the client-side import happens
  }, []);

  return null;
}
