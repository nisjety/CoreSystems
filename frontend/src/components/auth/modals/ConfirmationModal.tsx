'use client';

import { useEffect, useState, useCallback } from 'react';
import { 
  AlertTriangle, 
  CheckCircle, 
  Info, 
  X, 
  AlertCircle,
  Loader2
} from 'lucide-react';

// Enhanced features - fallback implementations when providers not available
const defaultToastActions = {
  success: (title: string, description?: string) => console.log('Confirmation Success:', title, description),
  error: (title: string, description?: string) => console.log('Confirmation Error:', title, description),
  info: (title: string, description?: string) => console.log('Confirmation Info:', title, description),
  warning: (title: string, description?: string) => console.log('Confirmation Warning:', title, description),
};

// Enhanced Norwegian translations for confirmation modals
const defaultConfirmationTranslations = {
  'confirmation.success.action': 'Handlingen ble fullført!',
  'confirmation.success.deleted': 'Elementet ble slettet',
  'confirmation.success.confirmed': 'Handlingen ble bekreftet',
  'confirmation.success.logout': 'Du ble logget ut',
  'confirmation.success.permission': 'Tillatelser ble oppdatert',
  'confirmation.success.cookies': 'Cookies ble slettet',
  'confirmation.error.action': 'En feil oppstod under utføring',
  'confirmation.error.network': 'Nettverksfeil - prøv igjen',
  'confirmation.error.permission': 'Du har ikke tillatelse til denne handlingen',
  'confirmation.opened': 'Bekreftelsesmodal åpnet',
  'confirmation.closed': 'Bekreftelsesmodal lukket',
  'confirmation.cancelled': 'Handlingen ble avbrutt',
};

const defaultT = (key: string): string => defaultConfirmationTranslations[key as keyof typeof defaultConfirmationTranslations] || key;

type ConfirmationVariant = 'warning' | 'danger' | 'info' | 'success';

interface ConfirmationModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void> | void;
  title: string;
  description: string;
  confirmText?: string;
  cancelText?: string;
  variant?: ConfirmationVariant;
  requiresConfirmation?: boolean;
  confirmationText?: string;
  className?: string;
  showCloseButton?: boolean;
  closeOnOverlayClick?: boolean;
  destructive?: boolean;
  isLoading?: boolean;
}

