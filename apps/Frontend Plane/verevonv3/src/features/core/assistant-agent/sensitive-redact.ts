/**
 * Sensitive-placeholder redaction for assistant submissions (slice 9).
 *
 * Before a submission leaves the SPA, text that looks like a secret is
 * replaced with a placeholder so it never reaches the model, the trace or
 * the audit trail. This is defence in depth beside the secret-custody ADR
 * (F-t2-024): it never stores, logs or transmits the original. Behind
 * VITE_ASSISTANT_REDACT=true; when off, text passes through unchanged.
 */

export const REDACTION_PLACEHOLDER = '[redacted]'

const patterns: RegExp[] = [
  // Bearer / API tokens and JWTs
  /\b(?:Bearer|Basic)\s+[A-Za-z0-9\-._~+/=]{8,}/gi,
  /\beyJ[A-Za-z0-9\-_]{10,}\.[A-Za-z0-9\-_]{10,}(?:\.[A-Za-z0-9\-_]{5,})?\b/g,
  // Common API-key shapes
  /\bsk-(?:proj-)?[A-Za-z0-9\-_]{16,}\b/g,
  /\bgh[pousr]_[A-Za-z0-9]{16,}\b/g,
  /\bAIza[A-Za-z0-9\-_]{20,}\b/g,
  /\bAKIA[A-Z0-9]{16}\b/g,
  // Password-ish assignments: password=hunter2, api_key: x, secret = ...
  /\b(password|passwd|pwd|secret|api[_-]?key|access[_-]?token|client[_-]?secret)\s*[:=]\s*\S+/gi,
  // Private keys
  /-----BEGIN (?:RSA |EC |OPENSSH |PGP )?PRIVATE KEY-----[\s\S]*?-----END (?:RSA |EC |OPENSSH |PGP )?PRIVATE KEY-----/g,
  // Email addresses and phone numbers (personal data)
  /\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/g,
  /\b(?:\+?\d[\s-]?){7,}\d\b/g,
]

/** The redacted text; identical to the input when no pattern matches. */
export function redactSensitive(text: string): string {
  let out = text
  for (const pattern of patterns) {
    pattern.lastIndex = 0
    out = out.replace(pattern, REDACTION_PLACEHOLDER)
  }
  return out
}

/** Apply redaction only when the build flag turns it on. */
export function maybeRedact(text: string): string {
  return import.meta.env.VITE_ASSISTANT_REDACT === 'true' ? redactSensitive(text) : text
}
