import type { AuthFormData, AuthFormErrors } from '../types/auth';

function validateEmail(email: string): string {
  if (!email) {
    return 'E-postadresse er påkrevd';
  }
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
    return 'Ugyldig e-postadresse';
  }
  return '';
}

function validatePassword(password: string, isSignup = false): string {
  if (!password) {
    return 'Passord er påkrevd';
  }
  if (isSignup && password.length < 8) {
    return 'Passord må være minst 8 tegn';
  }
  return '';
}

function validateName(name: string): string {
  if (!name.trim()) {
    return 'Fullt navn er påkrevd';
  }
  return '';
}

function validateConfirmPassword(password: string, confirmPassword: string): string {
  if (!confirmPassword) {
    return 'Bekreft passord er påkrevd';
  }
  if (password !== confirmPassword) {
    return 'Passordene matcher ikke';
  }
  return '';
}

export function validateAuthForm(
  formData: AuthFormData,
  mode: 'signin' | 'signup' | 'forgot-password'
): { isValid: boolean; errors: AuthFormErrors } {
  const errors: AuthFormErrors = {
    email: '',
    password: '',
    name: '',
    confirmPassword: '',
  };

  // Email validation
  errors.email = validateEmail(formData.email);

  // Password validation (skip for forgot-password)
  if (mode !== 'forgot-password') {
    errors.password = validatePassword(formData.password, mode === 'signup');
  }

  // Name validation for signup
  if (mode === 'signup') {
    errors.name = validateName(formData.name);
    errors.confirmPassword = validateConfirmPassword(formData.password, formData.confirmPassword);
  }

  const isValid = !errors.email && !errors.password && !errors.name && !errors.confirmPassword;

  return { isValid, errors };
}