export function ConfirmationModal({
  isOpen,
  onClose,
  onConfirm,
  title,
  description,
  confirmText = 'Bekreft',
  cancelText = 'Avbryt',
  variant = 'warning',
  requiresConfirmation = false,
  confirmationText = 'bekreft',
  className = '',
  showCloseButton = true,
  closeOnOverlayClick = true,
  destructive = false,
  isLoading = false,
}: ConfirmationModalProps) {
  const [confirmationInput, setConfirmationInput] = useState('');
  const [isConfirming, setIsConfirming] = useState(false);

  // Enhanced modal state management with toast feedback
  useEffect(() => {
    if (isOpen) {
      setConfirmationInput('');
      defaultToastActions.info(defaultT('confirmation.opened'));
    }
  }, [isOpen]);

  // Enhanced close handler with toast feedback
  const handleClose = useCallback(() => {
    try {
      onClose();
      defaultToastActions.info(defaultT('confirmation.closed'));
    } catch (error) {
      console.error('Error closing confirmation modal:', error);
    }
  }, [onClose]);

  // Enhanced cancel handler with toast feedback
  const handleCancel = useCallback(() => {
    try {
      onClose();
      defaultToastActions.info(defaultT('confirmation.cancelled'));
    } catch (error) {
      console.error('Error cancelling confirmation:', error);
    }
  }, [onClose]);

  // Handle escape key with enhanced feedback
  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !isConfirming) {
        handleClose();
      }
    };

    if (isOpen) {
      document.addEventListener('keydown', handleEscape);
      document.body.style.overflow = 'hidden';
    }

    return () => {
      document.removeEventListener('keydown', handleEscape);
      document.body.style.overflow = 'unset';
    };
  }, [isOpen, isConfirming, handleClose]);

  const handleOverlayClick = (e: React.MouseEvent) => {
    if (closeOnOverlayClick && !isConfirming && e.target === e.currentTarget) {
      handleClose();
    }
  };

  const handleConfirm = async () => {
    if (requiresConfirmation && confirmationInput !== confirmationText) {
      return;
    }

    setIsConfirming(true);
    try {
      await onConfirm();
      // Enhanced success feedback with toast
      defaultToastActions.success(defaultT('confirmation.success.action'));
      onClose();
    } catch (error) {
      console.error('Confirmation action failed:', error);
      // Enhanced error feedback with toast
      defaultToastActions.error(
        defaultT('confirmation.error.action'),
        error instanceof Error ? error.message : 'Ukjent feil oppstod'
      );
    } finally {
      setIsConfirming(false);
    }
  };

  const getVariantConfig = () => {
    switch (variant) {
      case 'danger':
        return {
          icon: AlertTriangle,
          iconColor: 'text-red-500',
          bgColor: 'bg-red-50',
          borderColor: 'border-red-200',
          buttonVariant: 'destructive' as const,
        };
      case 'warning':
        return {
          icon: AlertCircle,
          iconColor: 'text-yellow-500',
          bgColor: 'bg-yellow-50',
          borderColor: 'border-yellow-200',
          buttonVariant: 'default' as const,
        };
      case 'info':
        return {
          icon: Info,
          iconColor: 'text-blue-500',
          bgColor: 'bg-blue-50',
          borderColor: 'border-blue-200',
          buttonVariant: 'default' as const,
        };
      case 'success':
        return {
          icon: CheckCircle,
          iconColor: 'text-green-500',
          bgColor: 'bg-green-50',
          borderColor: 'border-green-200',
          buttonVariant: 'default' as const,
        };
      default:
        return {
          icon: Info,
          iconColor: 'text-muted-foreground',
          bgColor: 'bg-muted',
          borderColor: 'border-border',
          buttonVariant: 'default' as const,
        };
    }
  };

  const config = getVariantConfig();
  const Icon = config.icon;

  const isConfirmDisabled = 
    isConfirming || 
    isLoading || 
    (requiresConfirmation && confirmationInput !== confirmationText);

  if (!isOpen) return null;

  return (
    <div 
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      onClick={handleOverlayClick}
    >
      {/* Backdrop */}
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" />
      
      {/* Modal matching reference design */}
      <div className={`relative bg-card rounded-2xl border border-border shadow-2xl w-full max-w-md overflow-hidden ${className}`}>
        {/* Header */}
        <div className="p-6 border-b border-border">
          <div className="flex items-start gap-4">
            <div className={`p-3 rounded-full ${config.bgColor} ${config.borderColor} border`}>
              <Icon className={`w-6 h-6 ${config.iconColor}`} />
            </div>
            
            <div className="flex-1">
              <div className="flex items-center justify-between">
                <h2 className="text-lg font-semibold text-foreground">{title}</h2>
                {showCloseButton && (
                  <button
                    onClick={handleClose}
                    disabled={isConfirming}
                    className="text-muted-foreground hover:text-foreground focus:outline-none focus:ring-2 focus:ring-ring rounded p-1"
                  >
                    <X className="w-4 h-4" />
                  </button>
                )}
              </div>
              <p className="mt-2 text-sm text-muted-foreground">
                {description}
              </p>
            </div>
          </div>
        </div>

        {/* Content */}
        <div className="p-6 space-y-4">
          {requiresConfirmation && (
            <div className="space-y-2">
              <label htmlFor="confirmation" className="text-sm font-medium text-foreground">
                Skriv <span className="font-mono bg-muted px-1 rounded text-foreground">{confirmationText}</span> for å bekrefte:
              </label>
              <input
                id="confirmation"
                type="text"
                value={confirmationInput}
                onChange={(e) => setConfirmationInput(e.target.value)}
                placeholder={confirmationText}
                disabled={isConfirming || isLoading}
                className="w-full px-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground font-mono"
              />
            </div>
          )}

          <div className="flex gap-3 justify-end">
            <button
              onClick={handleCancel}
              disabled={isConfirming || isLoading}
              className="px-4 py-2 border border-border rounded-lg hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-ring text-sm font-medium"
            >
              {cancelText}
            </button>
            
            <button
              onClick={handleConfirm}
              disabled={isConfirmDisabled}
              className={`px-4 py-2 text-sm font-medium rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-ring flex items-center gap-2 ${
                destructive 
                  ? 'bg-red-600 hover:bg-red-700 text-white focus:ring-red-500' 
                  : 'bg-primary hover:bg-primary/90 text-primary-foreground focus:ring-primary'
              } disabled:opacity-50`}
            >
              {isConfirming || isLoading ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  Behandler...
                </>
              ) : (
                confirmText
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// Preset confirmation modals for common actions - Norwegian text
interface DeleteConfirmationProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void> | void;
  itemName: string;
  itemType?: string;
  requiresConfirmation?: boolean;
  isLoading?: boolean;
}

export function DeleteConfirmationModal({
  isOpen,
  onClose,
  onConfirm,
  itemName,
  itemType = 'element',
  requiresConfirmation = true,
  isLoading = false,
}: DeleteConfirmationProps) {
  return (
    <ConfirmationModal
      isOpen={isOpen}
      onClose={onClose}
      onConfirm={onConfirm}
      title={`Slett ${itemType}`}
      description={`Er du sikker på at du vil slette "${itemName}"? Denne handlingen kan ikke angres.`}
      confirmText="Slett"
      cancelText="Avbryt"
      variant="danger"
      destructive={true}
      requiresConfirmation={requiresConfirmation}
      confirmationText="slett"
      isLoading={isLoading}
    />
  );
}

// Security action confirmation modal - Norwegian
interface SecurityConfirmationProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void> | void;
  action: string;
  description: string;
  requiresConfirmation?: boolean;
  isLoading?: boolean;
}

