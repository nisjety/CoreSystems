"use client";

import { LazyMotion, MotionConfig, domAnimation, useReducedMotion } from 'framer-motion';
import { ReactNode, useEffect } from 'react';

/**
 * Root-level LazyMotion provider.
 * By providing `domAnimation` features here once, all child components can
 * import `m` from framer-motion instead of `motion`, saving ~30KB from the
 * initial bundle (the full feature set is deferred until first interaction).
 */
export function MotionProvider({ children }: { children: ReactNode }) {
  const prefersReducedMotion = useReducedMotion();

  useEffect(() => {
    document.documentElement.dataset.reducedMotion = prefersReducedMotion ? 'true' : 'false';

    return () => {
      delete document.documentElement.dataset.reducedMotion;
    };
  }, [prefersReducedMotion]);

  return (
    <MotionConfig reducedMotion="user">
      <LazyMotion features={domAnimation}>{children}</LazyMotion>
    </MotionConfig>
  );
}
