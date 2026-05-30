/**
 * Performance API Polyfill
 * Wraps performance.mark() and performance.measure() to handle edge cases
 * that occur in Next.js 16 Webpack builds where timing can be misaligned
 */

if (typeof window !== 'undefined' && window.performance) {
  // Store original methods
  const originalMeasure = window.performance.measure.bind(window.performance);
  const originalMark = window.performance.mark.bind(window.performance);

  /**
   * Safe wrapper for performance.mark() that silently fails on error
   */
  window.performance.mark = function (name: string, ...args: any[]) {
    try {
      return originalMark(name, ...args);
    } catch (err) {
      // Silently suppress mark errors
      return undefined as any;
    }
  };

  /**
   * Safe wrapper for performance.measure() that handles:
   * - Missing start marks
   * - Negative timestamps
   * - Out-of-order timing
   */
  window.performance.measure = function (name: string, startOrOptions?: any, endMark?: string) {
    try {
      return originalMeasure(name, startOrOptions, endMark);
    } catch (err: any) {
      // Common Next.js 16 errors:
      // "Failed to execute 'measure' on 'Performance': 'RootPage' cannot have a negative time stamp."
      // "The start mark does not exist."
      
      if (
        err?.message?.includes('negative time stamp') ||
        err?.message?.includes('start mark does not exist') ||
        err?.message?.includes('cannot have a negative duration')
      ) {
        // Silently suppress - likely a timing issue with Next.js instrumentation
        return undefined as any;
      }
      
      // Re-throw other errors
      throw err;
    }
  };
}

export {};
