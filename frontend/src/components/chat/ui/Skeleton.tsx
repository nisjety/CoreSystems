'use client';

import { m } from 'framer-motion';

interface SkeletonProps {
  className?: string;
  variant?: 'card' | 'text' | 'circle' | 'rectangle';
  lines?: number;
  animate?: boolean;
}

export function Skeleton({ 
  className = '', 
  variant = 'rectangle', 
  lines = 1,
  animate = true 
}: SkeletonProps) {
  const baseClasses = 'bg-gradient-to-r from-gray-200 via-gray-100 to-gray-200 rounded';
  
  const animationProps = animate ? {
    animate: {
      backgroundPosition: ['200% 0', '-200% 0'],
    },
    transition: {
      duration: 2,
      repeat: Infinity,
      ease: 'linear' as const,
    },
    style: {
      backgroundSize: '200% 100%',
    }
  } : {};

  if (variant === 'card') {
    return (
      <m.div 
        className={`${baseClasses} p-6 space-y-4 ${className}`}
        {...animationProps}
      >
        {/* Header */}
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 bg-gray-300 rounded-xl" />
          <div className="space-y-2 flex-1">
            <div className="h-4 bg-gray-300 rounded w-1/3" />
            <div className="h-3 bg-gray-300 rounded w-1/2" />
          </div>
        </div>
        
        {/* Content lines */}
        <div className="space-y-3">
          {Array.from({ length: lines }).map((_, i) => (
            <div key={i} className="space-y-2">
              <div className="h-4 bg-gray-300 rounded w-full" />
              <div className="h-3 bg-gray-300 rounded w-3/4" />
            </div>
          ))}
        </div>
        
        {/* Footer */}
        <div className="flex justify-between pt-2">
          <div className="h-3 bg-gray-300 rounded w-1/4" />
          <div className="h-3 bg-gray-300 rounded w-1/5" />
        </div>
      </m.div>
    );
  }

  if (variant === 'circle') {
    return (
      <m.div 
        className={`${baseClasses} rounded-full ${className}`}
        {...animationProps}
      />
    );
  }

  if (variant === 'text') {
    return (
      <div className="space-y-2">
        {Array.from({ length: lines }).map((_, i) => (
          <m.div 
            key={i}
            className={`${baseClasses} h-4 ${i === lines - 1 ? 'w-3/4' : 'w-full'} ${className}`}
            {...animationProps}
          />
        ))}
      </div>
    );
  }

  return (
    <m.div 
      className={`${baseClasses} ${className}`}
      {...animationProps}
    />
  );
}

interface CardSkeletonProps {
  title?: string;
  lines?: number;
  className?: string;
  showHeader?: boolean;
  showFooter?: boolean;
}

export function CardSkeleton({ 
  title, 
  lines = 3, 
  className = '',
  showHeader = true,
  showFooter = true 
}: CardSkeletonProps) {
  return (
    <m.div 
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      className={`bg-white border border-gray-100 rounded-2xl shadow-[0_4px_24px_rgba(0,0,0,0.08)] p-6 ${className}`}
    >
      {showHeader && (
        <div className="flex items-center justify-between mb-6">
          <div className="flex items-center gap-2">
            <Skeleton variant="circle" className="w-10 h-10" />
            <div className="space-y-2">
              <Skeleton className="h-4 w-24" />
              {title && <div className="text-xs text-gray-500">{title}</div>}
            </div>
          </div>
          <Skeleton variant="circle" className="w-8 h-8" />
        </div>
      )}

      <div className="space-y-4">
        {Array.from({ length: lines }).map((_, i) => (
          <m.div
            key={i}
            initial={{ opacity: 0, x: -20 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{ delay: i * 0.1 }}
            className="bg-gray-50 rounded-xl p-4 space-y-3"
          >
            <div className="flex gap-3">
              <Skeleton className="w-16 h-12 rounded-lg" />
              <div className="flex-1 space-y-2">
                <Skeleton className="h-4 w-full" />
                <Skeleton className="h-3 w-3/4" />
                <div className="flex items-center gap-2 mt-2">
                  <Skeleton className="h-3 w-16" />
                  <span className="text-gray-300">•</span>
                  <Skeleton className="h-3 w-20" />
                </div>
              </div>
            </div>
          </m.div>
        ))}
      </div>

      {showFooter && (
        <div className="mt-6 pt-4 border-t border-gray-100">
          <div className="flex items-center justify-between">
            <Skeleton className="h-3 w-20" />
            <Skeleton className="h-3 w-24" />
          </div>
        </div>
      )}
    </m.div>
  );
}
