import { cn } from '../lib/utils';
import { Loader2 } from 'lucide-react';

interface LoadingSpinnerProps {
  size?: 'sm' | 'md' | 'lg';
  className?: string;
  color?: 'default' | 'primary' | 'white';
}

const sizeClasses = {
  sm: 'w-4 h-4',
  md: 'w-6 h-6',
  lg: 'w-8 h-8',
};

const colorClasses = {
  default: 'text-gray-500',
  primary: 'text-primary',
  white: 'text-white',
};

export function LoadingSpinner({ 
  size = 'md', 
  className = '',
  color = 'default'
}: LoadingSpinnerProps) {
  return (
    <Loader2 
      className={cn(
        'animate-spin',
        sizeClasses[size],
        colorClasses[color],
        className
      )}
    />
  );
}

// Component for full page loading
interface LoadingPageProps {
  message?: string;
  className?: string;
}

export function LoadingPage({ 
  message = 'Loading...', 
  className = '' 
}: LoadingPageProps) {
  return (
    <div className={cn(
      'flex flex-col items-center justify-center min-h-[200px] p-8',
      className
    )}>
      <LoadingSpinner size="lg" className="mb-4" />
      <p className="text-gray-600 text-sm">{message}</p>
    </div>
  );
}

// Component for inline loading with text
interface LoadingTextProps {
  text?: string;
  size?: 'sm' | 'md';
  className?: string;
}

export function LoadingText({ 
  text = 'Loading...', 
  size = 'sm',
  className = '' 
}: LoadingTextProps) {
  return (
    <div className={cn('flex items-center gap-2', className)}>
      <LoadingSpinner size={size} />
      <span className={cn(
        'text-gray-600',
        size === 'sm' ? 'text-sm' : 'text-base'
      )}>
        {text}
      </span>
    </div>
  );
}

// Component for button loading state
interface LoadingButtonContentProps {
  text: string;
  loadingText?: string;
  isLoading: boolean;
  spinnerSize?: 'sm' | 'md';
  className?: string;
}

export function LoadingButtonContent({
  text,
  loadingText,
  isLoading,
  spinnerSize = 'sm',
  className = ''
}: LoadingButtonContentProps) {
  if (isLoading) {
    return (
      <div className={cn('flex items-center gap-2', className)}>
        <LoadingSpinner size={spinnerSize} color="white" />
        <span>{loadingText || text}</span>
      </div>
    );
  }

  return <span className={className}>{text}</span>;
}

export { LoadingSpinner as default };
