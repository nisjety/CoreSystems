'use client';

import * as React from 'react';
import { Loader2 } from 'lucide-react';
import { cn } from '../lib/utils';

export interface LoadingSpinnerProps {
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

export function LoadingSpinner({ size = 'md', className }: LoadingSpinnerProps) {
  const sizeClasses = {
    sm: 'w-4 h-4',
    md: 'w-6 h-6',
    lg: 'w-8 h-8',
  };

  return (
    <Loader2 className={cn('animate-spin text-muted-foreground', sizeClasses[size], className)} />
  );
}

export interface ProgressBarProps {
  progress: number; // 0-100
  className?: string;
  showPercentage?: boolean;
  variant?: 'default' | 'success' | 'error' | 'warning';
}

export function ProgressBar({ 
  progress, 
  className, 
  showPercentage = false,
  variant = 'default' 
}: ProgressBarProps) {
  const clampedProgress = Math.max(0, Math.min(100, progress));

  const variants = {
    default: 'bg-primary',
    success: 'bg-green-500',
    error: 'bg-red-500',
    warning: 'bg-yellow-500',
  };

  return (
    <div className={cn('w-full', className)}>
      <div className="flex justify-between items-center mb-1">
        {showPercentage && (
          <span className="text-xs text-muted-foreground">{Math.round(clampedProgress)}%</span>
        )}
      </div>
      <div className="w-full bg-secondary rounded-full h-2">
        <div
          className={cn('h-2 rounded-full transition-all duration-300 ease-out', variants[variant])}
          style={{ width: `${clampedProgress}%` }}
        />
      </div>
    </div>
  );
}

export interface LoadingOverlayProps {
  isVisible: boolean;
  message?: string;
  stage?: string;
  progress?: number;
  className?: string;
}

export function LoadingOverlay({ 
  isVisible, 
  message, 
  stage, 
  progress,
  className 
}: LoadingOverlayProps) {
  if (!isVisible) return null;

  return (
    <div className={cn(
      'fixed inset-0 z-[9999] flex items-center justify-center bg-background/80 backdrop-blur-sm',
      className
    )}>
      <div className="bg-card rounded-lg shadow-lg p-6 max-w-sm w-full mx-4 border border-border">
        <div className="flex flex-col items-center space-y-4">
          <LoadingSpinner size="lg" />
          
          {stage && (
            <div className="text-sm font-medium text-foreground text-center">
              {stage}
            </div>
          )}
          
          {message && (
            <div className="text-xs text-muted-foreground text-center">
              {message}
            </div>
          )}
          
          {typeof progress === 'number' && (
            <ProgressBar 
              progress={progress} 
              showPercentage 
              className="w-full"
            />
          )}
        </div>
      </div>
    </div>
  );
}

export interface ButtonLoadingProps {
  isLoading: boolean;
  children: React.ReactNode;
  loadingText?: string;
  disabled?: boolean;
  className?: string;
  size?: 'sm' | 'md' | 'lg';
  variant?: 'default' | 'secondary' | 'outline' | 'ghost';
  type?: 'button' | 'submit' | 'reset';
  onClick?: () => void;
}

export function ButtonLoading({
  isLoading,
  children,
  loadingText,
  disabled,
  className,
  size = 'md',
  variant = 'default',
  type = 'button',
  onClick,
}: ButtonLoadingProps) {
  const sizeClasses = {
    sm: 'px-3 py-1.5 text-xs',
    md: 'px-4 py-2 text-sm',
    lg: 'px-6 py-3 text-base',
  };

  const variants = {
    default: 'bg-primary hover:bg-primary/90 text-primary-foreground',
    secondary: 'bg-secondary hover:bg-secondary/80 text-secondary-foreground',
    outline: 'border border-border hover:bg-accent hover:text-accent-foreground',
    ghost: 'hover:bg-accent hover:text-accent-foreground',
  };

  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled || isLoading}
      className={cn(
        'inline-flex items-center justify-center gap-2 rounded-lg font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:opacity-50 disabled:pointer-events-none',
        sizeClasses[size],
        variants[variant],
        className
      )}
    >
      {isLoading && <LoadingSpinner size="sm" className="text-current" />}
      {isLoading ? loadingText || children : children}
    </button>
  );
}

export interface SkeletonProps {
  className?: string;
  lines?: number;
  width?: string | number;
  height?: string | number;
}

export function Skeleton({ 
  className, 
  lines = 1, 
  width = '100%', 
  height = '1rem' 
}: SkeletonProps) {
  if (lines === 1) {
    return (
      <div
        className={cn(
          'animate-pulse bg-muted rounded',
          className
        )}
        style={{ width, height }}
      />
    );
  }

  return (
    <div className={cn('space-y-2', className)}>
      {Array.from({ length: lines }).map((_, index) => (
        <div
          key={index}
          className="animate-pulse bg-muted rounded"
          style={{ 
            width: index === lines - 1 ? '75%' : width, 
            height 
          }}
        />
      ))}
    </div>
  );
}
