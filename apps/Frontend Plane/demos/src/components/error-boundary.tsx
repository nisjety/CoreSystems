'use client';

import React from 'react';

interface ErrorBoundaryState {
  hasError: boolean;
  error?: Error;
  errorInfo?: React.ErrorInfo;
}

interface ErrorBoundaryProps {
  children: React.ReactNode;
  fallback?: React.ComponentType<{ error: Error; reset: () => void }>;
}

class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    // Check if this is a webpack module factory error
    const isWebpackModuleError = 
      error.message?.includes("Cannot read properties of undefined (reading 'call')") ||
      error.stack?.includes('webpack') ||
      error.stack?.includes('__webpack_require__');

    if (isWebpackModuleError) {
      console.warn('Webpack module factory error detected, attempting recovery...');
    }

    return {
      hasError: true,
      error,
    };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    this.setState({
      error,
      errorInfo,
    });

    // Log webpack module factory errors for debugging
    if (error.message?.includes("Cannot read properties of undefined (reading 'call')")) {
      console.error('TanStack Query webpack module factory error:', {
        error: error.message,
        stack: error.stack,
        componentStack: errorInfo.componentStack,
      });
    }
  }

  handleReset = () => {
    this.setState({ hasError: false, error: undefined, errorInfo: undefined });
  };

  render() {
    if (this.state.hasError && this.state.error) {
      // Use custom fallback if provided
      if (this.props.fallback) {
        const Fallback = this.props.fallback;
        return <Fallback error={this.state.error} reset={this.handleReset} />;
      }

      // Default fallback for webpack module factory errors
      if (this.state.error.message?.includes("Cannot read properties of undefined (reading 'call')")) {
        return (
          <div className="flex min-h-screen items-center justify-center bg-background">
            <div className="mx-auto max-w-md rounded-lg border bg-card p-6 text-center shadow-lg">
              <h2 className="mb-4 text-xl font-semibold text-destructive">
                Loading Error
              </h2>
              <p className="mb-4 text-sm text-muted-foreground">
                A module loading error occurred. This is usually temporary.
              </p>
              <button
                onClick={this.handleReset}
                className="inline-flex h-10 items-center justify-center rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
              >
                Try Again
              </button>
              <button
                onClick={() => window.location.reload()}
                className="ml-2 inline-flex h-10 items-center justify-center rounded-md border border-input bg-background px-4 py-2 text-sm font-medium hover:bg-accent hover:text-accent-foreground"
              >
                Reload Page
              </button>
            </div>
          </div>
        );
      }

      // Generic error fallback
      return (
        <div className="flex min-h-screen items-center justify-center bg-background">
          <div className="mx-auto max-w-md rounded-lg border bg-card p-6 text-center shadow-lg">
            <h2 className="mb-4 text-xl font-semibold text-destructive">
              Something went wrong
            </h2>
            <details className="mb-4 text-left">
              <summary className="cursor-pointer text-sm font-medium">
                Error details
              </summary>
              <pre className="mt-2 overflow-auto text-xs text-muted-foreground">
                {this.state.error.message}
              </pre>
            </details>
            <button
              onClick={this.handleReset}
              className="inline-flex h-10 items-center justify-center rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
            >
              Try Again
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}

export default ErrorBoundary;
