'use client';

import * as React from 'react';
import { X, CheckCircle, AlertCircle, Info, AlertTriangle } from 'lucide-react';
import { cn } from '../lib/utils';

export interface Toast {
  id: string;
  title?: string;
  description?: string;
  action?: React.ReactNode;
  variant?: 'default' | 'success' | 'error' | 'warning' | 'info';
  duration?: number;
}

interface ToastProps {
  toast: Toast;
  onDismiss: () => void;
}

const ToastIcon = ({ variant }: { variant: Toast['variant'] }) => {
  switch (variant) {
    case 'success':
      return <CheckCircle className="w-5 h-5 text-green-500" />;
    case 'error':
      return <AlertCircle className="w-5 h-5 text-red-500" />;
    case 'warning':
      return <AlertTriangle className="w-5 h-5 text-yellow-500" />;
    case 'info':
      return <Info className="w-5 h-5 text-blue-500" />;
    default:
      return null;
  }
};

export function ToastComponent({ toast, onDismiss }: ToastProps) {
  const [isVisible, setIsVisible] = React.useState(false);
  const [isExiting, setIsExiting] = React.useState(false);

  React.useEffect(() => {
    // Trigger entrance animation
    const timer = setTimeout(() => setIsVisible(true), 10);
    return () => clearTimeout(timer);
  }, []);

  const handleDismiss = React.useCallback(() => {
    setIsExiting(true);
    setTimeout(() => {
      onDismiss();
    }, 150);
  }, [onDismiss]);

  React.useEffect(() => {
    if (toast.duration && toast.duration > 0) {
      const timer = setTimeout(() => {
        handleDismiss();
      }, toast.duration);
      return () => clearTimeout(timer);
    }
  }, [toast.duration, handleDismiss]);

  const variants = {
    default: 'bg-background border-border',
    success: 'bg-green-50 border-green-200 dark:bg-green-900/20 dark:border-green-800',
    error: 'bg-red-50 border-red-200 dark:bg-red-900/20 dark:border-red-800',
    warning: 'bg-yellow-50 border-yellow-200 dark:bg-yellow-900/20 dark:border-yellow-800',
    info: 'bg-blue-50 border-blue-200 dark:bg-blue-900/20 dark:border-blue-800',
  };

  return (
    <div
      className={cn(
        'pointer-events-auto relative flex w-full max-w-sm items-center space-x-3 rounded-lg border p-4 shadow-lg transition-all duration-150',
        variants[toast.variant || 'default'],
        isVisible && !isExiting && 'translate-x-0 opacity-100',
        !isVisible && 'translate-x-full opacity-0',
        isExiting && 'translate-x-full opacity-0'
      )}
    >
      <ToastIcon variant={toast.variant} />
      
      <div className="flex-1 min-w-0">
        {toast.title && (
          <div className="text-sm font-medium text-foreground">
            {toast.title}
          </div>
        )}
        {toast.description && (
          <div className="text-xs text-muted-foreground mt-1">
            {toast.description}
          </div>
        )}
        {toast.action && (
          <div className="mt-2">
            {toast.action}
          </div>
        )}
      </div>

      <button
        onClick={handleDismiss}
        className="flex-shrink-0 rounded-md p-1 hover:bg-background/80 focus:outline-none focus:ring-2 focus:ring-ring"
      >
        <X className="w-4 h-4" />
      </button>
    </div>
  );
}

export function ToastViewport({ 
  toasts, 
  onDismiss 
}: { 
  toasts: Toast[]; 
  onDismiss: (id: string) => void;
}) {
  return (
    <div className="fixed top-0 right-0 z-[100] flex max-h-screen w-full flex-col-reverse space-y-2 space-y-reverse p-4 sm:bottom-0 sm:right-0 sm:top-auto sm:flex-col sm:space-y-2">
      {toasts.map((toast) => (
        <ToastComponent
          key={toast.id}
          toast={toast}
          onDismiss={() => onDismiss(toast.id)}
        />
      ))}
    </div>
  );
}
