'use client';

import * as React from 'react';

export interface LoadingState {
  isLoading: boolean;
  progress?: number; // 0-100
  message?: string;
  stage?: string;
  error?: string | null;
}

export interface LoadingStateUpdate extends Partial<LoadingState> {
  id?: string;
}

interface LoadingContextType {
  globalLoading: LoadingState;
  loadingStates: Record<string, LoadingState>;
  setLoading: (id: string, state: LoadingStateUpdate) => void;
  setGlobalLoading: (state: LoadingStateUpdate) => void;
  clearLoading: (id: string) => void;
  clearAllLoading: () => void;
  isAnyLoading: () => boolean;
  getLoadingState: (id: string) => LoadingState;
}

const defaultLoadingState: LoadingState = {
  isLoading: false,
  progress: 0,
  message: undefined,
  stage: undefined,
  error: null,
};

const LoadingContext = React.createContext<LoadingContextType | undefined>(undefined);

export function LoadingProvider({ children }: { children: React.ReactNode }) {
  const [globalLoading, setGlobalLoadingState] = React.useState<LoadingState>(defaultLoadingState);
  const [loadingStates, setLoadingStates] = React.useState<Record<string, LoadingState>>({});

  const setLoading = React.useCallback((id: string, update: LoadingStateUpdate) => {
    setLoadingStates(prev => ({
      ...prev,
      [id]: {
        ...defaultLoadingState,
        ...prev[id],
        ...update,
      }
    }));
  }, []);

  const setGlobalLoading = React.useCallback((update: LoadingStateUpdate) => {
    setGlobalLoadingState(prev => ({
      ...prev,
      ...update,
    }));
  }, []);

  const clearLoading = React.useCallback((id: string) => {
    setLoadingStates(prev => {
      const newState = { ...prev };
      delete newState[id];
      return newState;
    });
  }, []);

  const clearAllLoading = React.useCallback(() => {
    setLoadingStates({});
    setGlobalLoadingState(defaultLoadingState);
  }, []);

  const isAnyLoading = React.useCallback(() => {
    if (globalLoading.isLoading) return true;
    return Object.values(loadingStates).some(state => state.isLoading);
  }, [globalLoading.isLoading, loadingStates]);

  const getLoadingState = React.useCallback((id: string): LoadingState => {
    return loadingStates[id] || defaultLoadingState;
  }, [loadingStates]);

  const value = React.useMemo(() => ({
    globalLoading,
    loadingStates,
    setLoading,
    setGlobalLoading,
    clearLoading,
    clearAllLoading,
    isAnyLoading,
    getLoadingState,
  }), [
    globalLoading,
    loadingStates,
    setLoading,
    setGlobalLoading,
    clearLoading,
    clearAllLoading,
    isAnyLoading,
    getLoadingState,
  ]);

  return React.createElement(
    LoadingContext.Provider,
    { value },
    children
  );
}

export function useLoading() {
  const context = React.useContext(LoadingContext);
  if (context === undefined) {
    throw new Error('useLoading must be used within a LoadingProvider');
  }
  return context;
}

// Convenience hook for managing specific loading operations
export function useLoadingState(id: string) {
  const { setLoading, clearLoading, getLoadingState } = useLoading();
  
  const state = getLoadingState(id);

  const startLoading = React.useCallback((message?: string, stage?: string) => {
    setLoading(id, {
      isLoading: true,
      message,
      stage,
      progress: 0,
      error: null,
    });
  }, [id, setLoading]);

  const updateProgress = React.useCallback((progress: number, message?: string, stage?: string) => {
    setLoading(id, {
      isLoading: true,
      progress: Math.max(0, Math.min(100, progress)),
      message,
      stage,
      error: null,
    });
  }, [id, setLoading]);

  const finishLoading = React.useCallback(() => {
    setLoading(id, {
      isLoading: false,
      progress: 100,
      error: null,
    });
    // Clear after a brief moment to show completion
    setTimeout(() => clearLoading(id), 500);
  }, [id, setLoading, clearLoading]);

  const setError = React.useCallback((error: string) => {
    setLoading(id, {
      isLoading: false,
      error,
    });
  }, [id, setLoading]);

  const reset = React.useCallback(() => {
    clearLoading(id);
  }, [id, clearLoading]);

  return {
    ...state,
    startLoading,
    updateProgress,
    finishLoading,
    setError,
    reset,
  };
}

// Hook for async operations with automatic loading state management
export function useAsyncOperation<T extends unknown[], R>(
  id: string,
  operation: (...args: T) => Promise<R>,
  options?: {
    onSuccess?: (result: R) => void;
    onError?: (error: Error) => void;
    successMessage?: string;
    errorMessage?: string;
    useToast?: boolean;
  }
) {
  const loadingState = useLoadingState(id);

  const execute = React.useCallback(async (...args: T): Promise<R | undefined> => {
    try {
      loadingState.startLoading();
      const result = await operation(...args);
      loadingState.finishLoading();
      
      if (options?.successMessage && options.useToast !== false) {
        console.log('Success:', options.successMessage);
        // Toast will be integrated when ToastProvider is added to the app
      }
      
      options?.onSuccess?.(result);
      return result;
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'An error occurred';
      loadingState.setError(errorMessage);
      
      if (options?.errorMessage && options.useToast !== false) {
        console.log('Error:', options.errorMessage, errorMessage);
        // Toast will be integrated when ToastProvider is added to the app
      }
      
      options?.onError?.(err instanceof Error ? err : new Error(errorMessage));
      return undefined;
    }
  }, [operation, loadingState, options]);

  return {
    execute,
    ...loadingState,
  };
}