export function SecurityConfirmationModal({
  isOpen,
  onClose,
  onConfirm,
  action,
  description,
  requiresConfirmation = true,
  isLoading = false,
}: SecurityConfirmationProps) {
  return (
    <ConfirmationModal
      isOpen={isOpen}
      onClose={onClose}
      onConfirm={onConfirm}
      title="Sikkerhetshandling kreves"
      description={description}
      confirmText={`Bekreft ${action}`}
      cancelText="Avbryt"
      variant="warning"
      requiresConfirmation={requiresConfirmation}
      confirmationText="bekreft"
      isLoading={isLoading}
    />
  );
}

// Logout confirmation modal - Norwegian
interface LogoutConfirmationProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void> | void;
  isLoading?: boolean;
}

export function LogoutConfirmationModal({
  isOpen,
  onClose,
  onConfirm,
  isLoading = false,
}: LogoutConfirmationProps) {
  return (
    <ConfirmationModal
      isOpen={isOpen}
      onClose={onClose}
      onConfirm={onConfirm}
      title="Logg ut"
      description="Er du sikker på at du vil logge ut? Du må logge inn igjen for å få tilgang til kontoen din."
      confirmText="Logg ut"
      cancelText="Avbryt"
      variant="info"
      requiresConfirmation={false}
      isLoading={isLoading}
    />
  );
}

// Account deletion confirmation modal - Norwegian
interface AccountDeletionProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void> | void;
  userEmail: string;
  isLoading?: boolean;
}

export function AccountDeletionModal({
  isOpen,
  onClose,
  onConfirm,
  userEmail,
  isLoading = false,
}: AccountDeletionProps) {
  return (
    <ConfirmationModal
      isOpen={isOpen}
      onClose={onClose}
      onConfirm={onConfirm}
      title="Slett konto"
      description={`Dette vil permanent slette kontoen din (${userEmail}) og alle tilknyttede data. Denne handlingen kan ikke angres.`}
      confirmText="Slett konto"
      cancelText="Avbryt"
      variant="danger"
      destructive={true}
      requiresConfirmation={true}
      confirmationText="slett konto"
      isLoading={isLoading}
    />
  );
}

// Permission change confirmation modal - Norwegian
interface PermissionChangeProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void> | void;
  action: 'grant' | 'revoke';
  permission: string;
  targetUser: string;
  isLoading?: boolean;
}

export function PermissionChangeModal({
  isOpen,
  onClose,
  onConfirm,
  action,
  permission,
  targetUser,
  isLoading = false,
}: PermissionChangeProps) {
  const actionText = action === 'grant' ? 'Gi' : 'Fjern';
  
  return (
    <ConfirmationModal
      isOpen={isOpen}
      onClose={onClose}
      onConfirm={onConfirm}
      title={`${actionText} tillatelse`}
      description={`${actionText} ${permission} tillatelse ${action === 'grant' ? 'til' : 'fra'} ${targetUser}?`}
      confirmText={`${actionText} tillatelse`}
      cancelText="Avbryt"
      variant={action === 'revoke' ? 'warning' : 'info'}
      requiresConfirmation={action === 'revoke'}
      confirmationText={action === 'revoke' ? 'fjern' : undefined}
      isLoading={isLoading}
    />
  );
}

// Cookie deletion warning modal matching reference design
interface CookieDeletionModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => void;
}

export function CookieDeletionModal({
  isOpen,
  onClose,
  onConfirm,
}: CookieDeletionModalProps) {
  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-background/60 backdrop-blur-sm" onClick={onClose} aria-hidden="true" />
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Bekreftelse"
        className="relative bg-card text-card-foreground w-[92vw] max-w-md rounded-2xl border border-border shadow-2xl">
        {/* Header */}
        <div className="flex items-center gap-3 p-4 border-b border-border">
          <div className="w-8 h-8 rounded-full bg-muted flex items-center justify-center">
            <svg className="w-4 h-4 text-muted-foreground" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.732-.833-2.5 0L4.232 16.5c-.77.833.192 2.5 1.732 2.5z" />
            </svg>
          </div>
          <div>
            <h2 className="text-base font-semibold text-foreground">Slett cookies</h2>
            <p className="text-sm text-muted-foreground">Kan ikke angres</p>
          </div>
        </div>

        {/* Content */}
        <div className="p-4">
          <p className="text-sm text-foreground leading-relaxed">
            Alle lagrede data vil bli slettet og du må logge inn på nytt.
          </p>
        </div>

        {/* Actions */}
        <div className="flex gap-2 p-4 border-t border-border">
          <button
            onClick={onClose}
            className="flex-1 px-4 py-2 text-sm border border-border rounded-lg hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-ring"
          >
            Avbryt
          </button>
          <button
            onClick={() => {
              onConfirm();
              onClose();
            }}
            className="flex-1 px-4 py-2 text-sm bg-primary hover:bg-primary/90 text-primary-foreground rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
          >
            Slett
          </button>
        </div>
      </div>
    </div>
  );
}