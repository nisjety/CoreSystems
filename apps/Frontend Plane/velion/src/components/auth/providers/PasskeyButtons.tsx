'use client';

import { useState } from 'react';
import { usePasskey } from '../hooks/use-passkey';
import { useAuthTranslation } from '../lib/i18n/hooks';
import { Fingerprint, Key, Plus, Trash2, Shield, Loader2 } from 'lucide-react';

interface PasskeyCredential {
  id: string;
  name: string;
  created: Date;
  lastUsed: Date;
}

interface PasskeyButtonsProps {
  mode: 'register' | 'authenticate' | 'manage';
  onSuccess?: (credential?: PasskeyCredential) => void;
  onError?: (error: string) => void;
  className?: string;
  disabled?: boolean;
  showUnsupportedMessage?: boolean;
  email?: string;
  displayName?: string;
}

export function PasskeyButtons({
  mode,
  onSuccess,
  onError,
  className = '',
  disabled = false,
  showUnsupportedMessage = true,
  email = '',
  displayName,
}: PasskeyButtonsProps) {
  const { authT } = useAuthTranslation();
  const {
    isSupported,
    isLoading,
    credentials,
    register,
    authenticate,
    deleteCredential,
    error,
  } = usePasskey();

  const [deletingId, setDeletingId] = useState<string | null>(null);

  const handleRegister = async () => {
    if (!email) {
      onError?.(authT.passkey('emailRequired'));
      return;
    }
    
    try {
      await register(email, displayName);
      onSuccess?.();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : authT.passkeyError('registrationFailed');
      onError?.(errorMessage);
    }
  };

  const handleAuthenticate = async () => {
    try {
      await authenticate(email);
      onSuccess?.();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : authT.passkeyError('authenticationFailed');
      onError?.(errorMessage);
    }
  };

  const handleDelete = async (credentialId: string) => {
    if (!credentialId) return;
    
    setDeletingId(credentialId);
    try {
      await deleteCredential(credentialId);
      onSuccess?.();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : authT.passkeyError('deleteFailed');
      onError?.(errorMessage);
    } finally {
      setDeletingId(null);
    }
  };

  // Show unsupported message if requested and passkeys aren't supported
  if (!isSupported && showUnsupportedMessage) {
    return (
      <div className={`text-center p-4 border border-orange-200 rounded-lg bg-orange-50 ${className}`}>
        <Shield className="w-8 h-8 text-orange-500 mx-auto mb-2" />
        <p className="text-sm text-orange-700 font-medium mb-1">
          {authT.passkey('unsupported')}
        </p>
        <p className="text-xs text-orange-600">
          {authT.passkey('unsupportedMessage')}
        </p>
      </div>
    );
  }

  // Don't render anything if not supported and message is disabled
  if (!isSupported) {
    return null;
  }

  if (mode === 'register') {
    return (
      <div className={className}>
        <button
          type="button"
          onClick={handleRegister}
          disabled={disabled || isLoading || !email}
          className="w-full flex items-center justify-center gap-2 px-4 py-2.5 border border-dashed border-primary/50 rounded-lg hover:bg-primary/5 transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:opacity-50"
        >
          {isLoading ? (
            <>
              <Loader2 className="w-4 h-4 mr-2 animate-spin" />
              {authT.passkey('creating')}
            </>
          ) : (
            <>
              <Plus className="w-4 h-4 mr-2" />
              <span className="text-sm font-medium">{authT.passkey('register')}</span>
            </>
          )}
        </button>
        {error && (
          <p className="text-sm text-red-600 mt-2 text-center">{error}</p>
        )}
        {!email && (
          <p className="text-xs text-muted-foreground mt-1 text-center">
            {authT.passkey('emailRequired')}
          </p>
        )}
        <p className="text-xs text-muted-foreground mt-1 text-center">
          {authT.passkey('description')}
        </p>
      </div>
    );
  }

  if (mode === 'authenticate') {
    return (
      <div className={className}>
        <button
          type="button"
          onClick={handleAuthenticate}
          disabled={disabled || isLoading}
          className="w-full flex items-center justify-center gap-2 px-4 py-2.5 border border-border rounded-lg hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:opacity-50"
        >
          {isLoading ? (
            <>
              <Loader2 className="w-4 h-4 mr-2 animate-spin" />
              {authT.passkey('authenticating')}
            </>
          ) : (
            <>
              <Key className="w-4 h-4 mr-2" />
              <span className="text-sm font-medium">{authT.passkey('authenticate')}</span>
            </>
          )}
        </button>
        {error && (
          <p className="text-sm text-red-600 mt-2 text-center">{error}</p>
        )}
      </div>
    );
  }

  if (mode === 'manage') {
    return (
      <div className={`space-y-4 ${className}`}>
        <div className="flex items-center justify-between">
          <h3 className="text-lg font-medium">{authT.passkey('manage')}</h3>
          <button
            type="button"
            onClick={handleRegister}
            disabled={disabled || isLoading || !email}
            className="px-3 py-1.5 text-sm bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors disabled:opacity-50 focus:outline-none focus:ring-2 focus:ring-ring"
          >
            {isLoading ? (
              <>
                <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                {authT.passkey('creating')}
              </>
            ) : (
              <>
                <Plus className="w-4 h-4 mr-2" />
                {authT.passkey('register')}
              </>
            )}
          </button>
        </div>

        {credentials && credentials.length > 0 ? (
          <div className="space-y-2">
            {credentials.map((credential) => (
              <div
                key={credential.id}
                className="flex items-center justify-between p-3 border rounded-lg bg-gray-50"
              >
                <div className="flex items-center space-x-3">
                  <Key className="w-5 h-5 text-gray-500" />
                  <div>
                    <p className="font-medium text-sm">
                      {credential.name || authT.passkey('title')}
                    </p>
                    <p className="text-xs text-gray-500">
                      {authT.passkey('created')} {new Date(credential.created).toLocaleDateString()}
                    </p>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={!disabled && deletingId !== credential.id ? () => handleDelete(credential.id) : undefined}
                  disabled={disabled || deletingId === credential.id}
                  className="text-red-600 hover:text-red-700 hover:bg-red-50 p-2 rounded transition-colors focus:outline-none focus:ring-2 focus:ring-red-500"
                  aria-label={authT.passkey('delete')}
                >
                  {deletingId === credential.id ? (
                    <Loader2 className="w-4 h-4 animate-spin" />
                  ) : (
                    <Trash2 className="w-4 h-4" />
                  )}
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div className="text-center py-8 border-2 border-dashed border-gray-200 rounded-lg">
            <Key className="w-12 h-12 text-gray-400 mx-auto mb-4" />
            <p className="text-gray-500 font-medium mb-2">{authT.passkey('noPasskeys')}</p>
            <p className="text-sm text-gray-400 mb-4">
              {authT.passkey('noPasskeysDescription')}
            </p>
            <button
              type="button"
              onClick={handleRegister}
              disabled={disabled || isLoading || !email}
              className="px-4 py-2 bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors disabled:opacity-50 focus:outline-none focus:ring-2 focus:ring-ring"
            >
              {isLoading ? (
                <>
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                  {authT.passkey('creating')}
                </>
              ) : (
                <>
                  <Plus className="w-4 h-4 mr-2" />
                  {authT.passkey('register')}
                </>
              )}
            </button>
          </div>
        )}

        {error && (
          <div className="p-3 border border-red-200 rounded-lg bg-red-50">
            <p className="text-sm text-red-600">{error}</p>
          </div>
        )}
      </div>
    );
  }

  return null;
}

// Simplified component for quick passkey authentication
interface QuickPasskeyButtonProps {
  onSuccess?: () => void;
  onError?: (error: string) => void;
  disabled?: boolean;
  className?: string;
  email?: string;
}

function QuickPasskeyButton({
  onSuccess,
  onError,
  disabled = false,
  className = '',
  email,
}: QuickPasskeyButtonProps) {
  const { authT } = useAuthTranslation();
  const { isSupported, isLoading, authenticate } = usePasskey();

  if (!isSupported) {
    return null;
  }

  const handleQuickAuth = async () => {
    try {
      await authenticate(email);
      onSuccess?.();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : authT.passkeyError('authenticationFailed');
      onError?.(errorMessage);
    }
  };

  return (
    <button
      type="button"
      onClick={handleQuickAuth}
      disabled={disabled || isLoading}
      className={`text-blue-600 hover:text-blue-700 hover:bg-blue-50 px-3 py-1.5 rounded transition-colors focus:outline-none focus:ring-2 focus:ring-blue-500 ${className}`}
    >
      {isLoading ? (
        <>
          <Loader2 className="w-4 h-4 mr-2 animate-spin" />
          {authT.passkey('authenticating')}
        </>
      ) : (
        <>
          <Fingerprint className="w-4 h-4 mr-2" />
          {authT.passkey('quickAuth')}
        </>
      )}
    </button>
  );
}