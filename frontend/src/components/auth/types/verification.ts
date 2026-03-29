export type VerificationMethod = 'email' | 'sms' | 'totp';
export type VerificationStep = 'method-selection' | 'code-entry' | 'success';
export type TriggerType = 'sign-in' | '2fa-setup' | 'security-action';

export interface MultiChannelVerificationProps {
  onSuccess?: () => void;
  onCancel?: () => void;
  triggerType?: TriggerType;
  userEmail?: string;
  userPhoneNumber?: string;
  availableMethods?: VerificationMethod[];
  preferredMethod?: VerificationMethod;
}

export interface VerificationState {
  currentStep: VerificationStep;
  selectedMethod: VerificationMethod | null;
  verificationCode: string;
  isLoading: boolean;
  error: string | null;
  timeLeft: number;
  canResend: boolean;
}

export interface TwoFactorSetupState {
  currentStep: 'method-selection' | 'email-setup' | 'sms-setup' | 'totp-setup' | 'backup-codes' | 'completion';
  enabledMethods: VerificationMethod[];
  backupCodes: string[];
  totpSecret?: string;
  totpQrCode?: string;
}

export interface SecuritySettings {
  twoFactorEnabled: boolean;
  enabledMethods: VerificationMethod[];
  hasBackupCodes: boolean;
  trustedDevices: TrustedDevice[];
  recentActivity: SecurityEvent[];
}

export interface TrustedDevice {
  id: string;
  name: string;
  lastUsed: Date;
  location?: string;
  deviceType: 'mobile' | 'desktop' | 'tablet';
}

export interface SecurityEvent {
  id: string;
  type: 'sign-in' | 'sign-out' | '2fa-enabled' | '2fa-disabled' | 'password-changed';
  timestamp: Date;
  location?: string;
  deviceInfo?: string;
}
