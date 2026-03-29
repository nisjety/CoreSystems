import type { VerificationMethod } from '../types/verification';

export function getMethodIcon(method: VerificationMethod) {
  const iconMap = {
    email: 'Mail',
    sms: 'Smartphone', 
    totp: 'Key'
  };
  return iconMap[method];
}

export function getMethodLabel(method: VerificationMethod): string {
  const labelMap = {
    email: 'Email Code',
    sms: 'SMS Code',
    totp: 'Authenticator App'
  };
  return labelMap[method];
}

export function getMethodDescription(method: VerificationMethod, email?: string, phone?: string): string {
  const descriptionMap = {
    email: `Get verification code via email${email ? ` to ${email}` : ''}`,
    sms: `Get verification code via SMS${phone ? ` to ${phone}` : ''}`,
    totp: 'Use your authenticator app to generate a code'
  };
  return descriptionMap[method];
}

export function formatVerificationCode(code: string): string {
  // Remove non-digits and limit to 6 characters
  return code.replace(/\D/g, '').slice(0, 6);
}

export function isValidVerificationCode(code: string): boolean {
  return /^\d{6}$/.test(code);
}

export function createResendTimer(seconds: number): number {
  return seconds;
}

export function formatTimeLeft(seconds: number): string {
  return `${seconds}s`;
}
