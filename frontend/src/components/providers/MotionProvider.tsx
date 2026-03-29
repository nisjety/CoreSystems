"use client";

import { LazyMotion, domAnimation } from 'framer-motion';
import { ReactNode } from 'react';

/**
 * Root-level LazyMotion provider.
 * By providing `domAnimation` features here once, all child components can
 * import `m` from framer-motion instead of `motion`, saving ~30KB from the
 * initial bundle (the full feature set is deferred until first interaction).
 */
export function MotionProvider({ children }: { children: ReactNode }) {
  return <LazyMotion features={domAnimation}>{children}</LazyMotion>;
}
