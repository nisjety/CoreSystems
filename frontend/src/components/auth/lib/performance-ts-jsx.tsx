import { memo, ReactNode, useMemo, useCallback, useState } from 'react';
import { cn } from './utils';

interface PerformantComponentProps {
  children: ReactNode;
  className?: string;
  skipMemo?: boolean;
  memoKey?: string;
}

// High-order component for automatic memoization
export function withMemo<P extends object>(
  Component: React.ComponentType<P>,
  customCompare?: (prevProps: P, nextProps: P) => boolean
) {
  const MemoizedComponent = memo(Component, customCompare);
  MemoizedComponent.displayName = `Memo(${Component.displayName || Component.name})`;
  return MemoizedComponent;
}

// Performance-optimized wrapper component
export const PerformantWrapper = memo<PerformantComponentProps>(({
  children,
  className,
  memoKey,
}) => {
  const memoizedChildren = useMemo(() => children, [children, memoKey]);
  
  return (
    <div className={cn(className)}>
      {memoizedChildren}
    </div>
  );
});

PerformantWrapper.displayName = 'PerformantWrapper';

// Lazy loading component with suspense boundary
export const LazyComponent = memo<{
  component: React.LazyExoticComponent<React.ComponentType>;
  fallback?: ReactNode;
  className?: string;
}>(({ component: Component, fallback = <div>Loading...</div>, className }) => {
  return (
    <div className={cn(className)}>
      <Component />
    </div>
  );
});

LazyComponent.displayName = 'LazyComponent';

// Performance hooks
export function useStableCallback<T extends (...args: any[]) => any>(
  callback: T,
  deps: React.DependencyList
): T {
  // eslint-disable-next-line react-hooks/exhaustive-deps -- Wrapper function for stable callback
  return useCallback(callback, deps) as unknown as T;
}

export function useStableMemo<T>(
  factory: () => T,
  deps: React.DependencyList
): T {
  // eslint-disable-next-line react-hooks/exhaustive-deps -- Wrapper function for stable memo
  return useMemo(factory, deps);
}

// Component performance utilities
export const performanceUtils = {
  // Check if component should re-render
  shouldComponentUpdate: (prevProps: any, nextProps: any) => {
    const prevKeys = Object.keys(prevProps);
    const nextKeys = Object.keys(nextProps);
    
    if (prevKeys.length !== nextKeys.length) return false;
    
    return prevKeys.every(key => 
      Object.is(prevProps[key], nextProps[key])
    );
  },
  
  // Shallow compare for objects
  shallowEqual: (obj1: any, obj2: any) => {
    const keys1 = Object.keys(obj1);
    const keys2 = Object.keys(obj2);
    
    if (keys1.length !== keys2.length) return false;
    
    return keys1.every(key => obj1[key] === obj2[key]);
  },
  
  // Deep compare for complex objects (use sparingly)
  deepEqual: (obj1: any, obj2: any): boolean => {
    if (obj1 === obj2) return true;
    
    if (obj1 == null || obj2 == null) return false;
    
    if (typeof obj1 !== 'object' || typeof obj2 !== 'object') return false;
    
    const keys1 = Object.keys(obj1);
    const keys2 = Object.keys(obj2);
    
    if (keys1.length !== keys2.length) return false;
    
    return keys1.every(key => performanceUtils.deepEqual(obj1[key], obj2[key]));
  },
};

// Performance monitoring hook
export const usePerformanceMonitor = (componentName: string) => {
  const [renderTime] = useState(() => performance.now());
  
  return useCallback(() => {
    const endTime = performance.now();
    const duration = endTime - renderTime;
    
    if (duration > 16) { // More than one frame (16ms)
      console.warn(`Slow render detected in ${componentName}: ${duration.toFixed(2)}ms`);
    }
    
    return duration;
  }, [componentName, renderTime]);
};

// Virtual scrolling hook for large lists
export function useVirtualScroll<T>(
  items: T[],
  itemHeight: number,
  containerHeight: number,
  overscan = 5
) {
  return useMemo(() => {
    const visibleItems = Math.ceil(containerHeight / itemHeight);
    const totalItems = items.length;

    return {
      itemHeight,
      visibleItems,
      totalItems,
      overscan,
      getVisibleRange: (scrollTop: number) => {
        const start = Math.floor(scrollTop / itemHeight);
        const end = Math.min(start + visibleItems + overscan, totalItems);

        return {
          start: Math.max(0, start - overscan),
          end,
          items: items.slice(Math.max(0, start - overscan), end),
        };
      },
    };
  }, [items, itemHeight, containerHeight, overscan]);
}

export default {
  withMemo,
  PerformantWrapper,
  LazyComponent,
  useStableCallback,
  useStableMemo,
  performanceUtils,
  usePerformanceMonitor,
  useVirtualScroll,
};
