import { requestJson } from '@/shared/api/http'

/**
 * Re-confirming the signed-in password before a high-risk approval is granted (build plan WS13
 * S5). The gateway refuses such a grant unless the person confirmed their password within the
 * last two minutes; the approval card asks for the confirmation before it sends the grant.
 */

/** A confirmation that was just made, and how long it stays good for the next grant. */
export interface StepUpProof {
  method: 'password'
  stepUpAt: string
  expiresAt: string
  expiresInSeconds: number
}

/**
 * Re-checks the signed-in password. A wrong password fails with an `ApiError` whose code is
 * `INVALID_PASSWORD`; the password itself is sent once and never kept.
 */
export function confirmStepUp(password: string): Promise<StepUpProof> {
  return requestJson<StepUpProof>('/api/v1/auth/step-up', {
    method: 'POST',
    body: JSON.stringify({ password }),
  })
}
