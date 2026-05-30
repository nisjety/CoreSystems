import React from 'react';
import {
  Loader2,
  CheckCircle,
  AlertCircle,
} from 'lucide-react';

import { useAuthTranslation } from '../lib/i18n/hooks';

// ── Toast Notification System ─────────────────────────────────────────────

export interface ToastNotification {
  id: string;
  type: 'success' | 'error' | 'info' | 'warning';
  title: string;
  description?: string;
  duration?: number;
}

class ToastManager {
  private static instance: ToastManager;
  private listeners: Set<(toast: ToastNotification) => void> = new Set();

  static getInstance(): ToastManager {
    if (!ToastManager.instance) {
      ToastManager.instance = new ToastManager();
    }
    return ToastManager.instance;
  }

  show(toast: Omit<ToastNotification, 'id'>): void {
    const notification: ToastNotification = {
      ...toast,
      id: `toast-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      duration: toast.duration || 5000,
    };
    this.listeners.forEach(listener => listener(notification));
    const message = `${toast.title}${toast.description ? `: ${toast.description}` : ''}`;
    switch (toast.type) {
      case 'success': console.log(`✅ Auth Success: ${message}`); break;
      case 'error': console.error(`❌ Auth Error: ${message}`); break;
      case 'warning': console.warn(`⚠️ Auth Warning: ${message}`); break;
      default: console.info(`ℹ️ Auth Info: ${message}`);
    }
  }

  subscribe(listener: (toast: ToastNotification) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
}

const toastManager = ToastManager.getInstance();

export const defaultToastActions = {
  success: (title: string, description?: string) => toastManager.show({ type: 'success', title, description }),
  error: (title: string, description?: string) => toastManager.show({ type: 'error', title, description }),
  info: (title: string, description?: string) => toastManager.show({ type: 'info', title, description }),
  warning: (title: string, description?: string) => toastManager.show({ type: 'warning', title, description }),
};

// ── Loading State ─────────────────────────────────────────────────────────

export interface LoadingState {
  submitting: boolean;
  validating: boolean;
  fieldStates: Record<string, 'idle' | 'validating' | 'valid' | 'invalid'>;
}

export const defaultLoadingState: LoadingState = {
  submitting: false,
  validating: false,
  fieldStates: {},
};

// ── Password Strength ─────────────────────────────────────────────────────

export type PasswordFeedbackKey = 'passwordMinLength' | 'passwordUpper' | 'passwordLower' | 'passwordNumber' | 'passwordSpecial';

export const calculatePasswordStrength = (password: string): { score: number; feedback: PasswordFeedbackKey[] } => {
  let score = 0;
  const feedback: PasswordFeedbackKey[] = [];
  if (password.length >= 8) score += 1; else feedback.push('passwordMinLength');
  if (/[A-Z]/.test(password)) score += 1; else feedback.push('passwordUpper');
  if (/[a-z]/.test(password)) score += 1; else feedback.push('passwordLower');
  if (/\d/.test(password)) score += 1; else feedback.push('passwordNumber');
  if (/[!@#$%^&*(),.?":{}|<>]/.test(password)) score += 1; else feedback.push('passwordSpecial');
  if (password.length >= 12) score += 1;
  return { score, feedback };
};

// ── PasswordStrengthIndicator ─────────────────────────────────────────────

interface PasswordStrengthIndicatorProps {
  password: string;
  authT: ReturnType<typeof useAuthTranslation>['authT'];
}

export function PasswordStrengthIndicator({ password, authT }: PasswordStrengthIndicatorProps) {
  const strength = calculatePasswordStrength(password);
  const strengthColors = ['bg-red-500', 'bg-orange-500', 'bg-yellow-500', 'bg-blue-500', 'bg-green-500'];
  const strengthLabels = [
    authT.passwordStrength('veryWeak'),
    authT.passwordStrength('weak'),
    authT.passwordStrength('ok'),
    authT.passwordStrength('strong'),
    authT.passwordStrength('veryStrong'),
  ];

  if (!password) return null;

  return (
    <div className="mt-2 space-y-2">
      <div className="flex space-x-1">
        {(['s1', 's2', 's3', 's4', 's5'] as const).map((id, i) => (
          <div
            key={id}
            className={`h-1 flex-1 rounded ${
              i < strength.score ? strengthColors[Math.min(strength.score - 1, 4)] : 'bg-muted'
            }`}
          />
        ))}
      </div>
      <div className="flex justify-between text-xs">
        <span className="text-muted-foreground">
          {strengthLabels[Math.min(strength.score - 1, 4)] || authT.passwordStrength('enterPassword')}
        </span>
        {strength.feedback.length > 0 && (
          <span className="text-muted-foreground">
            {authT.passwordStrength('missing')}: {strength.feedback
              .map(k => authT.validation(k))
              .join(', ')}
          </span>
        )}
      </div>
    </div>
  );
}

// ── FieldValidationIndicator ──────────────────────────────────────────────

interface FieldValidationIndicatorProps {
  validation?: { valid: boolean; warnings: string[] };
  fieldState?: 'idle' | 'validating' | 'valid' | 'invalid';
}

export function FieldValidationIndicator({ validation, fieldState }: FieldValidationIndicatorProps) {
  if (!validation || fieldState === 'idle') return null;

  return (
    <div className="absolute right-3 top-1/2 transform -translate-y-1/2">
      {fieldState === 'validating' && (
        <Loader2 className="w-4 h-4 animate-spin text-muted-foreground" />
      )}
      {fieldState === 'valid' && (
        <CheckCircle className="w-4 h-4 text-green-500" />
      )}
      {fieldState === 'invalid' && (
        <AlertCircle className="w-4 h-4 text-destructive" />
      )}
    </div>
  );
}

// ── FieldErrorDisplay ─────────────────────────────────────────────────────

interface FieldErrorDisplayProps {
  error?: string;
  warnings?: string[];
  showWarnings?: boolean;
  maxWarnings?: number;
}

export function FieldErrorDisplay({ error, warnings = [], showWarnings = true, maxWarnings }: FieldErrorDisplayProps) {
  const visibleWarnings = maxWarnings !== undefined ? warnings.slice(0, maxWarnings) : warnings;
  if (!error && (!showWarnings || visibleWarnings.length === 0)) return null;
  return (
    <div className="mt-1 space-y-1">
      {error && (
        <p className="text-sm text-destructive" role="alert">
          {error}
        </p>
      )}
      {showWarnings && visibleWarnings.length > 0 && (
        <div className="space-y-1">
          {visibleWarnings.map((warning) => (
            <p key={warning} className="text-xs text-orange-600" role="alert">
              • {warning}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}

// ── FormSubmitButton ──────────────────────────────────────────────────────

interface FormSubmitButtonProps {
  isLoading: boolean;
  loadingLabel: string;
  label: string;
}

export function FormSubmitButton({ isLoading, loadingLabel, label }: FormSubmitButtonProps) {
  return (
    <button
      type="submit"
      disabled={isLoading}
      className="w-full bg-primary hover:bg-primary/90 text-primary-foreground font-medium py-2.5 px-4 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
    >
      {isLoading ? (
        <div className="flex items-center justify-center gap-2">
          <Loader2 size={16} className="animate-spin" />
          <span>{loadingLabel}</span>
        </div>
      ) : (
        label
      )}
    </button>
  );
}
