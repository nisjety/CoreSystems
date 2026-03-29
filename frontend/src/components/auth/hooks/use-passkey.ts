import { useState, useCallback, useEffect } from 'react';
import { detectWebAuthnSupport } from '../utils/feature-detection';

interface PasskeyCredential {
  id: string;
  name: string;
  created: Date;
  lastUsed: Date;
}

interface UsePasskeyReturn {
  isSupported: boolean;
  isLoading: boolean;
  error: string | null;
  credentials: PasskeyCredential[];
  register: (email: string, displayName?: string) => Promise<void>;
  authenticate: (email?: string) => Promise<void>;
  deleteCredential: (credentialId: string) => Promise<void>;
  refreshCredentials: () => Promise<void>;
  clearError: () => void;
}

interface PasskeyOptions {
  rpId?: string;
  rpName?: string;
  timeout?: number;
  userVerification?: UserVerificationRequirement;
}

export function usePasskey(options: PasskeyOptions = {}): UsePasskeyReturn {
  const [isSupported, setIsSupported] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [credentials, setCredentials] = useState<PasskeyCredential[]>([]);

  const {
    rpId = typeof window !== 'undefined' ? window.location.hostname : 'localhost',
    rpName = 'ID-Knuten Auth Service',
    timeout = 60000,
    userVerification = 'preferred',
  } = options;

  // Check WebAuthn support on mount
  useEffect(() => {
    setIsSupported(detectWebAuthnSupport());
  }, []);

  const clearError = useCallback(() => {
    setError(null);
  }, []);

  const generateUserId = useCallback(() => {
    return new Uint8Array(32).map(() => Math.floor(Math.random() * 256));
  }, []);

  const base64ToBuffer = useCallback((base64: string): ArrayBuffer => {
    const binaryString = atob(base64);
    const bytes = new Uint8Array(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
      bytes[i] = binaryString.charCodeAt(i);
    }
    return bytes.buffer;
  }, []);

  const register = useCallback(async (email: string, displayName?: string) => {
    if (!isSupported) {
      setError('WebAuthn is not supported in this browser');
      return;
    }

    setIsLoading(true);
    setError(null);

    try {
      const userId = generateUserId();
      
      const publicKeyCredentialCreationOptions: PublicKeyCredentialCreationOptions = {
        challenge: crypto.getRandomValues(new Uint8Array(32)),
        rp: {
          name: rpName,
          id: rpId,
        },
        user: {
          id: userId,
          name: email,
          displayName: displayName || email,
        },
        pubKeyCredParams: [
          { alg: -7, type: 'public-key' }, // ES256
          { alg: -257, type: 'public-key' }, // RS256
        ],
        authenticatorSelection: {
          authenticatorAttachment: 'platform',
          userVerification,
          residentKey: 'preferred',
        },
        timeout,
        attestation: 'none',
      };

      const credential = await navigator.credentials.create({
        publicKey: publicKeyCredentialCreationOptions,
      }) as PublicKeyCredential;

      if (!credential) {
        throw new Error('Failed to create credential');
      }

      // Here you would typically send the credential to your server
      // For now, we'll store it locally for demo purposes
      const newCredential: PasskeyCredential = {
        id: credential.id,
        name: displayName || email,
        created: new Date(),
        lastUsed: new Date(),
      };

      setCredentials(prev => [...prev, newCredential]);
      
      // In a real implementation, you'd send this to your server:
      // const response = credential.response as AuthenticatorAttestationResponse;
      // await fetch('/api/passkey/register', {
      //   method: 'POST',
      //   headers: { 'Content-Type': 'application/json' },
      //   body: JSON.stringify({
      //     id: credential.id,
      //     rawId: bufferToBase64(credential.rawId),
      //     response: {
      //       attestationObject: bufferToBase64(response.attestationObject),
      //       clientDataJSON: bufferToBase64(response.clientDataJSON),
      //     },
      //     type: credential.type,
      //   }),
      // });

    } catch (err) {
      console.error('Passkey registration error:', err);
      if (err instanceof Error) {
        if (err.name === 'NotAllowedError') {
          setError('Passkey registration was cancelled or timed out');
        } else if (err.name === 'InvalidStateError') {
          setError('A passkey for this account already exists');
        } else {
          setError(`Registration failed: ${err.message}`);
        }
      } else {
        setError('An unknown error occurred during registration');
      }
    } finally {
      setIsLoading(false);
    }
  }, [isSupported, rpId, rpName, timeout, userVerification, generateUserId]);

  const authenticate = useCallback(async () => {
    if (!isSupported) {
      setError('WebAuthn is not supported in this browser');
      return;
    }

    setIsLoading(true);
    setError(null);

    try {
      const publicKeyCredentialRequestOptions: PublicKeyCredentialRequestOptions = {
        challenge: crypto.getRandomValues(new Uint8Array(32)),
        timeout,
        userVerification,
        rpId,
      };

      // If we have stored credentials, we can specify allowCredentials
      if (credentials.length > 0) {
        publicKeyCredentialRequestOptions.allowCredentials = credentials.map(cred => ({
          id: base64ToBuffer(cred.id),
          type: 'public-key',
        }));
      }

      const credential = await navigator.credentials.get({
        publicKey: publicKeyCredentialRequestOptions,
      }) as PublicKeyCredential;

      if (!credential) {
        throw new Error('Authentication cancelled');
      }

      // Update last used time for the credential
      setCredentials(prev => 
        prev.map(cred => 
          cred.id === credential.id 
            ? { ...cred, lastUsed: new Date() }
            : cred
        )
      );

      // In a real implementation, you'd send this to your server:
      // const response = credential.response as AuthenticatorAssertionResponse;
      // await fetch('/api/passkey/authenticate', {
      //   method: 'POST',
      //   headers: { 'Content-Type': 'application/json' },
      //   body: JSON.stringify({
      //     id: credential.id,
      //     rawId: bufferToBase64(credential.rawId),
      //     response: {
      //       authenticatorData: bufferToBase64(response.authenticatorData),
      //       clientDataJSON: bufferToBase64(response.clientDataJSON),
      //       signature: bufferToBase64(response.signature),
      //       userHandle: response.userHandle ? bufferToBase64(response.userHandle) : null,
      //     },
      //     type: credential.type,
      //   }),
      // });

    } catch (err) {
      console.error('Passkey authentication error:', err);
      if (err instanceof Error) {
        if (err.name === 'NotAllowedError') {
          setError('Authentication was cancelled or timed out');
        } else {
          setError(`Authentication failed: ${err.message}`);
        }
      } else {
        setError('An unknown error occurred during authentication');
      }
    } finally {
      setIsLoading(false);
    }
  }, [isSupported, timeout, userVerification, rpId, credentials, base64ToBuffer]);

  const deleteCredential = useCallback(async (credentialId: string) => {
    setIsLoading(true);
    setError(null);

    try {
      // In a real implementation, you'd call your server to delete the credential
      // await fetch(`/api/passkey/credentials/${credentialId}`, { method: 'DELETE' });
      
      setCredentials(prev => prev.filter(cred => cred.id !== credentialId));
    } catch (err) {
      console.error('Error deleting credential:', err);
      setError('Failed to delete passkey');
    } finally {
      setIsLoading(false);
    }
  }, []);

  const refreshCredentials = useCallback(async () => {
    setIsLoading(true);
    setError(null);

    try {
      // In a real implementation, you'd fetch credentials from your server
      // const response = await fetch('/api/passkey/credentials');
      // const creds = await response.json();
      // setCredentials(creds);
      
      // For now, we'll just use what's stored locally
    } catch (err) {
      console.error('Error refreshing credentials:', err);
      setError('Failed to refresh passkeys');
    } finally {
      setIsLoading(false);
    }
  }, []);

  return {
    isSupported,
    isLoading,
    error,
    credentials,
    register,
    authenticate,
    deleteCredential,
    refreshCredentials,
    clearError,
  };
}
