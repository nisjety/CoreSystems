'use client';

import * as React from 'react';
import { Toast } from '../ui/toast';

interface ToastContextType {
  toasts: Toast[];
  addToast: (toast: Omit<Toast, 'id'>) => void;
  dismissToast: (id: string) => void;
  clearToasts: () => void;
}

const ToastContext = React.createContext<ToastContextType | undefined>(undefined);

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = React.useState<Toast[]>([]);

  const addToast = React.useCallback((toast: Omit<Toast, 'id'>) => {
    const id = Math.random().toString(36).substr(2, 9);
    const newToast: Toast = {
      ...toast,
      id,
      duration: toast.duration ?? 5000, // 5 seconds default
    };
    
    setToasts((prev) => [...prev, newToast]);
  }, []);

  const dismissToast = React.useCallback((id: string) => {
    setToasts((prev) => prev.filter((toast) => toast.id !== id));
  }, []);

  const clearToasts = React.useCallback(() => {
    setToasts([]);
  }, []);

  const value = React.useMemo(
    () => ({
      toasts,
      addToast,
      dismissToast,
      clearToasts,
    }),
    [toasts, addToast, dismissToast, clearToasts]
  );

  return React.createElement(
    ToastContext.Provider,
    { value },
    children
  );
}

export function useToast() {
  const context = React.useContext(ToastContext);
  if (context === undefined) {
    throw new Error('useToast must be used within a ToastProvider');
  }
  return context;
}

// Convenience hook for common toast patterns
export function useToastActions() {
  const { addToast } = useToast();

  const success = React.useCallback(
    (title: string, description?: string) => {
      addToast({
        title,
        description,
        variant: 'success',
        duration: 4000,
      });
    },
    [addToast]
  );

  const error = React.useCallback(
    (title: string, description?: string) => {
      addToast({
        title,
        description,
        variant: 'error',
        duration: 6000,
      });
    },
    [addToast]
  );

  const warning = React.useCallback(
    (title: string, description?: string) => {
      addToast({
        title,
        description,
        variant: 'warning',
        duration: 5000,
      });
    },
    [addToast]
  );

  const info = React.useCallback(
    (title: string, description?: string) => {
      addToast({
        title,
        description,
        variant: 'info',
        duration: 4000,
      });
    },
    [addToast]
  );

  return { success, error, warning, info };
}
