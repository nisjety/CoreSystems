export function formatError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === 'string') {
    return error;
  }
  return 'An unexpected error occurred';
}

export function isEmailVerificationError(error: unknown): boolean {
  return error instanceof Error && error.message.includes('Email not verified');
}

export function extractEmailFromError(error: unknown, fallbackEmail?: string): string | null {
  // This could be enhanced to extract email from specific error messages
  return fallbackEmail || null;
}
