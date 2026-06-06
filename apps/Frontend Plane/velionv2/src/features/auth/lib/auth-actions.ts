"use client";

/**
 * Thin wrappers over the better-auth client for the auth-page flows that the
 * page itself shouldn't hardcode: password reset, email verification, SSO
 * sign-in, and WebAuthn capability detection (to gate the passkey button +
 * conditional autofill).
 */

import { authClient } from "@/lib/auth/auth-client";

type ActionResult = { error?: { message?: string } | null } | null | undefined;

function errorMessage(result: ActionResult, fallback: string): string | null {
  if (result && "error" in result && result.error) {
    return result.error.message || fallback;
  }
  return null;
}

/** Request a password-reset email. `redirectTo` is where the link lands (the reset form). */
export async function requestPasswordReset(email: string, redirectTo: string): Promise<string | null> {
  try {
    // better-auth 1.6 exposes `requestPasswordReset`; older builds used
    // `forgetPassword`. Resolve whichever exists at runtime.
    const client = authClient as unknown as {
      requestPasswordReset?: (input: { email: string; redirectTo: string }) => Promise<ActionResult>;
      forgetPassword?: (input: { email: string; redirectTo: string }) => Promise<ActionResult>;
    };
    const call = client.requestPasswordReset ?? client.forgetPassword;
    if (!call) return "Password reset is not available.";
    const result = await call({ email, redirectTo });
    return errorMessage(result, "Could not send reset email.");
  } catch (error) {
    return error instanceof Error ? error.message : "Could not send reset email.";
  }
}

/** Complete a password reset using the token from the email link. */
export async function resetPassword(newPassword: string, token: string): Promise<string | null> {
  try {
    const result = (await authClient.resetPassword({ newPassword, token })) as ActionResult;
    return errorMessage(result, "Could not reset password.");
  } catch (error) {
    return error instanceof Error ? error.message : "Could not reset password.";
  }
}

/** (Re)send the email-verification link. */
export async function sendVerificationEmail(email: string, callbackURL: string): Promise<string | null> {
  try {
    const client = authClient as unknown as {
      sendVerificationEmail?: (input: { email: string; callbackURL: string }) => Promise<ActionResult>;
    };
    if (!client.sendVerificationEmail) return null;
    const result = await client.sendVerificationEmail({ email, callbackURL });
    return errorMessage(result, "Could not send verification email.");
  } catch (error) {
    return error instanceof Error ? error.message : "Could not send verification email.";
  }
}

/**
 * Begin an SSO sign-in. Posts to the same-origin better-auth SSO endpoint
 * (proxied to auth-core) with a work email or org domain and follows the
 * returned IdP redirect URL. Returns an error message or null on success.
 */
export async function beginSsoSignIn(input: {
  email?: string;
  domain?: string;
  callbackURL: string;
}): Promise<string | null> {
  try {
    const response = await fetch("/api/auth/sign-in/sso", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "include",
      body: JSON.stringify({
        email: input.email?.trim() || undefined,
        domain: input.domain?.trim() || undefined,
        callbackURL: input.callbackURL,
      }),
    });
    const body = (await response.json().catch(() => null)) as
      | { url?: string; redirect?: boolean; error?: { message?: string } | string }
      | null;
    if (!response.ok) {
      const message = typeof body?.error === "object" ? body?.error?.message : body?.error;
      return message || "SSO is not available for this account.";
    }
    if (body?.url) {
      window.location.assign(body.url);
      return null;
    }
    return "SSO did not return a redirect.";
  } catch (error) {
    return error instanceof Error ? error.message : "SSO sign-in failed.";
  }
}

/** True when this browser can do WebAuthn (so we only show the passkey button when usable). */
export function detectWebAuthnSupport(): boolean {
  if (typeof window === "undefined") return false;
  return Boolean(
    window.PublicKeyCredential &&
      navigator.credentials &&
      typeof navigator.credentials.create === "function" &&
      typeof navigator.credentials.get === "function",
  );
}

/** Whether conditional-mediation (passkey autofill on the email field) is available. */
export async function isConditionalMediationAvailable(): Promise<boolean> {
  if (typeof window === "undefined" || !window.PublicKeyCredential) return false;
  try {
    const fn = (window.PublicKeyCredential as unknown as {
      isConditionalMediationAvailable?: () => Promise<boolean>;
    }).isConditionalMediationAvailable;
    return fn ? await fn() : false;
  } catch {
    return false;
  }
}
