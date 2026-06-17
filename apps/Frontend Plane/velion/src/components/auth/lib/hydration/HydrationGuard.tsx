/**
 * Hydration-safe wrapper component
 * Prevents server/client mismatch by only rendering content after hydration
 */

import React, { useSyncExternalStore, ReactNode } from 'react';

interface HydrationGuardProps {
  children: ReactNode;
  fallback?: ReactNode;
}

const subscribe = () => () => {};

function HydrationGuard({ children, fallback = null }: HydrationGuardProps) {
  const isHydrated = useSyncExternalStore(subscribe, () => true, () => false);
  return isHydrated ? <>{children}</> : <>{fallback}</>;
}

export function useIsHydrated() {
  return useSyncExternalStore(subscribe, () => true, () => false);
}
